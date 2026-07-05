//! Live loader: turn an unsigned transaction into a verdict.
//!
//! Given a base64 wire transaction (what a dApp is about to ask a wallet to
//! sign), fetch every account it touches from mainnet at the CURRENT slot,
//! clone them + the invoked programs into LiteSVM, simulate the tx
//! prospectively (sigverify off), and run the invariant bank. This is the
//! product's pre-sign check; the `scan` bin and the HTTP API both call it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use litesvm::LiteSVM;
use serde::Serialize;
use serde_json::{json, Value};
use solana_account::Account;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

use crate::scenarios::{level_str, FindingDto};
use crate::{default_bank, evaluate, naive_balance_diff, short, sim, Verdict};

const BUILTINS: &[&str] = &[
    "11111111111111111111111111111111",
    "ComputeBudget111111111111111111111111111111",
    "BPFLoaderUpgradeab1e11111111111111111111111",
    "BPFLoader2111111111111111111111111111111111",
    "AddressLookupTab1e1111111111111111111111111",
    "Vote111111111111111111111111111111111111111",
    "Stake11111111111111111111111111111111111111",
];

#[derive(Serialize)]
pub struct ScanReport {
    pub user: String,
    pub simulated: bool,
    pub level: String,
    pub findings: Vec<FindingDto>,
    pub naive_level: String,
    pub naive_notes: Vec<String>,
    pub caught_gap: bool,
    pub cloned_accounts: u32,
    pub programs: u32,
    pub missing: u32,
    pub timing: Timing,
}

#[derive(Serialize, Default)]
pub struct Timing {
    pub resolve_ms: u128,
    pub fetch_accounts_ms: u128,
    pub programs_ms: u128,
    pub sim_ms: u128,
    pub total_ms: u128,
    pub program_cache_hits: u32,
    pub program_cache_misses: u32,
}

/// In-process program-ELF cache. Programs are ~static, so once seen their bytes
/// stay in RAM and later scans skip the (expensive) dump entirely. A production
/// service pre-warms this with the top ~50 DeFi programs so nearly every scan
/// is a cache hit on programs; only account state must be fetched live.
static PROGRAM_CACHE: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();

fn program_cache() -> &'static Mutex<HashMap<String, Arc<Vec<u8>>>> {
    PROGRAM_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn default_rpc() -> String {
    std::env::var("CUSTOS_RPC").unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into())
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

fn cache_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/scan_cache");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Get a program's ELF, returning `(bytes, cache_hit)`. In-memory cache first,
/// then on-disk cache, then a live `solana program dump`.
fn get_program(rpc_url: &str, pk: &str) -> Option<(Arc<Vec<u8>>, bool)> {
    if let Some(a) = program_cache().lock().unwrap().get(pk) {
        return Some((a.clone(), true));
    }
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
    let bytes = std::fs::read(&path).ok().filter(|b| !b.is_empty())?;
    let arc = Arc::new(bytes);
    program_cache().lock().unwrap().insert(pk.to_string(), arc.clone());
    Some((arc, false))
}

/// Resolve the full account set a versioned tx touches, resolving ALTs by
/// fetching the lookup-table accounts (prospective txs carry no loadedAddresses).
fn resolve_keys(rpc_url: &str, vtx: &VersionedTransaction) -> Vec<String> {
    let msg = &vtx.message;
    let mut all: Vec<String> = msg.static_account_keys().iter().map(|k| k.to_string()).collect();
    if let Some(lookups) = msg.address_table_lookups() {
        for l in lookups {
            let table = l.account_key.to_string();
            let info = rpc(rpc_url, "getAccountInfo", json!([table, {"encoding":"base64"}]));
            if let Some(d) = info["value"]["data"][0].as_str() {
                let raw = base64::engine::general_purpose::STANDARD.decode(d).unwrap_or_default();
                let addrs: Vec<Pubkey> = raw.get(56..).unwrap_or(&[])
                    .chunks(32).filter(|c| c.len() == 32)
                    .map(|c| Pubkey::try_from(c).unwrap()).collect();
                for &i in l.writable_indexes.iter().chain(l.readonly_indexes.iter()) {
                    if let Some(a) = addrs.get(i as usize) {
                        all.push(a.to_string());
                    }
                }
            }
            all.push(table); // clone the table so LiteSVM can resolve too
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    all.retain(|k| seen.insert(k.clone()));
    all
}

/// Fetch a confirmed transaction's base64 wire form by signature.
pub fn fetch_wire_b64(sig: &str, rpc_url: &str) -> String {
    let b = rpc(rpc_url, "getTransaction", json!([sig, {"maxSupportedTransactionVersion":0,"encoding":"base64"}]));
    b["transaction"][0].as_str().expect("tx not found").to_string()
}

/// Scan an unsigned base64 wire transaction and return a verdict.
pub fn scan_b64(tx_b64: &str, user_override: Option<&str>, rpc_url: &str) -> ScanReport {
    let t_total = Instant::now();
    let wire = base64::engine::general_purpose::STANDARD.decode(tx_b64.trim()).expect("base64 tx");
    let vtx: VersionedTransaction = bincode::deserialize(&wire).expect("deserialize tx");
    let user = Pubkey::from_str(user_override.unwrap_or(&vtx.message.static_account_keys()[0].to_string())).unwrap();

    let t = Instant::now();
    let keys = resolve_keys(rpc_url, &vtx);
    let resolve_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut infos: Vec<(String, Value)> = vec![];
    for chunk in keys.chunks(100) {
        let ks: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        let r = rpc(rpc_url, "getMultipleAccounts", json!([ks, {"encoding":"base64"}]));
        for (k, info) in chunk.iter().zip(r["value"].as_array().unwrap()) {
            infos.push((k.clone(), info.clone()));
        }
    }
    let fetch_accounts_ms = t.elapsed().as_millis();

    let mut svm = LiteSVM::new().with_sigverify(false).with_blockhash_check(false);
    svm.warp_to_slot(500_000_000);

    let (mut cloned, mut programs, mut missing) = (0u32, 0u32, 0u32);
    let (mut hits, mut misses, mut programs_ms) = (0u32, 0u32, 0u128);
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
            let t = Instant::now();
            if let Some((elf, hit)) = get_program(rpc_url, k) {
                svm.add_program(pk, &elf).unwrap();
                programs += 1;
                if hit { hits += 1 } else { misses += 1 }
            }
            programs_ms += t.elapsed().as_millis();
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
    // fund the fee payer so the prospective tx can be simulated
    let _ = svm.airdrop(&user, 1_000_000_000);

    let t = Instant::now();
    let outcome = sim::capture(&mut svm, vtx, user, &watch, crate::spl_token_id(), crate::system_id());
    let (naive_level, naive_notes) = naive_balance_diff(&outcome);
    let Verdict { level, findings } = evaluate(&outcome, &default_bank());
    let sim_ms = t.elapsed().as_millis();

    ScanReport {
        user: user.to_string(),
        simulated: outcome.success,
        level: level_str(level).into(),
        findings: findings.iter().map(|f| FindingDto {
            code: f.code.into(),
            account: short(&f.account),
            message: f.message.clone(),
            level: level_str(f.level).into(),
        }).collect(),
        naive_level: level_str(naive_level).into(),
        naive_notes,
        caught_gap: level > naive_level,
        cloned_accounts: cloned,
        programs,
        missing,
        timing: Timing {
            resolve_ms,
            fetch_accounts_ms,
            programs_ms,
            sim_ms,
            total_ms: t_total.elapsed().as_millis(),
            program_cache_hits: hits,
            program_cache_misses: misses,
        },
    }
}

/// Build an unsigned "hidden delegate" tx (memo + Approve u64::MAX) targeting a
/// real token account, as a dApp would hand it to a wallet. base64 wire form.
pub fn build_hidden_approve_b64(source_ata: &str, owner: &str) -> String {
    use crate::spl;
    use solana_message::Message;
    use solana_transaction::Transaction;
    let token = crate::spl_token_id();
    let memo = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr").unwrap();
    let source = Pubkey::from_str(source_ata).unwrap();
    let owner = Pubkey::from_str(owner).unwrap();
    let attacker = solana_keypair::Keypair::new();
    use solana_signer::Signer;
    let ixs = vec![
        spl::memo(memo, owner, "\u{1F381} Claim your ARDR airdrop"),
        spl::approve(token, source, attacker.pubkey(), owner, u64::MAX),
    ];
    let msg = Message::new(&ixs, Some(&owner));
    let vtx: VersionedTransaction = Transaction::new_unsigned(msg).into();
    base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&vtx).unwrap())
}
