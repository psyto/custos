//! `custos scan <SIGNATURE>` — the live path (thin CLI over `loader`).
//!
//! Fetch a real mainnet transaction, clone every account it touches (ALT
//! included) + every program it invokes into LiteSVM, simulate it, and run the
//! Custos invariant bank.
//!
//! Usage:
//!   cargo run --bin scan -- <SIGNATURE> [--rpc <URL>] [--user <PUBKEY>]
//!   CUSTOS_RPC=<url> cargo run --bin scan -- <SIGNATURE>

use custos_engine::loader;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn render(l: &str) -> &str {
    match l {
        "GREEN" => "GREEN ✅",
        "YELLOW" => "WARN ⚠",
        "RED" => "RED ⛔",
        _ => l,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(sig) = args.get(1).cloned() else {
        eprintln!("usage: scan <SIGNATURE> [--rpc URL] [--user PUBKEY]");
        std::process::exit(2);
    };
    let rpc = flag(&args, "--rpc").unwrap_or_else(loader::default_rpc);
    let user = flag(&args, "--user");

    eprintln!("• fetching + simulating {} …", &sig[..sig.len().min(16)]);
    let wire = loader::fetch_wire_b64(&sig, &rpc);
    let r = loader::scan_b64(&wire, user.as_deref(), &rpc);

    println!("════════════════════════════════════════════════════════");
    println!("CUSTOS SCAN  {sig}");
    println!("  protected wallet : {}", r.user);
    println!("  simulation       : {}", if r.simulated { "executed" } else { "reverted (program-level)" });
    println!("  assembled        : {} accounts + {} programs (missing: {})", r.cloned_accounts, r.programs, r.missing);
    println!("  ─ balance-diff scanner : {}", render(&r.naive_level));
    for n in &r.naive_notes {
        println!("      · {n}");
    }
    println!("  ─ Custos engine        : {}", render(&r.level));
    if r.findings.is_empty() {
        println!("      · no user-protective invariant fired");
    }
    for f in &r.findings {
        println!("      · [{}] {} — {}", f.code, f.account, f.message);
    }
    println!("════════════════════════════════════════════════════════");
    if !r.simulated {
        eprintln!("\nnote: historical replay against CURRENT state can revert on stale prices;");
        eprintln!("      the product simulates a PROSPECTIVE tx vs current state (no such confound).");
    }
}
