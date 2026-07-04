//! Custos Gate D — replay a REAL multi-CPI mainnet transaction (Jupiter swap)
//! against cloned mainnet state in LiteSVM.
//!
//! The load-bearing question the external critique raised: can a local VM
//! assemble all touched programs + accounts of a real multi-program CPI tx
//! and actually EXECUTE it (reach program logic + capture logs/CU), or does
//! it collapse into "account not found / can't load program"?
//!
//! We replay with sigverify + blockhash-check OFF (we cannot re-sign someone
//! else's tx; in the product the tx is unsigned pre-flight anyway). State is
//! cloned at CURRENT slot (public RPC has no archival), so an EXACT historical
//! match is not expected — the gate is about EXECUTION REACH, not fidelity.

use std::path::PathBuf;
use std::str::FromStr;

use base64::Engine;
use litesvm::LiteSVM;
use solana_account::Account;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

fn art() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts")
}
fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}

fn main() {
    println!("=== Custos Gate D: real Jupiter multi-CPI tx replay ===\n");
    let m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(art().join("gd_manifest.json")).unwrap()).unwrap();

    let mut svm = LiteSVM::new()
        .with_sigverify(false)
        .with_blockhash_check(false);
    // Warp past every mainnet ALT's last_extended_slot so all looked-up
    // addresses count as "active" during resolution (else the recently
    // extended entries are seen as not-yet-active -> InvalidAddressLookupTableIndex).
    svm.warp_to_slot(500_000_000);

    // ---- clone account state ----
    let (mut cloned, mut missing) = (0u32, 0u32);
    for a in m["accounts"].as_array().unwrap() {
        if a["missing"].as_bool().unwrap_or(false) {
            missing += 1;
            continue;
        }
        let pk = Pubkey::from_str(a["pk"].as_str().unwrap()).unwrap();
        let f: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(art().join(a["file"].as_str().unwrap())).unwrap()).unwrap();
        svm.set_account(
            pk,
            Account {
                lamports: f["lamports"].as_u64().unwrap(),
                data: b64(f["data_b64"].as_str().unwrap()),
                owner: Pubkey::from_str(f["owner"].as_str().unwrap()).unwrap(),
                executable: false,
                rent_epoch: u64::MAX,
            },
        )
        .unwrap();
        cloned += 1;
    }

    // ---- load programs (arbitrary mainnet BPF, incl. Jupiter 2.9MB) ----
    let mut progs = 0u32;
    for p in m["programs"].as_array().unwrap() {
        if !p["dumped"].as_bool().unwrap_or(false) {
            continue;
        }
        let pk = Pubkey::from_str(p["pk"].as_str().unwrap()).unwrap();
        let elf = std::fs::read(art().join(p["so"].as_str().unwrap())).unwrap();
        svm.add_program(pk, &elf).unwrap();
        progs += 1;
    }

    println!("assembled: {cloned} state accounts + {progs} programs (missing at current slot: {missing})");

    // ---- deserialize the real wire tx and replay ----
    let raw = b64(std::fs::read_to_string(art().join("gd_tx.b64")).unwrap().trim());
    let tx: VersionedTransaction = bincode::deserialize(&raw).expect("deserialize VersionedTransaction");
    // blockhash-check is off, so the recorded (now-expired) blockhash is fine.

    let top_program = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
    println!("mainnet-recorded CU: {}\n", m["cu"]);

    match svm.send_transaction(tx) {
        Ok(meta) => {
            println!("REPLAY: Ok  CU={}", meta.compute_units_consumed);
            print_logs(&meta.logs, top_program);
            verdict(&meta.logs, top_program, true);
        }
        Err(e) => {
            // A failing tx is NOT a gate failure — the question is whether we
            // reached program execution vs failed to assemble the environment.
            let logs = &e.meta.logs;
            println!("REPLAY: program-level failure (expected vs stale state): {:?}", e.err);
            println!("        CU consumed before failure: {}", e.meta.compute_units_consumed);
            print_logs(logs, top_program);
            verdict(logs, top_program, false);
        }
    }
}

fn print_logs(logs: &[String], top: &str) {
    println!("  --- logs ({} lines) ---", logs.len());
    for l in logs {
        println!("  | {l}");
    }
    let _ = top;
}

fn verdict(logs: &[String], top: &str, _ok: bool) {
    let reached_top = logs.iter().any(|l| l.contains(top) && l.contains("invoke"));
    let any_cpi = logs.iter().filter(|l| l.contains("invoke")).count();
    let env_fail = logs
        .iter()
        .any(|l| l.contains("account not found") || l.contains("insufficient account keys") || l.contains("ProgramAccountNotFound"));
    println!("\n  reached Jupiter program: {reached_top}");
    println!("  distinct CPI invoke lines: {any_cpi}");
    println!("  environment-assembly failure: {env_fail}");
    let green = reached_top && !env_fail;
    println!(
        "\n=== GATE D: {} ===",
        if green {
            "GREEN — real multi-CPI tx assembled + executed (reached program logic)"
        } else {
            "RED — environment could not be assembled/executed"
        }
    );
}
