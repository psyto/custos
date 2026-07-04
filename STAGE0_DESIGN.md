# Custos — Stage 0 Design

Status: **Stage 0 (gate validated)** · Last updated: 2026-07-04

## 0. One-liner

Simulate a prospective transaction against real mainnet state and check what happens
to *your* accounts before you sign. A Solana execution firewall built on invariant
checks over a simulated outcome, not on a scam-address allowlist.

## 1. Why this, why now

- The Solana Frontier Hackathon (10,000+ participants, 2,857 projects) rewarded
  **applied consumer products with founder-market fit**, not protocols. An
  execution-firewall project (Sudont) placed in the Top 25; security + risk-scoring
  were live winning categories. Sponsor mix (Phantom, Arcium, World, Coinbase)
  signals a consumer-safety / privacy / identity tailwind.
- Custos is the shape that lets deep low-level Solana + simulation + invariant work
  present as a *visible consumer product* rather than infrastructure.
- **Founder-market fit is uniquely strong here:** the hard part — a Solana-aware
  invariant engine over a LiteSVM simulator, calibrated on 168K mainnet executions —
  already exists as [`psyto/solinv`](https://github.com/psyto/solinv). Custos points
  it at the user's next transaction instead of a protocol's fuzz campaign.

## 2. Core technical insight

A fuzzer *constructs* malicious instruction variants → it needs a per-instruction
interface definition (`InstructionSpec`) and hits Anchor-version ABI gates. This was
solinv's most expensive cost (see solinv Days 14–17).

A firewall *replays a concrete transaction* → it loads the real on-chain `.so` and
the real accounts and just executes. **The most expensive fuzzer cost does not apply
to the firewall.** Only the cheap, valuable substrate carries over: simulation +
mainnet account cloning + outcome observation.

## 3. Architecture

```
[dApp / phishing site]  --unsigned tx-->  Custos
  1. State Loader   getMultipleAccounts + program .so   → clone touched accounts/programs
  2. Sim Engine     LiteSVM.execute(tx)                 → run REAL programs, capture pre/post + logs + CU
  3. Invariant Bank run detectors over the outcome      → fire user-protective invariants
  4. Verdict        GREEN / YELLOW / RED + reason        → user signs or rejects
```

Product surfaces (priority order): (a) hosted "connect wallet → pre-flight verdict"
web app; (b) wallet extension; (c) embeddable dApp SDK; (d) co-signer policy engine (B2B).

## 4. solinv asset-reuse map (honest)

| solinv asset | Reuse in Custos | Degree |
| ------------ | --------------- | ------ |
| Crucible / LiteSVM simulation substrate | tx-replay engine base | ★★★ direct |
| solinv-corpus mainnet seeder (RPC/Yellowstone account clone) | State Loader | ★★★ direct |
| bytepoke (account disc / sighash decode, byte writers) | tx + token-balance decode | ★★★ direct |
| realloc-race Path-B pre/post snapshot infra | balance-delta / ownership-change base | ★★ ported |
| cpi-reentrancy log parser (`Program … invoke [depth]`) | CPI-tree view + F7 | ★★ ported |
| cu-dos detector | F8 CU anomaly | ★★ ported |
| solinv-core invariant trait architecture | scaffold for new invariants | ★★★ direct |
| Critical-5 (signer/owner/disc/pda-forge/account-swap) | attack-construction model; **does not passively apply** to a benign real tx. Only account-swap's *idea* reshapes into F3 | ★ idea only |
| Calibration record (168K exec / 0 violation) + OSS brand | judge-facing credibility / FMF proof | ★★★ as-is |

**Honest takeaway:** the engine substrate carries over wholesale; the *value-bearing
invariants are new* (F1–F6), because solinv's 10 invariants are protocol-safety and
Custos needs user-safety. Do not claim the existing catalog wins this on its own.

## 5. Invariant catalog

See README §"Invariant catalog". F1–F6 new (user-safety); F7–F8 transferred.
The load-bearing ones for the demo are **F1 (balance-drain)**, **F2 (delegate/approval)**,
and **F6 (hidden-instruction)** — the modal drainer signature.

## 6. Demo scenarios (the money shot)

Three-panel comparison; the point is **"a signature-based scanner shows green while
simulation + invariants show red."**

1. **Benign (Jupiter swap)** → GREEN, with a balance-change preview (`USDC −100 / SOL +0.42`, known programs only).
2. **Novel drainer ("claim your airdrop")** → hidden `SetAuthority` grants an unknown delegate over the user's USDC. **F2/F6 fire → RED**, plain language: "This transaction gives an unknown address permission to move all your USDC." Side panel: signature scanner (drainer address unlisted) says *safe*.
3. **Subtle (looks like a normal stake)** → simulated post-state shows a user-owned account's owner changed. **F3 fires** — caught structurally, never seen before.

## 7. Differentiation (honest)

Blockaid / Blowfish already simulate and preview balance changes. Custos competes on:
(1) invariant depth + Solana account-model specificity; (2) OSS / self-hostable;
(3) embeddable SDK + co-signer. If the depth cannot be *shown* to beat incumbents in
a live demo, Custos is a second-mover. The demo is the product.

## 8. Technical gate (Stage 0) — DONE, GREEN

| Gate | Proves | Result |
| ---- | ------ | ------ |
| A | clone real mainnet account into LiteSVM, byte round-trip | ✅ GREEN |
| B | load + execute arbitrary mainnet BPF `.so`, capture logs/CU/pre-post | ✅ GREEN |
| C | real SPL-token transfer → token-balance pre/post delta (F1/F2 heart) | ⏳ pending |
| D | replay a real multi-CPI dApp tx (Jupiter swap) against cloned state | ⏳ pending |

Code: [`gate/`](./gate). Gate B log shows a mainnet-dumped binary executing with
`invoke [1] … success`, CU 28,410, 5,000-lamport fee captured as a pre/post delta.

## 9. Roadmap

- **Stage 0 (now):** gates A/B GREEN. Next: **Gate C → Gate D** (cheap-to-expensive
  confidence). Gate C de-risks the core F1/F2 balance-delta observation; Gate D
  closes the practicality question (multi-program CPI replay).
- **Stage 1 (MVP, weekend-scale):** State Loader + LiteSVM replay + F1/F2/F6 +
  hosted "connect wallet → verdict" web UI + the 3-scenario demo with the
  signature-scanner comparison panel.
- **Stage 2:** F3–F5 + CPI-tree visualization; embeddable SDK.
- **Later:** co-signer policy engine (B2B); Arcium-backed private simulation.

## 10. Risks / open questions

1. **Multi-CPI real-tx replay (Gate D).** Memo is self-contained; a Jupiter swap
   CPIs into many programs. Can we assemble all touched programs + accounts into
   LiteSVM? This is the practicality gate. *Green-leaning but unproven.*
2. **Latency.** State clone + sim must land under ~1.5 s for signing UX. Cloning
   only touched accounts should suffice — verify.
3. **Incumbent gap must be demonstrable.** Beating Blockaid/Blowfish has to show in
   the demo, not in prose.
4. **Branding.** solinv is public; ship Custos under its own name and cite solinv as
   the engine supplier, not as the same product.
