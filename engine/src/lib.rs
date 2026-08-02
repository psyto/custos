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

pub mod loader;
pub mod scenarios;
pub mod spl;
pub mod sim;

pub fn spl_token_id() -> Pubkey {
    Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
}
pub fn token2022_id() -> Pubkey {
    Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap()
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
    /// Parse an SPL Token or Token-2022 account. The base 165-byte layout is
    /// identical for both; Token-2022 accounts may carry TLV extensions past
    /// byte 165 (with an account-type byte == 2 at offset 165, which we use to
    /// avoid mis-parsing a Token-2022 mint as an account).
    pub fn parse(s: &AccountSnapshot) -> Option<TokenAccount> {
        let is_token = s.owner == spl_token_id() || s.owner == token2022_id();
        if !is_token || s.data.len() < 165 {
            return None;
        }
        if s.data.len() > 165 && s.data.get(165) != Some(&2) {
            return None; // Token-2022 mint or non-account TLV blob
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
        self.pre.get(pk).and_then(|o| o.as_ref()).and_then(TokenAccount::parse)
    }
    fn post_token(&self, pk: &Pubkey) -> Option<TokenAccount> {
        self.post.get(pk).and_then(|o| o.as_ref()).and_then(TokenAccount::parse)
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

/// M1 — realized token value leaving the user's accounts must remain within
/// the shared, authored mandate. This is deliberately separate from F1-F5:
/// approvals and authority changes move no value in the current transaction.
pub struct MandateConformance {
    pub mandate: reexec_spec::MandateSpec,
}

impl Invariant for MandateConformance {
    fn code(&self) -> &'static str {
        "M1-mandate"
    }

    fn check(&self, o: &Outcome) -> Vec<Finding> {
        let mut outflows: BTreeMap<Pubkey, (u64, Pubkey)> = BTreeMap::new();

        for pk in o.keys().cloned().collect::<Vec<_>>() {
            let Some(pre) = o.pre_token(&pk) else { continue };
            if pre.owner != o.user {
                continue;
            }

            let post_amount = o.post_token(&pk).map(|post| post.amount).unwrap_or(0);
            let outflow = pre.amount.saturating_sub(post_amount);
            if outflow == 0 {
                continue;
            }

            let entry = outflows.entry(pre.mint).or_insert((0, pk));
            entry.0 = entry.0.saturating_add(outflow);
        }

        outflows
            .into_iter()
            .filter_map(|(mint, (outflow, account))| {
                (outflow > self.mandate.max_value_out).then(|| Finding {
                    level: Level::Red,
                    code: self.code(),
                    account,
                    message: format!(
                        "moves {outflow} of {mint} out of your accounts; authored mandate allows at most {}",
                        self.mandate.max_value_out,
                    ),
                })
            })
            .collect()
    }
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
            if let Some(ca) = post.close_authority {
                if post.close_authority != pre.close_authority && ca != o.user {
                    f.push(Finding {
                        level: Level::Red,
                        code: self.code(),
                        account: pk,
                        message: format!("close-authority granted to {} (can close and sweep the account)", short(&ca)),
                    });
                }
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

/// F4 — a token account the user controls is closed / deallocated while it
/// still held tokens (the "empty-then-close" rent-and-account grab; also any
/// path that removes an account you own).
pub struct F4AccountClose;
impl Invariant for F4AccountClose {
    fn code(&self) -> &'static str {
        "F4-close"
    }
    fn check(&self, o: &Outcome) -> Vec<Finding> {
        let mut f = vec![];
        for pk in o.keys().cloned().collect::<Vec<_>>() {
            let Some(pre) = o.pre_token(&pk) else { continue };
            if pre.owner != o.user || pre.amount == 0 {
                continue;
            }
            // Still the user's token account afterwards? If not, it was closed.
            if o.post_token(&pk).is_none() {
                f.push(Finding {
                    level: Level::Red,
                    code: self.code(),
                    account: pk,
                    message: format!("your token account was closed/deallocated while holding {} tokens", pre.amount),
                });
            }
        }
        f
    }
}

/// Programs a firewall treats as known/verified. Anything else the tx invokes
/// is surfaced as a caution (F5).
const PROGRAM_ALLOWLIST: &[&str] = &[
    "11111111111111111111111111111111",             // System
    "ComputeBudget111111111111111111111111111111",  // ComputeBudget
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",   // SPL Token
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",   // Token-2022
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",  // Associated Token Account
    "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",   // Memo v2
    "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo",   // Memo v1
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",   // Jupiter v6
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",  // Raydium AMM v4
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",  // Raydium CLMM
    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",   // Orca Whirlpool
    "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",   // Meteora DLMM
    "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY",   // Phoenix
];

/// F5 — the transaction invokes a program that isn't on the verified allowlist.
/// A caution (not a block) on its own; the real drainer signal is usually the
/// state change (F2/F3/F4) such a program produces.
pub struct F5UnknownProgram;
impl Invariant for F5UnknownProgram {
    fn code(&self) -> &'static str {
        "F5-unknown-program"
    }
    fn check(&self, o: &Outcome) -> Vec<Finding> {
        let mut seen = std::collections::BTreeSet::new();
        let mut f = vec![];
        for l in &o.logs {
            let Some(rest) = l.strip_prefix("Program ") else { continue };
            let Some(idx) = rest.find(" invoke [") else { continue };
            let id = &rest[..idx];
            if !PROGRAM_ALLOWLIST.contains(&id) && seen.insert(id.to_string()) {
                let pk = Pubkey::from_str(id).unwrap_or_default();
                f.push(Finding {
                    level: Level::Yellow,
                    code: self.code(),
                    account: pk,
                    message: format!("interacts with an unverified program {}", short(&pk)),
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
    vec![
        Box::new(F1Drain),
        Box::new(F2DelegateGrant),
        Box::new(F3AuthorityChange),
        Box::new(F4AccountClose),
        Box::new(F5UnknownProgram),
    ]
}

/// The normal malice-tier checks plus a shared, authored spend mandate.
pub fn bank_with_mandate(mandate: reexec_spec::MandateSpec) -> Vec<Box<dyn Invariant>> {
    let mut b = default_bank();
    b.push(Box::new(MandateConformance { mandate }));
    b
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
    fn f4_fires_on_close_while_funded() {
        let (u, mint, pk) = (user(), user(), user());
        let tok = spl_token_id();
        let pre = token_bytes(&mint, &u, 1000, None, None);
        let o = Outcome {
            user: u,
            pre: BTreeMap::from([(pk, Some(AccountSnapshot { lamports: 2_039_280, owner: tok, data: pre }))]),
            post: BTreeMap::from([(pk, None)]), // account closed / deallocated
            logs: vec![],
            success: true,
            token_id: tok,
            system_id: system_id(),
        };
        let v = evaluate(&o, &default_bank());
        assert_eq!(v.level, Level::Red);
        assert!(v.findings.iter().any(|f| f.code == "F4-close"));
    }

    #[test]
    fn f5_flags_unknown_program_but_not_known() {
        let tok = spl_token_id();
        let mk = |id: &str| Outcome {
            user: user(),
            pre: BTreeMap::new(),
            post: BTreeMap::new(),
            logs: vec![format!("Program {id} invoke [1]")],
            success: true,
            token_id: tok,
            system_id: system_id(),
        };
        let unknown = mk("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin");
        assert_eq!(evaluate(&unknown, &default_bank()).level, Level::Yellow);
        assert!(evaluate(&unknown, &default_bank()).findings.iter().any(|f| f.code == "F5-unknown-program"));

        let known = mk("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        assert_eq!(evaluate(&known, &default_bank()).level, Level::Green);
    }

    #[test]
    fn f2_fires_on_token2022_account() {
        let (u, mint, atk, pk) = (user(), user(), user(), user());
        let t22 = token2022_id();
        let snap = |data| Some(AccountSnapshot { lamports: 2_039_280, owner: t22, data });
        let o = Outcome {
            user: u,
            pre: BTreeMap::from([(pk, snap(token_bytes(&mint, &u, 1000, None, None)))]),
            post: BTreeMap::from([(pk, snap(token_bytes(&mint, &u, 1000, Some((&atk, u64::MAX)), None)))]),
            logs: vec![],
            success: true,
            token_id: spl_token_id(),
            system_id: system_id(),
        };
        let v = evaluate(&o, &default_bank());
        assert_eq!(v.level, Level::Red);
        assert!(v.findings.iter().any(|f| f.code == "F2-delegate"));
    }

    #[test]
    fn token2022_mint_not_parsed_as_account() {
        let t22 = token2022_id();
        // extensions present, type byte = Mint(1) → must NOT parse as an account
        let mut mint = vec![0u8; 170];
        mint[165] = 1;
        assert!(TokenAccount::parse(&AccountSnapshot { lamports: 1, owner: t22, data: mint }).is_none());
        // extensions present, type byte = Account(2) → parses
        let mut acct = vec![0u8; 170];
        acct[108] = 1;
        acct[165] = 2;
        assert!(TokenAccount::parse(&AccountSnapshot { lamports: 1, owner: t22, data: acct }).is_some());
    }

    #[test]
    fn delegate_to_self_is_not_flagged() {
        let (u, mint, pk) = (user(), user(), user());
        let pre = token_bytes(&mint, &u, 1000, None, None);
        let post = token_bytes(&mint, &u, 1000, Some((&u, 500)), None);
        let o = outcome(u, pk, pre, post);
        assert_eq!(evaluate(&o, &default_bank()).level, Level::Green);
    }

    fn mandate(max_value_out: u64) -> reexec_spec::MandateSpec {
        reexec_spec::MandateSpec {
            max_size: reexec_spec::MAX_MANDATE_SIZE,
            instrument: reexec_spec::MANDATE_INSTRUMENT,
            max_value_out,
        }
    }

    #[test]
    fn m1_flags_outflow_above_authored_limit_but_allows_below_it() {
        let (u, mint, pk) = (user(), user(), user());
        let o = outcome(
            u,
            pk,
            token_bytes(&mint, &u, 1_000, None, None),
            token_bytes(&mint, &u, 200, None, None),
        );

        let red = evaluate(&o, &bank_with_mandate(mandate(500)));
        assert_eq!(red.level, Level::Red);
        assert!(red.findings.iter().any(|f| f.code == "M1-mandate"));

        let allowed = evaluate(&o, &bank_with_mandate(mandate(900)));
        assert!(!allowed.findings.iter().any(|f| f.code == "M1-mandate"));
    }

    #[test]
    fn m1_sums_outflow_across_user_accounts_of_the_same_mint() {
        let (u, mint, first, second) = (user(), user(), user(), user());
        let tok = spl_token_id();
        let snap = |data| Some(AccountSnapshot { lamports: 2_039_280, owner: tok, data });
        let o = Outcome {
            user: u,
            pre: BTreeMap::from([
                (first, snap(token_bytes(&mint, &u, 400, None, None))),
                (second, snap(token_bytes(&mint, &u, 400, None, None))),
            ]),
            post: BTreeMap::from([
                (first, snap(token_bytes(&mint, &u, 100, None, None))),
                (second, snap(token_bytes(&mint, &u, 300, None, None))),
            ]),
            logs: vec![],
            success: true,
            token_id: tok,
            system_id: system_id(),
        };

        let verdict = evaluate(&o, &bank_with_mandate(mandate(350)));
        let finding = verdict.findings.iter().find(|f| f.code == "M1-mandate").unwrap();
        assert!(finding.message.contains("moves 400"));
    }

    #[test]
    fn m1_ignores_delegate_only_transaction() {
        let (u, mint, delegate, pk) = (user(), user(), user(), user());
        let o = outcome(
            u,
            pk,
            token_bytes(&mint, &u, 1_000, None, None),
            token_bytes(&mint, &u, 1_000, Some((&delegate, u64::MAX)), None),
        );

        let verdict = evaluate(&o, &bank_with_mandate(mandate(0)));
        assert!(!verdict.findings.iter().any(|f| f.code == "M1-mandate"));
        assert!(verdict.findings.iter().any(|f| f.code == "F2-delegate"));
    }

    #[test]
    fn m1_counts_a_closed_funded_account_as_full_outflow() {
        let (u, mint, pk) = (user(), user(), user());
        let tok = spl_token_id();
        let pre = token_bytes(&mint, &u, 1_000, None, None);
        let o = Outcome {
            user: u,
            pre: BTreeMap::from([(pk, Some(AccountSnapshot { lamports: 2_039_280, owner: tok, data: pre }))]),
            post: BTreeMap::from([(pk, None)]),
            logs: vec![],
            success: true,
            token_id: tok,
            system_id: system_id(),
        };

        let verdict = evaluate(&o, &bank_with_mandate(mandate(999)));
        assert!(verdict.findings.iter().any(|f| f.code == "M1-mandate"));
    }

    #[test]
    fn m1_stage0_default_is_inert() {
        let (u, mint, pk) = (user(), user(), user());
        let o = outcome(
            u,
            pk,
            token_bytes(&mint, &u, u64::MAX, None, None),
            token_bytes(&mint, &u, 0, None, None),
        );

        let verdict = evaluate(&o, &bank_with_mandate(reexec_spec::MandateSpec::stage0_default()));
        assert!(!verdict.findings.iter().any(|f| f.code == "M1-mandate"));
    }
}
