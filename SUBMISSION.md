# Custos — submission summary

*The pre-sign firewall for Solana: simulate the transaction you're about to sign against
real mainnet state, and check what happens to **your** accounts — before you sign.*

Repo: [`psyto/custos`](https://github.com/psyto/custos) · One-pager: [PITCH.md](./PITCH.md) ·
Demo script: [DEMO.md](./DEMO.md) · Quickstart: [README](./README.md#quick-start-30s)

---

## 1. Origin

Custos started from an analysis of the Solana Frontier Hackathon winners (10,000+
participants, 2,857 projects). The pattern: applied consumer products with clear
founder-market fit won; an execution-firewall project placed in the Top 25; security and
risk-scoring were live winning categories. The insight was to point a deep simulation
engine — the LiteSVM/invariant substrate proven in [`psyto/solinv`](https://github.com/psyto/solinv)
(168K-execution mainnet calibration) — at a **consumer safety** surface instead of a
protocol fuzz campaign.

## 2. Thesis

Solana drainers rarely move funds in the transaction you sign. They grant a delegate,
reassign an account's authority, or route through an unknown program — **zero balance
moves now**, the drain happens one tx later. So:

- **Signature/allowlist scanners** only catch drainers they've already seen.
- **Balance-diff simulators** call "no tokens moved" SAFE — missing delegation/authority.
- **Instruction parsers** enumerate known-bad shapes — bypassed by CPI-hidden ops.

Custos simulates the exact tx against current state and judges by the **post-simulation
state of the accounts you control** — pattern-agnostic, needs no prior knowledge of the
scam, robust to novel encodings.

## 3. What was built

| Layer | Artifact |
| ----- | -------- |
| Technical gates | load arbitrary mainnet BPF, clone real accounts, replay a real multi-CPI Jupiter swap |
| Differentiation proofs | balance-diff blind spot; instruction-parser blind spot (nested CPI) |
| Engine | `Outcome → invariant bank (F1–F5) → Verdict`; SPL Token + Token-2022; 8 unit tests |
| CLI | `demo`, `scan <sig>`, `live_red`, `profile` |
| HTTP API | `GET /api/demo`, `GET /api/build`, `POST /api/scan` |
| Wallet UI | 4-scenario approval screen + live Phantom pre-sign firewall (RED blocks, GREEN signs) |

Invariants: **F1** drain · **F2** delegate-grant · **F3** authority-change · **F4**
account-close · **F5** unknown-program.

## 4. Evidence ledger (every claim → a runnable command)

| Claim | Command | Result |
| ----- | ------- | ------ |
| Local VM runs an arbitrary mainnet program + real account | `cd gate && cargo run` | GREEN (gates A/B) |
| A real multi-CPI Jupiter swap assembles + executes | `cd gate && cargo run --bin gate_d` | CPI tree to depth 3 |
| Catches what balance-diff misses | `cd engine && cargo run --bin live_red` | balance-diff GREEN, Custos **RED (F2)**, drained 1 tx later — on a **real** USDC account |
| Catches what a parser misses (hidden in a CPI) | `cd engine && cargo run --bin demo` (scenario 4) | parser GREEN, Custos **RED (F2+F5)** |
| No false alarms on real DeFi | `cd engine && cargo run --bin scan -- <jupiter-sig>` | **GREEN** (19–54 accounts, 4–8 programs) |
| Pre-sign check over HTTP on real data | `curl /api/build` → `POST /api/scan` | **RED (F2)** in ~250–360 ms |
| Fast enough to sign against | `cd engine && cargo run --bin profile` | warm **235 ms** (cold 1161 ms, one-time) |
| Covers Token-2022, not just SPL Token | `cd engine && cargo test` | 8 tests pass |

## 5. How it was de-risked (an external critique, answered with data)

A structured critique flagged three make-or-break risks. Each was resolved with a
runnable artifact, not prose:

1. **"Multi-CPI replay is a different dimension / a non-working toy."** → **Gate D**: a
   real Jupiter swap (19 accounts, 4 programs incl. 2.9 MB Jupiter + 1.7 MB CLMM)
   assembled and executed. Real swaps measured at 14–54 accounts / 4–8 programs — bounded.
2. **"Differentiation is demo-deep."** → **`proof_f2` / `proof_f2_nested`**: a tx that
   grants an unlimited delegate with zero balance change (balance-diff GREEN, Custos RED),
   including one hidden inside an unknown program's CPI (parser GREEN, Custos RED).
3. **"Latency is fatal (MB `.so` per sign)."** → **`profile`**: programs are static and
   cached — cold first-sight ~1.2 s is a one-time per-program cost; steady-state ~235 ms,
   with account fetch the only live-bound term.

## 6. Honest scope

- Custos is an engine + API + wallet UI, verified end-to-end on **real mainnet data**.
- It is **not** claimed to beat a specific mature incumbent (Blockaid/Blowfish also
  simulate). The durable edge is **architectural** (state invariants, novel-encoding
  robustness) and **open/self-hostable**.
- The `build → scan` pre-sign backend is curl-verified; the in-browser Phantom
  `signTransaction` on GREEN is real code pending a browser+wallet verification.
- Historical replay uses current-slot state (can revert on stale prices); the product
  path — a prospective tx vs current state — has no such confound.
- Deferred: F6 (UI-intent mismatch, needs a dApp-declared intent input); a dedicated RPC
  + warm-VM reuse for sub-100 ms; browser-verified signing + a recorded demo.

## 7. Try it

```bash
git clone https://github.com/psyto/custos && cd custos
cd engine && cargo run --bin demo        # the gap, offline
cd ../api && cargo run                    # wallet UI → http://127.0.0.1:8787
cd ../engine && cargo run --bin live_red  # real account × prospective drainer → RED
```
