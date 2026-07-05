//! Custos Stage 1 demo — three prospective transactions judged by the SAME
//! engine, next to a balance-diff-only scanner. Thin printer over
//! `custos_engine::scenarios::builtin()` (shared with the HTTP API).

use custos_engine::scenarios::builtin;

fn render(l: &str) -> String {
    match l {
        "GREEN" => "GREEN ✅".into(),
        "YELLOW" => "WARN ⚠".into(),
        "RED" => "RED ⛔".into(),
        other => other.into(),
    }
}

fn main() {
    println!("=== Custos Stage 1 engine demo: 3 prospective txs, one engine ===");
    for s in builtin() {
        println!("\n────────────────────────────────────────────────────────");
        println!("SCENARIO: {}", s.title);
        println!("  {}", s.subtitle);
        println!("  ─ Balance-diff scanner : {}", render(&s.naive_level));
        for n in &s.naive_notes {
            println!("      · {n}");
        }
        println!("  ─ Custos engine        : {}", render(&s.custos_level));
        if s.custos_findings.is_empty() {
            println!("      · no user-protective invariant fired");
        }
        for f in &s.custos_findings {
            println!("      · [{}] {} — {}", f.code, f.account, f.message);
        }
        println!("{}", if s.caught_gap { "  ⇒ Custos caught what balance-diff missed" } else { "  ⇒ agree" });
    }
    println!("\n────────────────────────────────────────────────────────");
    println!("The drainer scenarios move ZERO balance in-tx, so a balance-diff scanner");
    println!("passes them. Custos reads post-simulation STATE (F1-F5) and stops them —");
    println!("including one hidden inside an unknown program's CPI.");
}
