# Custos

**Simulate a transaction against real mainnet state and check what happens to *your* accounts — before you sign.**

Custos is a Solana **execution firewall**. Given an unsigned transaction (the one a
dApp or a phishing site is asking you to sign), Custos clones the relevant mainnet
account state into a local VM, executes the transaction against the *real* on-chain
programs, and runs a catalog of **user-protective invariants** on the simulated
outcome. It returns a plain-language GREEN / YELLOW / RED verdict with a reason.

The bet: **signature- and allowlist-based scanners catch yesterday's scams. Custos
simulates and checks invariants, so it can catch a drainer it has never seen before.**

Custos reuses the LiteSVM / Crucible simulation substrate proven in
[`psyto/solinv`](https://github.com/psyto/solinv) (a Solana-aware invariant fuzzing
framework with a 168K-execution mainnet calibration record). solinv points that
engine at a protocol's fuzz campaign; Custos points the same engine at your next
transaction.

---

## Quick start (≈30s)

```bash
# 1. See the engine catch drainers a balance-diff scanner misses (offline)
cd engine && cargo run --bin demo

# 2. The wallet-approval UI (three-panel comparison + Phantom intercept)
cd api && cargo run          # → http://127.0.0.1:8787

# 3. Scan a REAL mainnet transaction by signature
cd engine && cargo run --bin scan -- <SIGNATURE>

# 4. Real account × prospective drainer → RED on live state
cd engine && cargo run --bin live_red
```

No config needed; a public RPC is the default (set `CUSTOS_RPC` for a faster one).
See **[PITCH.md](./PITCH.md)** for the one-page case, **[DEMO.md](./DEMO.md)** for a
guided walkthrough, and **[SUBMISSION.md](./SUBMISSION.md)** for the full story +
evidence ledger.

---

## Status: Stage 1 — engine + live firewall working

The technical gate is GREEN and the engine now runs end-to-end: it scans real mainnet
transactions, flags real prospective drainers (RED) while passing benign swaps (GREEN),
and serves a wallet-approval UI. Five invariants (F1–F5) with unit tests; a pre-sign
`/api/scan` endpoint; measured warm latency ~235 ms. Below is how it got here.

### Gate history (Stage 0)

The gate answered one question: **can a local VM (LiteSVM) load an *arbitrary*
mainnet program, clone a *real* mainnet account, execute a transaction against them,
and observe the outcome (logs, compute units, pre/post state)?** If not, the whole
"simulate before signing" premise is dead.

The gate answered one question: **can a local VM (LiteSVM) load an *arbitrary*
mainnet program, clone a *real* mainnet account, execute a transaction against them,
and observe the outcome (logs, compute units, pre/post state)?** If not, the whole
"simulate before signing" premise is dead.

| Gate | What it proves | Result |
| ---- | -------------- | ------ |
| **A** | Clone a real mainnet account (USDC mint) into LiteSVM; byte-identical round-trip | ✅ GREEN |
| **B** | Dump an arbitrary mainnet BPF program (Memo, 74 KB `.so`) from mainnet, load it, execute it, capture logs + CU + pre/post lamports | ✅ GREEN |
| **D** | Replay a real multi-CPI **Jupiter swap** (19 accounts, 4 programs incl. Jupiter 2.9 MB + Raydium CLMM 1.7 MB) against cloned state; CPI tree executes to depth 3 | ✅ GREEN¹ |
| **C** | Real SPL-token transfer → observe **token-balance** pre/post delta (the heart of the "your funds moved" invariant) | ⏳ next |

¹ *Gate D reaches program logic (`Jupiter Route → Raydium CLMM Swap`) and fails only
on a stale-price check (`RequireGtViolated`), because state is cloned at the current
slot while historical replay would need archival state. This confound does not exist
in the product path (simulating a **prospective** tx against **current** state). The
load-bearing question — can a local VM assemble + execute a real multi-program CPI tx —
is answered: yes. Reusable finding: warp the VM clock past every ALT's
`last_extended_slot` or resolution fails with `InvalidAddressLookupTableIndex`.*

Gates A + B establish the *mechanics*; Gate D establishes *product realism* (real
multi-program CPI replay assembles and executes). A runnable **incumbent-gap proof**
(`cargo run --bin proof_f2`) shows the payoff: a tx that leaves USDC balance unchanged
but grants an attacker an unlimited delegate — a balance-diff preview returns GREEN,
Custos's F2 post-state invariant returns RED, and the attacker drains the account one
tx later. A second proof (`cargo run --bin proof_f2_nested`) hides the same `Approve`
inside a custom program's CPI: a top-level instruction parser returns GREEN while
Custos's post-state invariant returns RED — showing the verdict layer must read state,
not parse instruction shapes. (Honest caveat: these illustrate blind spots of
*balance-diff* and *instruction-parsing*, not a head-to-head win over a specific
simulating incumbent — see `STAGE0_DESIGN.md` §8b.) See
[`STAGE0_DESIGN.md`](./STAGE0_DESIGN.md) for the full design and asset-reuse map.

### Reproduce the gate

```bash
# artifacts/ already contains memo.so + usdc_mint.json dumped from mainnet.
# To refresh them:
#   solana program dump MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr gate/artifacts/memo.so -u m
#   solana account EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v -u m --output json > gate/artifacts/usdc_mint.json

cd gate
cargo run -q
```

Expected tail: `=== GATE RESULT: GREEN — firewall premise holds ===`

---

## How it works (target architecture)

```
[dApp / phishing site]  --unsigned tx-->  Custos
                                            |
  1. State Loader  --getMultipleAccounts--> mainnet RPC   (clone touched accounts + programs)
  2. Sim Engine    --LiteSVM.execute------->              (run the REAL programs, capture pre/post + logs)
  3. Invariant Bank --run detectors-------->              (fire on the simulated outcome)
  4. Verdict + plain-language reason
                                            |
                            GREEN / YELLOW / RED  -->  [user signs or rejects]
```

Because Custos **replays a concrete transaction** (rather than *constructing*
malicious instruction variants the way a fuzzer does), it sidesteps solinv's most
expensive cost — per-protocol `InstructionSpec` authoring and Anchor-version ABI
gates. It loads the real on-chain bytecode and real accounts and just executes.

## Invariant catalog (the IP)

User-protective invariants that fire on the simulated post-state — protecting the
*signer*, not the protocol:

| # | Invariant | Fires when | Verdict |
| - | --------- | ---------- | ------- |
| F1 | balance-drain guard | user SOL/token balance leaves beyond intent | RED |
| F2 | delegate/approval guard | `Approve`(unlimited) / `SetAuthority` grants an unknown delegate over the user's token account | RED |
| F3 | ownership-change guard | an account the user owns changes owner | RED |
| F4 | account-close guard | a user account is closed, balance to a third party | RED |
| F5 | unknown-program CPI guard | tx invokes a program off the verified allowlist | YELLOW |
| F6 | hidden-instruction guard | the UI-described action ≠ the instructions actually in the tx | RED |
| F7 | CPI-reentrancy (from solinv) | anomalous CPI structure | YELLOW |
| F8 | CU-anomaly (from solinv) | near compute-unit ceiling | INFO |

F1–F6 are new (user-safety); F7–F8 are transferred from solinv (protocol-safety).
**Implemented today: F1 (drain), F2 (delegate), F3 (authority), F4 (account-close),
F5 (unknown-program)** — 8 unit tests, and they cover **both SPL Token and Token-2022**
accounts (drainers increasingly use Token-2022; the base layout is shared and mints are
disambiguated by the account-type byte). F5 stays silent on real DeFi (majors are
allowlisted: System, Token/Token-2022, ATA, Memo, Jupiter, Raydium AMM+CLMM, Orca,
Meteora, Phoenix), so benign real swaps remain GREEN. F6 needs a dApp-declared intent
input and is deferred.

## Differentiation (honest)

Blockaid / Blowfish already do simulation-based previews. Custos competes on three
axes only:

1. **Invariant depth + Solana account-model specificity** — catch by *structure*,
   not by a scam-address list.
2. **OSS / self-hostable** — the closed SaaS incumbents are not.
3. **Embeddable SDK + co-signer policy engine** — distributable beyond one wallet.

If the invariant depth can't be *shown* to beat the incumbents in a live demo, this
is a second-mover. The demo is the product.

## Repository layout

```
custos/
├── README.md
├── STAGE0_DESIGN.md        # full design, asset-reuse map, roadmap
├── gate/                   # Stage 0 technical gates + incumbent-gap proofs
│   ├── src/main.rs         #   gate A/B, gate_d, proof_f2, proof_f2_nested
│   ├── proxy-approve/      #   custom BPF program for the nested-CPI proof
│   └── artifacts/          #   mainnet .so + account dumps
└── engine/                 # Stage 1 engine (the real core)
    ├── src/lib.rs          #   Outcome + invariant bank (F1/F2/F3) + Verdict
    ├── src/sim.rs          #   LiteSVM capture (pre/post snapshot)
    ├── src/spl.rs          #   SPL Token wire helpers
│   ├── src/scenarios.rs    #   shared built-in scenarios (demo + API)
│   ├── src/bin/demo.rs     #   4 scenarios, one engine, vs balance-diff
│   ├── src/bin/scan.rs     #   LIVE path: scan a real mainnet tx by signature
│   └── src/bin/live_red.rs #   LIVE RED: real account state × prospective drainer
├── api/                    # axum HTTP service over the engine
│   └── src/main.rs         #   GET /api/demo (verdicts) + GET / (UI)
└── web/
    └── index.html          # wallet-approval UI (balance-diff vs Custos panels)
```

### Wallet UI

```bash
cd api && cargo run          # → http://127.0.0.1:8787
```

The page simulates each transaction and renders a wallet-style approval card with two
panels — a balance-diff scanner and the Custos engine — plus a Block/Safe-to-sign
control. The hidden-delegate and ownership-theft cards show balance-diff GREEN next to
Custos RED. Verdicts come live from `GET /api/demo` (the engine's shared `scenarios`
module).

The **Live pre-sign firewall** section wires Phantom end-to-end: connect the wallet,
then receive either a drainer or a harmless tx. Custos runs `build → scan` and, on a
**RED** verdict, blocks — Phantom is never prompted. On a **GREEN** verdict it
deserializes the tx (`@solana/web3.js`) and calls `window.solana.signTransaction`. The
`build`/`scan` backend is curl-verified on real data (benign → GREEN, drainer → RED);
the in-browser Phantom signing is real code pending a browser+wallet verification.

### Stage 1 (in progress): engine core

`engine/` is the reusable core: `simulate → snapshot → invariants → verdict`.
`cargo run --bin demo` (in `engine/`) runs four prospective transactions
through the same engine beside a balance-diff-only scanner:

```
Benign claim (memo)                    balance-diff GREEN | Custos GREEN
Hidden delegate (Approve MAX)          balance-diff GREEN | Custos RED  (F2)
Silent ownership theft (SetAuthority)  balance-diff GREEN | Custos RED  (F3)
Routed through an unknown program      balance-diff GREEN | Custos RED  (F2 + F5)
```

The invariant bank (`F1Drain`, `F2DelegateGrant`, `F3AuthorityChange`,
`F4AccountClose`, `F5UnknownProgram`) judges by post-simulation **state**, with
unit tests. The last scenario hides the `Approve` inside an unknown program's CPI —
an instruction parser sees only an opaque call, but the engine reads the resulting
delegate state (F2) and flags the unverified program (F5).

**Live path** (`cargo run --bin scan -- <SIGNATURE>`): fetch a real mainnet tx,
clone every touched account (ALT-resolved) + every invoked program into LiteSVM,
simulate, and run the bank — emitting a `verdict_json`. Verified end-to-end on real
Jupiter swaps (e.g. 54 accounts / 8 programs), correctly GREEN on benign swaps with
no false findings even when historical replay reverts on stale price.

**Latency** (`cargo run --bin profile`): a scan is timed by phase, cold vs warm, with
an in-process program-ELF cache. Measured on a real Jupiter swap (4 programs incl.
Jupiter 2.9 MB + Raydium CLMM 1.7 MB) over the public RPC:

```
COLD (first sight, CLI dump) : 1161 ms  = resolve 35 + accounts 102 + programs 992 + sim 4
WARM (programs cached)       :  235 ms  = resolve 31 + accounts  89 + programs  87 + sim 3
```

Programs are static, so their ~1 s acquisition is a one-time cost — a service
pre-warms the top ~50 DeFi programs and nearly every real scan is WARM (~235 ms, well
under a signing-UX budget). The only live-bound term is account state (~89 ms here),
which drops further on a dedicated RPC (Helius/Triton). This quantifies the "programs
cacheable; only account state is live" thesis the design leaned on.

**Live RED** (`cargo run --bin live_red`): the product's true mode — clone a *real*
on-chain USDC token account (current state) and simulate a *prospective* drainer
(memo "claim" + hidden `Approve(u64::MAX)`). On real account state the engine returns
RED (F2, unlimited delegate) while a balance-diff scanner returns GREEN — no
historical-replay confound, because a prospective tx's correct pre-state *is* current
state. Next: the wallet UI, F1 refinement, and more invariants (unknown-program CPI,
account-close).

## Related

- [`psyto/solinv`](https://github.com/psyto/solinv) — invariant fuzzing framework; the simulation engine Custos reuses.
- [`psyto/pinocchio-bench`](https://github.com/psyto/pinocchio-bench) — CU benchmark + differential-verification harnesses.

---

*Internal working name. Latin* custos *= guardian / sentinel.*
