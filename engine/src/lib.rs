//! Custos execution-firewall engine.
//!
//! Pipeline: simulate a prospective transaction against cloned mainnet state,
//! snapshot the accounts the user controls before/after, then run
//! user-protective invariants over the [`Outcome`] and aggregate a [`Verdict`].
//!
//! The verdict layer judges by STATE ("did an account I control gain a
//! delegate / change authority / lose value?"), not by parsing instruction
//! shapes — pattern-agnostic and robust to novel encodings (see the Stage 0
//! `proof_f2_nested` demonstration).

use std::collections::BTreeMap;
use std::str::FromStr;

use solana_pubkey::Pubkey;

pub mod scenarios;
pub mod spl;
pub mod sim;

pub fn spl_token_id() -> Pubkey {
    Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
}
pub fn system_id() -> Pubkey {
    Pubkey::from_str("11111111111111111111111111111111").unwrap()
}

/// A single account's state at one point in time.
#[derive(Clone, Debug)]
pub struct AccountSnapshot {
    pub lamports: u64,
    pub owner: Pubkey,
    pub data: Vec<u8>,
}

/// Parsed SPL Token account (the fields the invariants care about).
#[derive(Clone, Debug)]
pub struct TokenAccount {
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub delegate: Option<Pubkey>,
    pub delegated_amount: u64,
    pub close_authority: Option<Pubkey>,
}

impl TokenAccount {
    /// Parse a 165-byte SPL Token account owned by the token program.
    pub fn parse(s: &AccountSnapshot, token_id: &Pubkey) -> Option<TokenAccount> {
        if s.owner != *token_id || s.data.len() != 165 {
            return None;
        }
        let d = &s.data;
        let coption = |tag_off: usize| -> Option<Pubkey> {
            if u32::from_le_bytes(d[tag_off..tag_off + 4].try_into().unwrap()) == 1 {
                Some(Pubkey::try_from(&d[tag_off + 4..tag_off + 36]).unwrap())
            } else {
                None
            }
        };
        Some(TokenAccount {
            mint: Pubkey::try_from(&d[0..32]).unwrap(),
            owner: Pubkey::try_from(&d[32..64]).unwrap(),
            amount: u64::from_le_bytes(d[64..72].try_into().unwrap()),
            delegate: coption(72),
            delegated_amount: u64::from_le_bytes(d[121..129].try_into().unwrap()),
            close_authority: coption(129),
        })
    }
}

/// Result of simulating one transaction, with everything the invariants read.
pub struct Outcome {
    /// The wallet being protected (the fee payer / signer of the prospective tx).
    pub user: Pubkey,
    pub pre: BTreeMap<Pubkey, Option<AccountSnapshot>>,
    pub post: BTreeMap<Pubkey, Option<AccountSnapshot>>,
    pub logs: Vec<String>,
    pub success: bool,
    pub token_id: Pubkey,
    pub system_id: Pubkey,
}

impl Outcome {
    fn keys(&self) -> impl Iterator<Item = &Pubkey> {
        // union of pre/post keys (pre is a superset in practice; be safe)
        self.pre.keys().chain(self.post.keys().filter(move |k| !self.pre.contains_key(*k)))
    }
    fn pre_token(&self, pk: &Pubkey) -> Option<TokenAccount> {
        self.pre.get(pk).and_then(|o| o.as_ref()).and_then(|s| TokenAccount::parse(s, &self.token_id))
    }
    fn post_token(&self, pk: &Pubkey) -> Option<TokenAccount> {
        self.post.get(pk).and_then(|o| o.as_ref()).and_then(|s| TokenAccount::parse(s, &self.token_id))
    }
}

/// Severity, ordered Green < Info < Yellow < Red.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Green,
    Info,
    Yellow,
    Red,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub level: Level,
    pub code: &'static str,
    pub account: Pubkey,
    pub message: String,
}

pub trait Invariant {
    fn code(&self) -> &'static str;
    fn check(&self, o: &Outcome) -> Vec<Finding>;
}

/// F2 — a token account the user controls gains (or increases) a delegate.
/// Delegation moves no funds in-tx but grants future control (drainer pattern).
pub struct F2DelegateGrant;
impl Invariant for F2DelegateGrant {
    fn code(&self) -> &'static str {
        "F2-delegate"
    }
    fn check(&self, o: &Outcome) -> Vec<Finding> {
        let mut f = vec![];
        for pk in o.keys().cloned().collect::<Vec<_>>() {
            let (Some(pre), Some(post)) = (o.pre_token(&pk), o.post_token(&pk)) else { continue };
            if pre.owner != o.user {
                continue;
            }
            let granted = match (pre.delegate, post.delegate) {
                (_, Some(d)) if d != o.user && (pre.delegate != Some(d) || post.delegated_amount > pre.delegated_amount) => Some(d),
                _ => None,
            };
            if let Some(d) = granted {
                let unlimited = post.delegated_amount == u64::MAX || post.delegated_amount >= post.amount;
                f.push(Finding {
                    level: Level::Red,
                    code: self.code(),
                    account: pk,
                    message: format!(
                        "grants delegate {} authority over {} of your tokens{}",
                        short(&d),
                        if unlimited { "ALL".into() } else { post.delegated_amount.to_string() },
                        if unlimited { " (unlimited)" } else { "" },
                    ),
                });
            }
        }
        f
    }
}

/// F3 — the owner or close-authority of a token account the user controls is
/// reassigned away from the user (SetAuthority). No balance moves; the user
/// simply loses the account.
pub struct F3AuthorityChange;
impl Invariant for F3AuthorityChange {
    fn code(&self) -> &'static str {
        "F3-authority"
    }
    fn check(&self, o: &Outcome) -> Vec<Finding> {
        let mut f = vec![];
        for pk in o.keys().cloned().collect::<Vec<_>>() {
            let (Some(pre), Some(post)) = (o.pre_token(&pk), o.post_token(&pk)) else { continue };
            if pre.owner != o.user {
                continue;
            }
            if post.owner != o.user {
                f.push(Finding {
                    level: Level::Red,
                    code: self.code(),
                    account: pk,
                    message: format!("ownership of your token account transferred to {}", short(&post.owner)),
                });
            }
            if post.close_authority.is_some() && post.close_authority != pre.close_authority && post.close_authority != Some(o.user) {
                f.push(Finding {
                    level: Level::Red,
                    code: self.code(),
                    account: pk,
                    message: format!("close-authority granted to {} (can close and sweep the account)", short(&post.close_authority.unwrap())),
                });
            }
        }
        f
    }
}

/// F1 — a token account the user controls is fully emptied (balance -> 0).
pub struct F1Drain;
impl Invariant for F1Drain {
    fn code(&self) -> &'static str {
        "F1-drain"
    }
    fn check(&self, o: &Outcome) -> Vec<Finding> {
        let mut f = vec![];
        for pk in o.keys().cloned().collect::<Vec<_>>() {
            let (Some(pre), Some(post)) = (o.pre_token(&pk), o.post_token(&pk)) else { continue };
            if pre.owner != o.user {
                continue;
            }
            if pre.amount > 0 && post.amount == 0 {
                f.push(Finding {
                    level: Level::Red,
                    code: self.code(),
                    account: pk,
                    message: format!("token account fully drained ({} -> 0)", pre.amount),
                });
            }
        }
        f
    }
}

#[derive(Debug)]
pub struct Verdict {
    pub level: Level,
    pub findings: Vec<Finding>,
}

pub fn default_bank() -> Vec<Box<dyn Invariant>> {
    vec![Box::new(F1Drain), Box::new(F2DelegateGrant), Box::new(F3AuthorityChange)]
}

pub fn evaluate(o: &Outcome, bank: &[Box<dyn Invariant>]) -> Verdict {
    let mut findings = vec![];
    for inv in bank {
        findings.extend(inv.check(o));
    }
    let level = findings.iter().map(|f| f.level).max().unwrap_or(Level::Green);
    Verdict { level, findings }
}

/// Models a "diff the balances only" scanner: it warns only when the user's
/// token balance decreases. It is blind to delegation and authority changes
/// (no balance moves in-tx), which is exactly the Custos gap.
pub fn naive_balance_diff(o: &Outcome) -> (Level, Vec<String>) {
    let mut notes = vec![];
    let mut worst = Level::Green;
    for pk in o.keys().cloned().collect::<Vec<_>>() {
        let (Some(pre), Some(post)) = (o.pre_token(&pk), o.post_token(&pk)) else { continue };
        if pre.owner != o.user {
            continue;
        }
        if post.amount < pre.amount {
            worst = worst.max(Level::Yellow);
            notes.push(format!("you send {} tokens from {}", pre.amount - post.amount, short(&pk)));
        }
    }
    if notes.is_empty() {
        notes.push("no token balance leaves your wallet".into());
    }
    (worst, notes)
}

pub fn short(pk: &Pubkey) -> String {
    let s = pk.to_string();
    format!("{}…{}", &s[..4], &s[s.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> Pubkey {
        Pubkey::new_unique()
    }
    fn token_bytes(mint: &Pubkey, owner: &Pubkey, amount: u64, delegate: Option<(&Pubkey, u64)>, new_owner: Option<&Pubkey>) -> Vec<u8> {
        let mut d = vec![0u8; 165];
        d[0..32].copy_from_slice(mint.as_ref());
        d[32..64].copy_from_slice(new_owner.unwrap_or(owner).as_ref());
        d[64..72].copy_from_slice(&amount.to_le_bytes());
        d[108] = 1;
        if let Some((del, amt)) = delegate {
            d[72..76].copy_from_slice(&1u32.to_le_bytes());
            d[76..108].copy_from_slice(del.as_ref());
            d[121..129].copy_from_slice(&amt.to_le_bytes());
        }
        d
    }
    fn outcome(user: Pubkey, pk: Pubkey, pre: Vec<u8>, post: Vec<u8>) -> Outcome {
        let tok = spl_token_id();
        let snap = |data| Some(AccountSnapshot { lamports: 2_039_280, owner: tok, data });
        Outcome {
            user,
            pre: BTreeMap::from([(pk, snap(pre))]),
            post: BTreeMap::from([(pk, snap(post))]),
            logs: vec![],
            success: true,
            token_id: tok,
            system_id: system_id(),
        }
    }

    #[test]
    fn f2_fires_on_new_unlimited_delegate() {
        let (u, mint, atk, pk) = (user(), user(), user(), user());
        let pre = token_bytes(&mint, &u, 1000, None, None);
        let post = token_bytes(&mint, &u, 1000, Some((&atk, u64::MAX)), None);
        let o = outcome(u, pk, pre, post);
        let v = evaluate(&o, &default_bank());
        assert_eq!(v.level, Level::Red);
        assert!(v.findings.iter().any(|f| f.code == "F2-delegate"));
        // balance-diff is blind: no amount change
        assert_eq!(naive_balance_diff(&o).0, Level::Green);
    }

    #[test]
    fn f3_fires_on_ownership_transfer() {
        let (u, mint, atk, pk) = (user(), user(), user(), user());
        let pre = token_bytes(&mint, &u, 1000, None, None);
        let post = token_bytes(&mint, &u, 1000, None, Some(&atk)); // owner -> attacker
        let o = outcome(u, pk, pre, post);
        let v = evaluate(&o, &default_bank());
        assert_eq!(v.level, Level::Red);
        assert!(v.findings.iter().any(|f| f.code == "F3-authority"));
        assert_eq!(naive_balance_diff(&o).0, Level::Green);
    }

    #[test]
    fn benign_no_change_is_green() {
        let (u, mint, pk) = (user(), user(), user());
        let same = token_bytes(&mint, &u, 1000, None, None);
        let o = outcome(u, pk, same.clone(), same);
        assert_eq!(evaluate(&o, &default_bank()).level, Level::Green);
    }

    #[test]
    fn delegate_to_self_is_not_flagged() {
        let (u, mint, pk) = (user(), user(), user());
        let pre = token_bytes(&mint, &u, 1000, None, None);
        let post = token_bytes(&mint, &u, 1000, Some((&u, 500)), None);
        let o = outcome(u, pk, pre, post);
        assert_eq!(evaluate(&o, &default_bank()).level, Level::Green);
    }
}
