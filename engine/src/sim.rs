//! Simulation layer: snapshot the watched accounts, execute the tx in LiteSVM,
//! snapshot again, and package an [`Outcome`] for the invariant bank.

use std::collections::BTreeMap;

use litesvm::LiteSVM;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

use crate::{AccountSnapshot, Outcome};

fn snapshot(svm: &LiteSVM, keys: &[Pubkey]) -> BTreeMap<Pubkey, Option<AccountSnapshot>> {
    keys.iter()
        .map(|k| {
            let s = svm.get_account(k).map(|a| AccountSnapshot {
                lamports: a.lamports,
                owner: a.owner,
                data: a.data,
            });
            (*k, s)
        })
        .collect()
}

/// Execute `tx`, capturing pre/post state of `watch` and the program logs.
/// `user` is the protected wallet (fee payer). This is the product's core call:
/// simulate a prospective tx, observe what happened to the user's accounts.
pub fn capture(
    svm: &mut LiteSVM,
    tx: impl Into<VersionedTransaction>,
    user: Pubkey,
    watch: &[Pubkey],
    token_id: Pubkey,
    system_id: Pubkey,
) -> Outcome {
    let pre = snapshot(svm, watch);
    let vtx: VersionedTransaction = tx.into();
    let (logs, success) = match svm.send_transaction(vtx) {
        Ok(meta) => (meta.logs, true),
        Err(e) => (e.meta.logs, false),
    };
    let post = snapshot(svm, watch);
    Outcome { user, pre, post, logs, success, token_id, system_id }
}
