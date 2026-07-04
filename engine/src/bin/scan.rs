//! `custos scan <SIGNATURE>` — the live path.
//!
//! Fetch a real mainnet transaction, clone every account it touches (ALT
//! included) + every program it invokes into LiteSVM, simulate it, and run the
//! Custos invariant bank over the outcome. This is the product loop against a
//! real tx (historical replay uses current-slot state; the product simulates a
//! prospective tx pre-sign — same machinery).
//!
//! Usage:
//!   cargo run --bin scan -- <SIGNATURE> [--rpc <URL>] [--user <PUBKEY>]
//!   CUSTOS_RPC=<url> cargo run --bin scan -- <SIGNATURE>

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;

use base64::Engine as _;
use custos_engine::{default_bank, evaluate, naive_balance_diff, short, sim, Level, Verdict};
use litesvm::LiteSVM;
use solana_account::Account;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;
use serde_json::{json, Value};

const BUILTINS: &[&str] = &[
    "11111111111111111111111111111111",
    "ComputeBudget111111111111111111111111111111",
    "BPFLoaderUpgradeab1e11111111111111111111111",
    "BPFLoader2111111111111111111111111111111111",
    "AddressLookupTab1e1111111111111111111111111",
    "Vote111111111111111111111111111111111111111",
    "Stake11111111111111111111111111111111111111",
];

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
    panic!("rpc {method} failed after retries");
}

fn cache_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/scan_cache");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sig = args.get(1).cloned().unwrap_or_else(|| {
        eprintln!("usage: scan <SIGNATURE> [--rpc URL] [--user PUBKEY]");
        std::process::exit(2);
    });
    let rpc_url = flag(&args, "--rpc").unwrap_or_else(|| {
        std::env::var("CUSTOS_RPC").unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into())
    });
    let user_override = flag(&args, "--user");

    eprintln!("• fetching tx {} …", &sig[..sig.len().min(16)]);
    let tx = rpc(&rpc_url, "getTransaction", json!([sig, {"maxSupportedTransactionVersion":0,"encoding":"json"}]));
    let msg = &tx["transaction"]["message"];
    let static_keys: Vec<String> = msg["accountKeys"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let loaded = &tx["meta"]["loadedAddresses"];
    let mut all: Vec<String> = static_keys.clone();
    for f in ["writable", "readonly"] {
        if let Some(a) = loaded[f].as_array() {
            all.extend(a.iter().map(|v| v.as_str().unwrap().to_string()));
        }
    }
    if let Some(alts) = msg["addressTableLookups"].as_array() {
        all.extend(alts.iter().map(|l| l["accountKey"].as_str().unwrap().to_string()));
    }
    let mut seen = BTreeSet::new();
    all.retain(|k| seen.insert(k.clone()));

    let user = Pubkey::from_str(&user_override.unwrap_or_else(|| static_keys[0].clone())).unwrap();
    let recorded_cu = tx["meta"]["computeUnitsConsumed"].as_u64();

    // wire tx (for exact replay)
    let b64 = rpc(&rpc_url, "getTransaction", json!([sig, {"maxSupportedTransactionVersion":0,"encoding":"base64"}]));
    let wire = base64::engine::general_purpose::STANDARD.decode(b64["transaction"][0].as_str().unwrap()).unwrap();
    let vtx: VersionedTransaction = bincode::deserialize(&wire).expect("deserialize tx");

    // fetch account states
    eprintln!("• cloning {} accounts …", all.len());
    let mut infos: Vec<(String, Value)> = vec![];
    for chunk in all.chunks(100) {
        let keys: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        let r = rpc(&rpc_url, "getMultipleAccounts", json!([keys, {"encoding":"base64"}]));
        for (k, info) in chunk.iter().zip(r["value"].as_array().unwrap()) {
            infos.push((k.clone(), info.clone()));
        }
    }

    let mut svm = LiteSVM::new().with_sigverify(false).with_blockhash_check(false);
    svm.warp_to_slot(500_000_000); // past ALT last_extended_slot

    let (mut cloned, mut progs, mut missing) = (0u32, 0u32, 0u32);
    let mut watch: Vec<Pubkey> = vec![user];
    for (k, info) in &infos {
        if BUILTINS.contains(&k.as_str()) {
            continue;
        }
        let pk = Pubkey::from_str(k).unwrap();
        if info.is_null() {
            missing += 1;
            continue;
        }
        if info["executable"].as_bool().unwrap_or(false) {
            match dump_program(&rpc_url, k) {
                Some(elf) => {
                    svm.add_program(pk, &elf).unwrap();
                    progs += 1;
                }
                None => eprintln!("  ! could not dump program {}", &k[..8]),
            }
        } else {
            let data = base64::engine::general_purpose::STANDARD.decode(info["data"][0].as_str().unwrap()).unwrap();
            svm.set_account(pk, Account {
                lamports: info["lamports"].as_u64().unwrap(),
                data,
                owner: Pubkey::from_str(info["owner"].as_str().unwrap()).unwrap(),
                executable: false,
                rent_epoch: u64::MAX,
            }).unwrap();
            watch.push(pk);
            cloned += 1;
        }
    }
    eprintln!("• assembled {cloned} state accounts + {progs} programs (missing: {missing})\n");

    // simulate + evaluate
    let outcome = sim::capture(&mut svm, vtx, user, &watch, custos_engine::spl_token_id(), custos_engine::system_id());
    let (naive_level, naive_notes) = naive_balance_diff(&outcome);
    let Verdict { level, findings } = evaluate(&outcome, &default_bank());

    println!("════════════════════════════════════════════════════════");
    println!("CUSTOS SCAN  {}", sig);
    println!("  protected wallet : {}", user);
    println!("  simulation       : {}", if outcome.success { "executed" } else { "reverted (program-level)" });
    if let Some(cu) = recorded_cu {
        println!("  mainnet CU (ref) : {cu}");
    }
    println!("  ─ balance-diff scanner : {}", render(naive_level));
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
    println!("\n  verdict_json: {}", json!({
        "signature": sig,
        "user": user.to_string(),
        "level": format!("{level:?}"),
        "findings": findings.iter().map(|f| json!({"code":f.code,"account":f.account.to_string(),"message":f.message,"level":format!("{:?}",f.level)})).collect::<Vec<_>>(),
    }));
    println!("════════════════════════════════════════════════════════");

    if !outcome.success {
        eprintln!("\nnote: historical replay against CURRENT state can revert on stale prices;");
        eprintln!("      the product simulates a PROSPECTIVE tx vs current state (no such confound).");
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn render(l: Level) -> &'static str {
    match l {
        Level::Green => "GREEN ✅",
        Level::Info => "INFO",
        Level::Yellow => "WARN ⚠",
        Level::Red => "RED ⛔",
    }
}

/// Dump a program's ELF via the solana CLI (cached). Production replaces this
/// with in-process loader extraction + a persistent program cache.
fn dump_program(rpc_url: &str, pk: &str) -> Option<Vec<u8>> {
    let path = cache_dir().join(format!("{pk}.so"));
    if !path.exists() {
        let ok = Command::new("solana")
            .args(["program", "dump", pk, path.to_str().unwrap(), "--url", rpc_url])
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok || !path.exists() {
            return None;
        }
    }
    std::fs::read(&path).ok().filter(|b| !b.is_empty())
}
