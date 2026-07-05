//! Latency profile — where does a pre-sign scan spend its time, and how much
//! does the in-process program cache save?
//!
//! Scans the SAME transaction twice: cold (programs dumped) then warm (programs
//! served from RAM). Accounts are fetched live both times (they must be current).
//! This quantifies the "programs are static/cacheable; only account state is
//! live" thesis.
//!
//! Usage: cargo run --bin profile -- [SIGNATURE] [--rpc URL]

use custos_engine::loader::{self, ScanReport};

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn row(label: &str, r: &ScanReport) {
    let t = &r.timing;
    println!(
        "  {label:<6} total {:>5}ms  =  resolve {:>4} + accounts {:>4} + programs {:>5} + sim {:>4}   [prog cache {}h/{}m]",
        t.total_ms, t.resolve_ms, t.fetch_accounts_ms, t.programs_ms, t.sim_ms, t.program_cache_hits, t.program_cache_misses
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // default: a Jupiter swap touching 4 programs incl Jupiter 2.9MB + CLMM 1.7MB
    let sig = args.get(1).filter(|s| !s.starts_with("--")).cloned().unwrap_or_else(||
        "3aRTnj6iE1YJqmuM6V7NL6PinwSaE6hJ4M2BPTSUsPS68BiUEzvEzib5kNyqM5QwYJGehYyrYXTGJMM2G7UfkXNN".into());
    let rpc = flag(&args, "--rpc").unwrap_or_else(loader::default_rpc);

    eprintln!("• profiling scan of {} …\n", &sig[..16]);
    let wire = loader::fetch_wire_b64(&sig, &rpc);

    println!("═══ Custos scan latency profile ═══");
    let cold = loader::scan_b64(&wire, None, &rpc);
    row("COLD", &cold);
    let warm = loader::scan_b64(&wire, None, &rpc);
    row("WARM", &warm);

    let prog_saved = cold.timing.programs_ms.saturating_sub(warm.timing.programs_ms);
    println!("\n  program acquisition: {}ms cold → {}ms warm  (saved {}ms via in-RAM cache)",
        cold.timing.programs_ms, warm.timing.programs_ms, prog_saved);
    println!("  live-bound floor (resolve+accounts, needs a fast RPC): {}ms",
        warm.timing.resolve_ms + warm.timing.fetch_accounts_ms);
    println!("  local work (programs+sim), warm: {}ms",
        warm.timing.programs_ms + warm.timing.sim_ms);
    println!("\n  note: public RPC rate-limits; the account-fetch term is RPC-bound and");
    println!("        drops sharply on a dedicated endpoint (Helius/Triton). Programs are");
    println!("        static — a pre-warmed cache makes nearly every real scan 'warm'.");
}
