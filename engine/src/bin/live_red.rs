//! Live RED demo — real mainnet account state × a prospective drainer.
//!
//! This is the product's true mode: a wallet is about to sign a tx. We clone a
//! REAL on-chain token account (a top USDC holder), then simulate a PROSPECTIVE
//! malicious tx (memo "claim" + hidden Approve of u64::MAX to an attacker) that
//! grants an unlimited delegate. Because the tx is prospective, current state
//! is the correct pre-state (no historical-replay confound). The Custos engine
//! reads the post-simulation delegate and returns RED; a balance-diff scanner
//! sees no tokens move and returns GREEN.
//!
//! Usage: cargo run --bin live_red [--rpc URL]

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use base64::Engine as _;
use custos_engine::{default_bank, evaluate, naive_balance_diff, short, sim, spl, Level, Verdict};
use litesvm::LiteSVM;
use solana_account::Account;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use serde_json::{json, Value};

const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const MEMO: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

fn art() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../gate/artifacts")
}
fn rpc(url: &str, method: &str, params: Value) -> Value {
    let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    for _ in 0..6 {
        if let Ok(resp) = ureq::post(url).send_json(body.clone()) {
            if let Ok(v) = resp.into_json::<Value>() {
                if !v["result"].is_null() {
                    return v["result"].clone();
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    panic!("rpc {method} failed");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rpc_url = args.iter().position(|a| a == "--rpc").and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("CUSTOS_RPC").ok())
        .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".into());

    let token = custos_engine::spl_token_id();
    let _ = USDC_MINT;

    // 1. a REAL on-chain USDC token account (override with --account <PUBKEY>).
    //    Default is a real ~330k-USDC account sourced from a live Jupiter tx.
    let ata_str = args.iter().position(|a| a == "--account").and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| "8N3vV8bPQJMWfM3kHnU79Q3Q9zfWjQtirPo11RN4ytnY".into());
    let ata = Pubkey::from_str(&ata_str).unwrap();
    eprintln!("• cloning real USDC account {} …", short(&ata));

    // 2. clone its CURRENT state.
    let info = rpc(&rpc_url, "getAccountInfo", json!([ata_str, {"encoding":"base64"}]));
    let val = &info["value"];
    let data = base64::engine::general_purpose::STANDARD.decode(val["data"][0].as_str().unwrap()).unwrap();
    let holder = Pubkey::try_from(&data[32..64]).unwrap(); // SPL owner field
    let balance = u64::from_le_bytes(data[64..72].try_into().unwrap());
    eprintln!("• cloned {} ({} USDC, owner {})", short(&ata), balance as f64 / 1e6, short(&holder));

    // 3. build the simulation env with the REAL account state.
    let mut svm = LiteSVM::new().with_sigverify(false).with_blockhash_check(false);
    svm.add_program(token, &std::fs::read(art().join("spl_token.so")).unwrap()).unwrap();
    svm.add_program(Pubkey::from_str(MEMO).unwrap(), &std::fs::read(art().join("memo.so")).unwrap()).unwrap();
    svm.set_account(ata, Account {
        lamports: val["lamports"].as_u64().unwrap(),
        data,
        owner: token,
        executable: false,
        rent_epoch: u64::MAX,
    }).unwrap();
    svm.airdrop(&holder, 1_000_000_000).unwrap(); // holder pays the fee in the sim

    let attacker = Keypair::new().pubkey();

    // 4. the PROSPECTIVE malicious tx (unsigned — sigverify is off, exactly as
    //    a pre-sign simulation would run it).
    let memo = Pubkey::from_str(MEMO).unwrap();
    let ixs = vec![
        spl::memo(memo, holder, "\u{1F381} Claim your ARDR airdrop"),
        spl::approve(token, ata, attacker, holder, u64::MAX),
    ];
    let msg = Message::new_with_blockhash(&ixs, Some(&holder), &svm.latest_blockhash());
    let tx = Transaction::new_unsigned(msg);

    // 5. simulate + evaluate.
    let outcome = sim::capture(&mut svm, tx, holder, &[ata], token, custos_engine::system_id());
    let (naive_level, naive_notes) = naive_balance_diff(&outcome);
    let Verdict { level, findings } = evaluate(&outcome, &default_bank());

    println!("\n════════════════════════════════════════════════════════");
    println!("CUSTOS LIVE RED  (real account state × prospective drainer)");
    println!("  real token account : {ata}");
    println!("  owner (wallet)     : {holder}");
    println!("  real USDC balance  : {}", balance as f64 / 1e6);
    println!("  prospective tx     : memo \"claim airdrop\" + hidden Approve(u64::MAX)");
    println!("  simulation         : {}", if outcome.success { "executed" } else { "reverted" });
    println!("  ─ balance-diff scanner : {}", render(naive_level));
    for n in &naive_notes {
        println!("      · {n}");
    }
    println!("  ─ Custos engine        : {}", render(level));
    for f in &findings {
        println!("      · [{}] {} — {}", f.code, short(&f.account), f.message);
    }
    let contrast = if level > naive_level {
        "  ⇒ Custos caught, on REAL account state, what balance-diff missed"
    } else {
        "  ⇒ (no gap)"
    };
    println!("{contrast}");
    println!("════════════════════════════════════════════════════════");
}

fn render(l: Level) -> &'static str {
    match l {
        Level::Green => "GREEN ✅",
        Level::Info => "INFO",
        Level::Yellow => "WARN ⚠",
        Level::Red => "RED ⛔",
    }
}
