# Review 001 (Custos) — `MandateConformance` (CC reviews Codex)

**Branch:** `task/001-mandate-conformance` (`c05a789`) · **Reviewer:** CC · **Verdict: APPROVE** (no P0/P1/P2).

Closes the cross-station fold: Custos (screen) now enforces the **same** `reexec-spec::MandateSpec` that
Probatio (certify) checks. Independently verified in-tree.

## Correctness audit

- **M1 semantics right.** `check` sums, **per mint**, `pre.amount.saturating_sub(post_amount)` over
  accounts where `pre_token.owner == o.user`; a closed account (`post_token` None) contributes its full
  `pre.amount`; fires one RED `M1-mandate` per mint whose aggregate outflow `> mandate.max_value_out`.
  Saturating arithmetic throughout — no overflow panic.
- **Boundary vs the malice tier is correct.** A delegate/authority change moves no balance ⇒ outflow 0 ⇒
  M1 stays silent (verified: `m1_ignores_delegate_only_transaction` shows F2 fires, M1 does not, even at
  `max_value_out: 0`). M1 measures realized value-out, not future authority — exactly the intended split.
- **Inert by default.** `stage0_default()` (`max_value_out = u64::MAX`) never fires (`u64::MAX > u64::MAX`
  is false) ⇒ `default_bank()` is untouched and every pre-existing test passes.
- **Value-out only leaves via existing user accounts.** Post-only (newly created) keys have no `pre_token`
  and are skipped, so inflows/new accounts are correctly not counted.

## Independent verification (CC ran these)

- `cargo test --offline` (engine): **13 passed, 0 failed** — the 8 pre-existing invariant tests plus the
  5 new M1 cases (`m1_flags_outflow_above_authored_limit_but_allows_below_it`,
  `m1_sums_outflow_across_user_accounts_of_the_same_mint`, `m1_ignores_delegate_only_transaction`,
  `m1_counts_a_closed_funded_account_as_full_outflow`, `m1_stage0_default_is_inert`).
- `cargo build --offline`: clean, **no warnings**. The cross-repo `reexec-spec` path dependency resolves
  offline (workspace-inheritance concern did not materialize).
- `cargo run --bin mandate_demo`: same 800-token payment prints `default bank: Green` then
  `authored max_value_out=500: Red` with `[M1-mandate] moves 800 … allows at most 500`. Honest demo.

## Notes (not findings)

- Like F1–F5, M1 does not gate on `Outcome.success`; a reverted tx whose post-state equals pre yields 0
  outflow, so no false positive. Consistent with the existing invariants — no change requested.
- `max_value_out` is a per-mint token-amount cap (no cross-asset USD normalization) — as scoped; pricing
  is deferred per the brief.

**Ready to merge.** Fold Step 2 complete: one authored `MandateSpec`, checked at certify (Probatio,
size/instrument) and screen (Custos, `max_value_out`).
