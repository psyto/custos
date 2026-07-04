//! Custos nested-CPI F2 case — why the verdict layer must read STATE, not
//! parse instructions.
//!
//! The malicious tx's only real instruction is a call to an unknown custom
//! program with 8 opaque bytes. That program CPIs SPL Token `Approve` inside
//! itself, handing an attacker unlimited control. An instruction parser that
//! inspects top-level instructions (a denylist of known-dangerous ops) sees
//! nothing to flag -> GREEN. Custos simulates and reads the account's
//! post-state delegate -> RED.
//!
//! Honest scope: a simulator that *also* enumerates inner instructions or
//! diffs approvals would catch this too. The point is architectural — Custos's
//! verdict is a STATE INVARIANT ("did any account I control gain a delegate /
//! change authority / lose value?"), which is pattern-agnostic and robust to
//! novel encodings, rather than a denylist of instruction shapes.

use std::path::PathBuf;
use std::str::FromStr;

use base64::Engine;
use litesvm::LiteSVM;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const MEMO: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

fn art() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts")
}
fn token_account_bytes(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut d = vec![0u8; 165];
    d[0..32].copy_from_slice(mint.as_ref());
    d[32..64].copy_from_slice(owner.as_ref());
    d[64..72].copy_from_slice(&amount.to_le_bytes());
    d[108] = 1; // state = Initialized
    d
}
fn read_amount(d: &[u8]) -> u64 {
    u64::from_le_bytes(d[64..72].try_into().unwrap())
}
fn read_delegate(d: &[u8]) -> Option<Pubkey> {
    if u32::from_le_bytes(d[72..76].try_into().unwrap()) == 1 {
        Some(Pubkey::try_from(&d[76..108]).unwrap())
    } else {
        None
    }
}
fn read_delegated_amount(d: &[u8]) -> u64 {
    u64::from_le_bytes(d[121..129].try_into().unwrap())
}
fn memo_ix(signer: Pubkey, text: &str) -> Instruction {
    Instruction {
        program_id: Pubkey::from_str(MEMO).unwrap(),
        accounts: vec![AccountMeta::new_readonly(signer, true)],
        data: text.as_bytes().to_vec(),
    }
}
fn approve_ix(token: Pubkey, source: Pubkey, delegate: Pubkey, owner: Pubkey, amount: u64) -> Instruction {
    let mut data = vec![4u8];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: token,
        accounts: vec![
            AccountMeta::new(source, false),
            AccountMeta::new_readonly(delegate, false),
            AccountMeta::new_readonly(owner, true),
        ],
        data,
    }
}

/// "Simulator C": an instruction parser / denylist. It inspects the top-level
/// instructions and flags only known-dangerous SPL Token ops. This is the
/// class of detector that does NOT judge by simulated end-state.
fn parser_verdict(ixs: &[Instruction], token: &Pubkey) -> (&'static str, String) {
    for ix in ixs {
        if ix.program_id == *token {
            match ix.data.first() {
                Some(4) => return ("RED", "top-level SPL Token Approve".into()),
                Some(6) => return ("RED", "top-level SPL Token SetAuthority".into()),
                _ => {}
            }
        }
    }
    ("GREEN", "no known-dangerous top-level instruction".into())
}

fn set_token_account(svm: &mut LiteSVM, token: Pubkey, pk: Pubkey, mint: &Pubkey, owner: &Pubkey, amount: u64) {
    svm.set_account(
        pk,
        Account { lamports: 2_039_280, data: token_account_bytes(mint, owner, amount), owner: token, executable: false, rent_epoch: u64::MAX },
    )
    .unwrap();
}

fn main() {
    println!("=== Custos nested-CPI F2: state-invariant vs instruction-parser ===\n");
    let mut svm = LiteSVM::new();
    let token = Pubkey::from_str(TOKEN).unwrap();
    let usdc = Pubkey::from_str(USDC_MINT).unwrap();

    let mint_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(art().join("usdc_mint.json")).unwrap()).unwrap();
    let mdata = base64::engine::general_purpose::STANDARD
        .decode(mint_json["account"]["data"][0].as_str().unwrap())
        .unwrap();
    svm.set_account(usdc, Account { lamports: 1_000_000, data: mdata, owner: token, executable: false, rent_epoch: u64::MAX }).unwrap();
    svm.add_program(token, &std::fs::read(art().join("spl_token.so")).unwrap()).unwrap();
    svm.add_program(Pubkey::from_str(MEMO).unwrap(), &std::fs::read(art().join("memo.so")).unwrap()).unwrap();

    // The attacker's custom program — to Custos and to a parser it is just an
    // unknown program id.
    let proxy = Keypair::new().pubkey();
    svm.add_program(proxy, &std::fs::read(art().join("proxy_approve.so")).unwrap()).unwrap();

    let user = Keypair::new();
    let attacker = Keypair::new();
    svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let user_ata = Keypair::new().pubkey();
    let attacker_ata = Keypair::new().pubkey();
    set_token_account(&mut svm, token, user_ata, &usdc, &user.pubkey(), 1_000_000_000);
    set_token_account(&mut svm, token, attacker_ata, &usdc, &attacker.pubkey(), 0);

    // ---- Non-strawman check: the parser DOES catch a direct top-level Approve.
    let direct = vec![approve_ix(token, user_ata, attacker.pubkey(), user.pubkey(), u64::MAX)];
    let (pd, pr) = parser_verdict(&direct, &token);
    println!("sanity: instruction-parser on a DIRECT top-level Approve -> {pd} ({pr})");
    println!("        (so the parser is real, not a strawman)\n");

    // ---- The nested-CPI malicious tx ----
    // Only real instruction = call the unknown proxy program with 8 bytes.
    let proxy_ix = Instruction {
        program_id: proxy,
        accounts: vec![
            AccountMeta::new(user_ata, false),
            AccountMeta::new_readonly(attacker.pubkey(), false),
            AccountMeta::new_readonly(user.pubkey(), true),
            AccountMeta::new_readonly(token, false),
        ],
        data: u64::MAX.to_le_bytes().to_vec(),
    };
    let ixs = vec![memo_ix(user.pubkey(), "\u{1F381} Claim your ARDR airdrop"), proxy_ix];

    // Simulator C (instruction parser) on the actual tx:
    let (c_verdict, c_reason) = parser_verdict(&ixs, &token);
    println!("--- Simulator C: instruction parser (top-level denylist) ---");
    println!("  top-level programs: Memo + <unknown program {}>", &proxy.to_string()[..8]);
    println!("  => VERDICT: {c_verdict}  ({c_reason})\n");

    // Execute + Custos F2 state invariant:
    let pre = svm.get_account(&user_ata).unwrap().data;
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&user.pubkey()), &[&user], svm.latest_blockhash());
    svm.send_transaction(tx).expect("nested-CPI tx executes");
    let post = svm.get_account(&user_ata).unwrap().data;

    let delegate = read_delegate(&post);
    let delegated = read_delegated_amount(&post);
    println!("--- Simulator B: Custos F2 (post-sim state invariant) ---");
    println!("  USDC balance   : {} -> {} (unchanged)", read_amount(&pre) as f64 / 1e6, read_amount(&post) as f64 / 1e6);
    println!("  post delegate  : {delegate:?}");
    println!("  delegated amt  : {}{}", delegated, if delegated == u64::MAX { " (UNLIMITED)" } else { "" });
    let f2_fires = delegate == Some(attacker.pubkey());
    let f2 = if f2_fires { "RED" } else { "GREEN" };
    println!("  => VERDICT: {f2}  (\"an unknown program granted an unknown address control of ALL your USDC\")\n");

    // Prove it: drain one tx later.
    let drain = Transaction::new_signed_with_payer(
        &[approve_then_transfer_drain(token, user_ata, attacker_ata, attacker.pubkey(), read_amount(&post))],
        Some(&attacker.pubkey()),
        &[&attacker],
        svm.latest_blockhash(),
    );
    let drained = svm.send_transaction(drain).is_ok();
    let user_final = read_amount(&svm.get_account(&user_ata).unwrap().data);
    println!("one tx later: attacker (delegate) drains -> success={drained}, user USDC final={}", user_final as f64 / 1e6);

    let proven = c_verdict == "GREEN" && f2 == "RED" && drained && user_final == 0;
    println!(
        "\n=== NESTED-CPI F2: {} ===",
        if proven {
            "PROVEN — instruction-parser said SAFE; Custos state-invariant said DANGER"
        } else {
            "NOT PROVEN"
        }
    );
}

// SPL Token Transfer by the delegate (tag 3).
fn approve_then_transfer_drain(token: Pubkey, source: Pubkey, dest: Pubkey, authority: Pubkey, amount: u64) -> Instruction {
    let mut data = vec![3u8];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: token,
        accounts: vec![
            AccountMeta::new(source, false),
            AccountMeta::new(dest, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data,
    }
}
