//! Custos Stage 1 demo — three prospective transactions judged by the SAME
//! engine, next to a balance-diff-only scanner.
//!
//!   1. benign "claim" (memo)            -> both GREEN
//!   2. hidden delegate (Approve MAX)    -> balance-diff GREEN, Custos RED (F2)
//!   3. silent ownership theft (SetAuth) -> balance-diff GREEN, Custos RED (F3)
//!
//! Run: cargo run --bin demo   (needs ../gate/artifacts: usdc_mint.json,
//! spl_token.so, memo.so)

use std::path::PathBuf;
use std::str::FromStr;

use base64::Engine as _;
use custos_engine::{default_bank, evaluate, naive_balance_diff, short, spl, Level, Verdict};
use litesvm::LiteSVM;
use solana_account::Account;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

const MEMO: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

fn art() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../gate/artifacts")
}

struct Env {
    svm: LiteSVM,
    user: Keypair,
    attacker: Keypair,
    user_ata: Pubkey,
    attacker_ata: Pubkey,
    token: Pubkey,
}

fn fresh_env() -> Env {
    let token = custos_engine::spl_token_id();
    let usdc = Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
    let mut svm = LiteSVM::new();

    let mint_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(art().join("usdc_mint.json")).unwrap()).unwrap();
    let mdata = base64::engine::general_purpose::STANDARD
        .decode(mint_json["account"]["data"][0].as_str().unwrap())
        .unwrap();
    svm.set_account(usdc, Account { lamports: 1_000_000, data: mdata, owner: token, executable: false, rent_epoch: u64::MAX }).unwrap();
    svm.add_program(token, &std::fs::read(art().join("spl_token.so")).unwrap()).unwrap();
    svm.add_program(Pubkey::from_str(MEMO).unwrap(), &std::fs::read(art().join("memo.so")).unwrap()).unwrap();

    let user = Keypair::new();
    let attacker = Keypair::new();
    svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    let user_ata = Keypair::new().pubkey();
    let attacker_ata = Keypair::new().pubkey();
    let set = |svm: &mut LiteSVM, pk, owner: &Pubkey, amount| {
        svm.set_account(pk, Account { lamports: 2_039_280, data: spl::token_account_bytes(&usdc, owner, amount), owner: token, executable: false, rent_epoch: u64::MAX }).unwrap();
    };
    set(&mut svm, user_ata, &user.pubkey(), 1_000_000_000);
    set(&mut svm, attacker_ata, &attacker.pubkey(), 0);

    Env { svm, user, attacker, user_ata, attacker_ata, token }
}

fn render(l: Level) -> &'static str {
    match l {
        Level::Green => "GREEN ✅",
        Level::Info => "INFO",
        Level::Yellow => "WARN ⚠",
        Level::Red => "RED ⛔",
    }
}

fn run(title: &str, subtitle: &str, e: &mut Env, ixs: Vec<solana_instruction::Instruction>) {
    let watch = vec![e.user_ata, e.attacker_ata, e.user.pubkey()];
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&e.user.pubkey()), &[&e.user], e.svm.latest_blockhash());
    let outcome = custos_engine::sim::capture(&mut e.svm, tx, e.user.pubkey(), &watch, e.token, custos_engine::system_id());

    let (naive_level, naive_notes) = naive_balance_diff(&outcome);
    let Verdict { level, findings } = evaluate(&outcome, &default_bank());

    println!("\n────────────────────────────────────────────────────────");
    println!("SCENARIO: {title}");
    println!("  {subtitle}");
    println!("  ─ Balance-diff scanner : {}", render(naive_level));
    for n in &naive_notes {
        println!("      · {n}");
    }
    println!("  ─ Custos engine        : {}", render(level));
    if findings.is_empty() {
        println!("      · no user-protective invariant fired");
    }
    for f in &findings {
        println!("      · [{}] {} — {}", f.code, short(&f.account), f.message);
    }
    let contrast = if level > naive_level { "  ⇒ Custos caught what balance-diff missed" } else { "  ⇒ agree" };
    println!("{contrast}");
}

fn main() {
    println!("=== Custos Stage 1 engine demo: 3 prospective txs, one engine ===");

    // 1. Benign claim (memo only).
    let mut e = fresh_env();
    let memo = Pubkey::from_str(MEMO).unwrap();
    let ukey = e.user.pubkey();
    run(
        "Benign airdrop claim",
        "a memo; no funds move, no authority changes",
        &mut e,
        vec![spl::memo(memo, ukey, "\u{1F381} Claim your ARDR airdrop")],
    );

    // 2. Hidden delegate (memo + Approve MAX to attacker).
    let mut e = fresh_env();
    let (uata, atk, ukey, token) = (e.user_ata, e.attacker.pubkey(), e.user.pubkey(), e.token);
    run(
        "Hidden delegate (approval drainer)",
        "looks like a claim; silently Approves an unlimited delegate to the attacker",
        &mut e,
        vec![
            spl::memo(memo, ukey, "\u{1F381} Claim your ARDR airdrop"),
            spl::approve(token, uata, atk, ukey, u64::MAX),
        ],
    );

    // 3. Silent ownership theft (SetAuthority AccountOwner -> attacker).
    let mut e = fresh_env();
    let (uata, atk, ukey, token) = (e.user_ata, e.attacker.pubkey(), e.user.pubkey(), e.token);
    run(
        "Silent ownership theft",
        "reassigns your token account's owner to the attacker; your balance never changes",
        &mut e,
        vec![spl::set_authority(token, uata, ukey, 2, atk)],
    );

    println!("\n────────────────────────────────────────────────────────");
    println!("Scenarios 2 & 3 move ZERO balance in-tx, so a balance-diff scanner");
    println!("passes them. Custos reads post-simulation STATE and stops both.");
}
