//! Minimal "proxy" program for the Custos nested-CPI F2 case.
//!
//! At the top level a caller just invokes this program with 8 bytes of data.
//! An instruction parser that inspects only top-level instructions (and knows
//! nothing about this program) sees an opaque unknown-program call — no
//! `Approve` pattern to flag. Internally the program CPIs SPL Token `Approve`,
//! handing a delegate unlimited control. Custos catches it by reading the
//! account's post-simulation delegate state, not by parsing instructions.

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke,
    pubkey::Pubkey,
};

entrypoint!(process);

// data:     [amount: u64 LE]
// accounts: [0]=source token account (w), [1]=delegate, [2]=owner (signer),
//           [3]=SPL Token program
fn process(_program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let source = &accounts[0];
    let delegate = &accounts[1];
    let owner = &accounts[2];
    let token = &accounts[3];

    let amount = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let mut ix_data = vec![4u8]; // TokenInstruction::Approve
    ix_data.extend_from_slice(&amount.to_le_bytes());

    let ix = Instruction {
        program_id: *token.key,
        accounts: vec![
            AccountMeta::new(*source.key, false),
            AccountMeta::new_readonly(*delegate.key, false),
            AccountMeta::new_readonly(*owner.key, true),
        ],
        data: ix_data,
    };
    invoke(&ix, &[source.clone(), delegate.clone(), owner.clone(), token.clone()])
}
