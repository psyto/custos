//! Minimal SPL Token wire helpers (hand-rolled) used by the demo and scanner.

use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

/// Raw bytes of an Initialized SPL Token account (165 bytes).
pub fn token_account_bytes(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut d = vec![0u8; 165];
    d[0..32].copy_from_slice(mint.as_ref());
    d[32..64].copy_from_slice(owner.as_ref());
    d[64..72].copy_from_slice(&amount.to_le_bytes());
    d[108] = 1; // state = Initialized
    d
}

pub fn approve(token: Pubkey, source: Pubkey, delegate: Pubkey, owner: Pubkey, amount: u64) -> Instruction {
    let mut data = vec![4u8]; // Approve
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: token,
        accounts: vec![
            AccountMeta::new(source, false),
            AccountMeta::new_readonly(delegate, false),
            AccountMeta::new_readonly(owner, true),
        ],
        data,
    }
}

pub fn transfer(token: Pubkey, source: Pubkey, dest: Pubkey, authority: Pubkey, amount: u64) -> Instruction {
    let mut data = vec![3u8]; // Transfer
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: token,
        accounts: vec![
            AccountMeta::new(source, false),
            AccountMeta::new(dest, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data,
    }
}

/// SetAuthority. authority_type: 2 = AccountOwner, 3 = CloseAccount.
pub fn set_authority(token: Pubkey, account: Pubkey, current: Pubkey, authority_type: u8, new_authority: Pubkey) -> Instruction {
    let mut data = vec![6u8, authority_type, 1u8]; // tag, type, COption=Some
    data.extend_from_slice(new_authority.as_ref());
    Instruction {
        program_id: token,
        accounts: vec![
            AccountMeta::new(account, false),
            AccountMeta::new_readonly(current, true),
        ],
        data,
    }
}

pub fn memo(program: Pubkey, signer: Pubkey, text: &str) -> Instruction {
    Instruction {
        program_id: program,
        accounts: vec![AccountMeta::new_readonly(signer, true)],
        data: text.as_bytes().to_vec(),
    }
}
