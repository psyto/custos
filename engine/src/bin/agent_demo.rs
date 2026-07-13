use std::collections::HashSet;

use custos_engine::scenarios::{builtin, payment_with_hidden_delegate, DEMO_MERCHANT};

struct Policy<'a> {
    max_usdc: u64,
    allowlist: HashSet<&'a str>,
}

fn authorize(amount_usdc: u64, destination: &str, policy: &Policy<'_>) -> bool {
    amount_usdc <= policy.max_usdc && policy.allowlist.contains(destination)
}

fn render(level: &str) -> &str {
    match level {
        "GREEN" => "GREEN ✅",
        "YELLOW" => "WARN ⚠",
        "RED" => "RED ⛔",
        other => other,
    }
}

fn print_findings(findings: &[custos_engine::scenarios::FindingDto]) {
    if findings.is_empty() {
        println!("  no Custos findings");
    }
    for finding in findings {
        println!(
            "  [{}] {} — {}",
            finding.code, finding.account, finding.message
        );
    }
}

fn main() {
    println!("=== Autonomous solver pre-broadcast gate ===");
    println!("Actor: an autonomous agent managing a solver's capital, no human in the loop.");
    println!("Mandate: never grant a delegate or lose principal unexpectedly.");

    let policy = Policy {
        max_usdc: 100,
        allowlist: HashSet::from([DEMO_MERCHANT]),
    };
    let amount_usdc = 5;
    let destination = DEMO_MERCHANT;
    let authorized = authorize(amount_usdc, destination, &policy);
    let hero = payment_with_hidden_delegate();

    println!("\nHERO: {}", hero.title);
    println!("  {}", hero.subtitle);
    println!("Declared intent: {{ amount_usdc: {amount_usdc}, destination: {destination} }}");
    println!(
        "Policy: {{ max_usdc: {}, allowlist: {{{}}} }}",
        policy.max_usdc, DEMO_MERCHANT
    );
    println!(
        "Authorization policy (declared intent): {}",
        if authorized { "PASS" } else { "FAIL" }
    );
    println!(
        "Custos verification (actual tx): {}",
        render(&hero.custos_level)
    );
    print_findings(&hero.custos_findings);
    println!("Note: the 5 USDC payment matches the declared intent; the RED is driven by the hidden delegate.");
    println!(
        "Decision: {}",
        if hero.custos_level != "GREEN" {
            "REFUSE TO BROADCAST — principal capital preserved"
        } else {
            "BROADCAST"
        }
    );

    println!("\nCustos also blocks (verification only):");
    for scenario in builtin() {
        println!("\n{}: {}", scenario.title, render(&scenario.custos_level));
        print_findings(&scenario.custos_findings);
    }
}
