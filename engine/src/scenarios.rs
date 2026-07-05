//! Built-in demo scenarios, reusable by the `demo` CLI and the HTTP API.
//! Each returns a [`ScenarioReport`]: the Custos verdict beside a
//! balance-diff-only scanner's verdict, over the same simulated transaction.

use std::path::PathBuf;
use std::str::FromStr;

use base64::Engine as _;
use litesvm::LiteSVM;
use serde::Serialize;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::{default_bank, evaluate, naive_balance_diff, short, spl, sim, Level, Verdict};

const MEMO: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

#[derive(Serialize)]
pub struct FindingDto {
    pub code: String,
    pub account: String,
    pub message: String,
    pub level: String,
}

#[derive(Serialize)]
pub struct ScenarioReport {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub custos_level: String,
    pub custos_findings: Vec<FindingDto>,
    pub naive_level: String,
    pub naive_notes: Vec<String>,
    pub caught_gap: bool,
}

fn art() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../gate/artifacts")
}

pub fn level_str(l: Level) -> &'static str {
    match l {
        Level::Green => "GREEN",
        Level::Info => "INFO",
        Level::Yellow => "YELLOW",
        Level::Red => "RED",
    }
}

struct Env {
    svm: LiteSVM,
    user: Keypair,
    attacker: Keypair,
    user_ata: Pubkey,
    token: Pubkey,
}

fn fresh_env() -> Env {
    let token = crate::spl_token_id();
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

    Env { svm, user, attacker, user_ata, token }
}

fn build(id: &str, title: &str, subtitle: &str, e: &mut Env, ixs: Vec<solana_instruction::Instruction>) -> ScenarioReport {
    let watch = vec![e.user_ata, e.user.pubkey()];
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&e.user.pubkey()), &[&e.user], e.svm.latest_blockhash());
    let outcome = sim::capture(&mut e.svm, tx, e.user.pubkey(), &watch, e.token, crate::system_id());

    let (naive_level, naive_notes) = naive_balance_diff(&outcome);
    let Verdict { level, findings } = evaluate(&outcome, &default_bank());

    ScenarioReport {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        custos_level: level_str(level).into(),
        custos_findings: findings.iter().map(|f| FindingDto {
            code: f.code.into(),
            account: short(&f.account),
            message: f.message.clone(),
            level: level_str(f.level).into(),
        }).collect(),
        naive_level: level_str(naive_level).into(),
        naive_notes,
        caught_gap: level > naive_level,
    }
}

/// The three canonical scenarios, evaluated fresh.
pub fn builtin() -> Vec<ScenarioReport> {
    let memo = Pubkey::from_str(MEMO).unwrap();
    let mut out = vec![];

    let mut e = fresh_env();
    let ukey = e.user.pubkey();
    out.push(build("benign", "Benign airdrop claim", "A memo; no funds move, no authority changes.", &mut e,
        vec![spl::memo(memo, ukey, "\u{1F381} Claim your ARDR airdrop")]));

    let mut e = fresh_env();
    let (uata, atk, ukey, token) = (e.user_ata, e.attacker.pubkey(), e.user.pubkey(), e.token);
    out.push(build("hidden-delegate", "Hidden delegate (approval drainer)",
        "Looks like a claim; silently approves an unlimited delegate to the attacker.", &mut e,
        vec![spl::memo(memo, ukey, "\u{1F381} Claim your ARDR airdrop"), spl::approve(token, uata, atk, ukey, u64::MAX)]));

    let mut e = fresh_env();
    let (uata, atk, ukey, token) = (e.user_ata, e.attacker.pubkey(), e.user.pubkey(), e.token);
    out.push(build("ownership-theft", "Silent ownership theft",
        "Reassigns your token account's owner to the attacker; your balance never changes.", &mut e,
        vec![spl::set_authority(token, uata, ukey, 2, atk)]));

    // 4. Routed through an unknown program that CPIs Approve internally — an
    //    instruction parser sees only an opaque unknown-program call.
    let mut e = fresh_env();
    let (uata, atk, ukey, token) = (e.user_ata, e.attacker.pubkey(), e.user.pubkey(), e.token);
    let proxy = Keypair::new().pubkey();
    if let Ok(elf) = std::fs::read(art().join("proxy_approve.so")) {
        e.svm.add_program(proxy, &elf).unwrap();
        let proxy_ix = Instruction {
            program_id: proxy,
            accounts: vec![
                AccountMeta::new(uata, false),
                AccountMeta::new_readonly(atk, false),
                AccountMeta::new_readonly(ukey, true),
                AccountMeta::new_readonly(token, false),
            ],
            data: u64::MAX.to_le_bytes().to_vec(),
        };
        out.push(build("unknown-program", "Routed through an unknown program",
            "The only visible instruction is an opaque call to an unknown program; it approves an unlimited delegate inside its own CPI.",
            &mut e, vec![spl::memo(memo, ukey, "\u{1F381} Claim your ARDR airdrop"), proxy_ix]));
    }

    out
}
