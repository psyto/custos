# Task 001 (Custos) — `MandateConformance`: the screen station of the shared mandate

**Cross-repo fold, Step 2b.** Pairs with probatio-svm task 020 (merged). Implemented by **Codex** in the
Custos window on a branch; **CC reviews**. This makes "one authored mandate, checked at certify AND
screen" real: Custos now reads the **same** `reexec-spec::MandateSpec` that Probatio certifies against.

## Why

Probatio (certify) proves an agent's *episode* stays within its mandate before capital moves. Custos
(screen) must enforce the *same authored mandate* on the *next transaction*, pre-broadcast. Task 020
extracted `MandateSpec` into the dependency-free `reexec-spec` crate and added `max_value_out` — the
generic, screen-checkable field. This task adds the Custos invariant that reads it.

Demo target (the money shot, ties to the Grok/Bankr prompt-injection drain, ~$150–200K): an authored
`{ max_value_out: X }` lets a benign tx through but fires **RED** on a tx moving `> X` out of the user's
accounts **even when the tx "succeeds"** — a tricked agent still cannot exceed its authored mandate.

## Dependency

Add to `engine/Cargo.toml`:
```toml
reexec-spec = { path = "../../probatio-svm/crates/reexec-spec" }
```
`reexec-spec` is `#![no_std]` + dependency-free; it builds cleanly as a leaf dep. If cargo objects to its
workspace-inherited manifest fields when consumed cross-repo, flag it in the handoff (the fix is to make
`reexec-spec/Cargo.toml` self-contained — a probatio-svm follow-up, **do not** edit that repo from here).

## Scope

New invariant in `engine/src/lib.rs`, **separate from the F1–F5 malice tier**:

```rust
pub struct MandateConformance { pub mandate: reexec_spec::MandateSpec }
```

`impl Invariant for MandateConformance` — `code() = "M1-mandate"`:

- Compute, **per mint**, the net token value leaving accounts the user controls:
  for every key in the outcome, if `pre_token.owner == o.user`, add
  `pre.amount.saturating_sub(post.amount)` to that mint's outflow; if the account was **closed**
  (`post_token` is `None` while `pre.amount > 0`), the full `pre.amount` counts as outflow.
- If **any single mint's** aggregate user-outflow `> mandate.max_value_out`, emit one RED
  `Finding { level: Red, code: "M1-mandate", account: <a representative user account of that mint>,
  message: "moves N of <mint> out of your accounts; authored mandate allows at most max_value_out" }`.
- `max_value_out == u64::MAX` (the `stage0_default`) ⇒ never fires (inert), so nothing regresses.

**Boundary (keep M1 distinct from the malice tier):** M1 is about **realized value-out this tx**. It must
NOT try to duplicate F1 (full-drain), F2 (delegate), F3 (authority), F4 (close-grab): a delegate/approval
grant moves **no** balance, so M1 correctly ignores it (that stays F2's job). M1 only sums realized
outflow. A closed account that held tokens counts its balance as outflow (that overlaps F4's *account*
signal, but M1's lens is *value*, and it only fires when the value exceeds the authored cap).

**Wiring:** keep `default_bank()` unchanged (malice tier only, so all existing tests pass). Add:
```rust
pub fn bank_with_mandate(mandate: reexec_spec::MandateSpec) -> Vec<Box<dyn Invariant>> {
    let mut b = default_bank();
    b.push(Box::new(MandateConformance { mandate }));
    b
}
```
Wire one demo path (e.g. a `mandate_demo` bin or a scenario) that authors a `max_value_out` and shows the
GREEN→RED flip on the same tx family under `bank_with_mandate`.

## Acceptance criteria / gates

- `cargo build` clean, no new warnings; `cargo test` green (all existing tests unchanged + new ones).
- New tests: (a) user account balance `1000 -> 200` with `max_value_out: 500` ⇒ M1 RED (300 out? no —
  800 out > 500); with `max_value_out: 900` ⇒ no M1. (b) split across two accounts of the same mint sums
  to the mint total. (c) a **delegate-only** tx (no balance change) ⇒ **no** M1 finding (F2's job, not M1).
  (d) closed funded account counts `pre.amount` as outflow. (e) `stage0_default()` ⇒ M1 never fires.
- No test hits the network (invariants run over an in-memory `Outcome`, as the F1–F5 tests do).

## Out of scope

- Cross-asset/USD normalization of `max_value_out` (v1 is per-mint token-amount; pricing is future).
- Other envelope fields (allowed-programs/counterparties), time-varying mandates.
- Promoting `reexec-spec` to its own repo (`psyto/reexec-core`) — tri-lane Step 3.
- Any edit to the probatio-svm repo from the Custos window (walls: cross-repo writes stay one-directional).
