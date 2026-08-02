//! Shows the same simulated payment evaluated under the default bank and an
//! authored mandate: a normal GREEN payment becomes RED only when the cap is
//! supplied to `bank_with_mandate`.

use std::collections::BTreeMap;

use custos_engine::{
    bank_with_mandate, default_bank, evaluate, spl_token_id, system_id, AccountSnapshot, Outcome,
};
use reexec_spec::MandateSpec;
use solana_pubkey::Pubkey;

fn token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> AccountSnapshot {
    let mut data = vec![0; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1;
    AccountSnapshot {
        lamports: 2_039_280,
        owner: spl_token_id(),
        data,
    }
}

fn main() {
    let (user, mint, account) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let payment = Outcome {
        user,
        pre: BTreeMap::from([(account, Some(token_account(mint, user, 1_000)))]),
        post: BTreeMap::from([(account, Some(token_account(mint, user, 200)))]),
        logs: vec![],
        success: true,
        token_id: spl_token_id(),
        system_id: system_id(),
    };
    let mandate = MandateSpec {
        max_size: 100,
        instrument: 0,
        max_value_out: 500,
    };

    println!("same payment: 800 tokens leave the user's account");
    println!(
        "default bank: {:?}",
        evaluate(&payment, &default_bank()).level
    );
    let verdict = evaluate(&payment, &bank_with_mandate(mandate));
    println!("authored max_value_out=500: {:?}", verdict.level);
    for finding in verdict.findings {
        println!("[{}] {}", finding.code, finding.message);
    }
}
