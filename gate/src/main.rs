//! Custos technical gate.
//!
//! Load-bearing question for the Execution Firewall product:
//!   Can LiteSVM (1) load an ARBITRARY mainnet BPF program by its raw .so,
//!   (2) clone a REAL mainnet account's state, and (3) execute a transaction
//!   against them while capturing pre/post state + logs + CU?
//!
//! If GREEN, the firewall's "simulate a prospective tx against real state
//! before signing" premise holds. If RED, the whole A-plan is blocked.

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

const MEMO_PROGRAM: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

fn artifacts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts")
}

fn main() {
    println!("=== Custos technical gate ===\n");
    let mut svm = LiteSVM::new();

    // ---------------------------------------------------------------
    // GATE A: clone a REAL mainnet account (USDC mint) into LiteSVM.
    // ---------------------------------------------------------------
    let usdc = Pubkey::from_str(USDC_MINT).unwrap();
    let raw = std::fs::read_to_string(artifacts().join("usdc_mint.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let acct = &v["account"];
    let data_b64 = acct["data"][0].as_str().unwrap();
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .unwrap();
    let owner = Pubkey::from_str(acct["owner"].as_str().unwrap()).unwrap();
    let lamports = acct["lamports"].as_u64().unwrap();

    svm.set_account(
        usdc,
        Account {
            lamports,
            data: data.clone(),
            owner,
            executable: acct["executable"].as_bool().unwrap(),
            rent_epoch: u64::MAX,
        },
    )
    .expect("set_account (clone real mainnet account)");

    let readback = svm.get_account(&usdc).expect("cloned account readable");
    let a_ok = readback.data == data && readback.owner == owner && readback.lamports == lamports;
    println!("GATE A  clone real mainnet account (USDC mint)");
    println!("  owner   : {}", readback.owner);
    println!("  data len: {} bytes (round-trips: {})", readback.data.len(), a_ok);
    println!("  verdict : {}\n", if a_ok { "GREEN" } else { "RED" });

    // ---------------------------------------------------------------
    // GATE B: load an ARBITRARY mainnet BPF program (.so dumped from
    //         mainnet) and actually execute it, capturing logs + CU.
    // ---------------------------------------------------------------
    let memo = Pubkey::from_str(MEMO_PROGRAM).unwrap();
    let elf = std::fs::read(artifacts().join("memo.so")).unwrap();
    svm.add_program(memo, &elf)
        .expect("add_program (arbitrary mainnet BPF)");

    // "User" wallet — in the product this is the connected wallet whose
    // prospective tx we simulate before they sign. Here we control it.
    let user = Keypair::new();
    svm.airdrop(&user.pubkey(), 1_000_000_000).expect("airdrop");

    let pre = svm.get_account(&user.pubkey()).unwrap().lamports;

    // Invoke the real Memo program with the user as the sole memo-signer.
    let ix = Instruction {
        program_id: memo,
        accounts: vec![AccountMeta::new_readonly(user.pubkey(), true)],
        data: b"custos-gate: simulated before signing".to_vec(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user],
        svm.latest_blockhash(),
    );

    let meta = svm.send_transaction(tx).expect("execute arbitrary mainnet BPF");
    let post = svm.get_account(&user.pubkey()).unwrap().lamports;

    let executed = meta
        .logs
        .iter()
        .any(|l| l.contains(MEMO_PROGRAM) && l.contains("success"));

    println!("GATE B  load + execute arbitrary mainnet BPF (Memo program)");
    println!("  .so size    : {} bytes", elf.len());
    println!("  CU consumed : {}", meta.compute_units_consumed);
    println!("  pre/post SOL: {} -> {} (delta {} lamports = fee)", pre, post, pre - post);
    println!("  logs:");
    for l in &meta.logs {
        println!("    | {l}");
    }
    println!("  program executed: {}", executed);
    println!("  verdict : {}\n", if executed { "GREEN" } else { "RED" });

    // ---------------------------------------------------------------
    println!("=== GATE RESULT: {} ===", if a_ok && executed { "GREEN — firewall premise holds" } else { "RED — blocked" });
}
