# Custos

**An execution firewall for autonomous agent payments.** Custos re-executes the
transaction an agent is about to broadcast — against real mainnet state, through the
real on-chain programs — and refuses to broadcast it when the transaction does more
than the agent declared.

> **Authorization** asks *is this payment allowed?*  ·  **Custos** asks *does this
> transaction do only that — and nothing else?*

Every guard shipped for agent payments today is an **authorization** layer: amount under
a limit, destination on an allowlist, policy signed. Authorization reads the payment the
agent *declares*. It cannot read the transaction. A 5 USDC payment to an allowlisted
merchant, carrying an unlimited token approval in the same transaction, passes every
authorization policy in production right now:

```
$ cargo run --bin agent_demo
Authorization policy (declared intent): PASS
Custos verification (actual tx): RED ⛔
  [F2-delegate] 9cGb…q74g — grants delegate GELb…X1mP authority over ALL of your tokens (unlimited)
Decision: REFUSE TO BROADCAST — principal capital preserved
```

Given an unsigned transaction, Custos clones the touched mainnet accounts and the real
on-chain programs into a local VM, executes the transaction, and runs a bank of
invariants on the **simulated post-state** — not on instruction shapes, not on a
scam-address list. It returns a plain-language GREEN / YELLOW / RED verdict with a
reason, in **~250 ms warm** on a real Jupiter swap ([measured](#stage-1-in-progress-engine-core)).

The bet: **signature- and allowlist-based scanners catch yesterday's scams. Custos
re-executes and checks invariants, so it can catch a drain it has never seen before.**

Custos reuses the LiteSVM / Crucible simulation substrate proven in
[`psyto/solinv`](https://github.com/psyto/solinv) (a Solana-aware invariant fuzzing
framework with a 168K-execution mainnet calibration record). solinv points that
engine at a protocol's fuzz campaign; Custos points the same engine at the next
transaction an agent is about to send.

**Two callers, one engine.** The same check guards the moment *before an action
commits* — whether an **autonomous agent / solver** is about to broadcast on its own
with no human in the loop (**the lane this repo is built for**, and the only lane where
the moat feature is buildable — see [Differentiation](#differentiation-honest)), or a
**human** is about to sign in their wallet ([second caller](#second-caller--the-human-signer-wallet-ui)).

---

## Quick start (≈30s)

```bash
# 1. THE HERO: authorization PASS vs Custos RED on the SAME tx (offline)
cd engine && cargo run --bin agent_demo

# 2. Authored spend mandate: same payment Green (default) → Red (max_value_out = 500)
cd engine && cargo run --bin mandate_demo

# 3. Solver pre-broadcast gate over the live HTTP API
cd api && cargo run &         # → http://127.0.0.1:8787
python3 scripts/solver_gate.py

# 4. Scan a REAL mainnet transaction by signature
cd engine && cargo run --bin scan -- <SIGNATURE>

# 5. Real account × prospective drainer → RED on live state
cd engine && cargo run --bin live_red

# 6. The invariant bank beside a balance-diff scanner, four scenarios (offline)
cd engine && cargo run --bin demo

# 7. Second caller — the wallet-approval UI for a human signer
cd api && cargo run           # → http://127.0.0.1:8787
```

No config needed; a public RPC is the default (set `CUSTOS_RPC` for a faster one).
See **[PITCH.md](./PITCH.md)** for the one-page case, **[DEMO.md](./DEMO.md)** for a
guided walkthrough, and **[SUBMISSION.md](./SUBMISSION.md)** for the full story +
evidence ledger.

---

## For AI agents & solvers (pre-broadcast gate)

AI agents now hold wallets and pay on their own — over x402 / MPP, through agent
wallets, inside solver pipelines. There is **no human to catch a bad approval**, and the
usual guard is an *authorization* policy (amount ≤ limit, destination allowlisted) that
is blind to what the raw transaction actually does. That is the gap Custos closes.

Put Custos on the one road every payment must travel (the solver's pre-broadcast step,
or the wallet's signer): the agent runs automatically, but each transaction is
re-executed and RED ones never broadcast. Two runnable demos:

- **`cd engine && cargo run --bin agent_demo`** — a *real* declared-intent authorization
  policy PASSES a 5 USDC payment to an allowlisted merchant; the **same** transaction
  also hides an unlimited `Approve`; Custos re-executes and returns **RED (F2)** →
  *refuse to broadcast*. Authorization ≠ verification, proven on one transaction.
- **`python3 scripts/solver_gate.py`** — the same gate over the live HTTP API
  (`build → scan → refuse-to-broadcast`), the shape a solver actually integrates.

**Honest coverage (what blocks today).** Custos fires on the *structural* drains: F1
full-drain (balance → 0), F2 delegate/approval, F3 authority change, F4 account close,
F5 unknown program. A hidden **partial** transfer to an attacker — one that does *not*
empty the account — is **not** caught by these invariants, and it also slips past a
declared-intent authorization policy. Closing that is the next invariant, not a claim
made today.

**North-star — F6 / intent-conformance.** The complete answer is F6: *the transaction
does only what was declared, nothing more* — which subsumes F1–F5 (a hidden delegate,
authority grab, or extra transfer are all "beyond the declared intent"). F6 needs a
trustworthy declared-intent input. For a consumer that input is an untrusted dApp's
word; for an **agent** it is the agent's own policy layer — trustworthy by construction
— which is exactly why intent-conformance is the killer feature for this lane, and the
wedge this repositioning aims at.

### Authored-mandate conformance (M1) — the screen station of a shared mandate

Beyond the F1–F6 *malice* tier (what a tx does to you, regardless of intent), Custos enforces an
**authored spend mandate** shared with its sibling
[Probatio](https://github.com/psyto/probatio-svm). Both read the *same* `MandateSpec` from the
dependency-free [`reexec-spec`](https://github.com/psyto/probatio-svm/tree/master/crates/reexec-spec)
crate: **Probatio certifies** an agent's episode against `max_size` / `instrument`; **Custos screens**
the next transaction against **`max_value_out`**. The `MandateConformance` invariant (`M1-mandate`) fires
**RED** when the realized token outflow from your accounts — summed per mint — exceeds the authored
`max_value_out`, so even a *tricked* agent cannot move more than its mandate allows (the Grok/Bankr
prompt-injection drain class). It is deliberately separate from F1–F6: a delegate/authority change moves
no value, so M1 stays silent there (that is F2/F3's job) — M1 measures realized value-out.

`bank_with_mandate(spec)` = the malice bank plus M1; `default_bank()` stays malice-only and
`stage0_default()` leaves the cap off, so nothing regresses. One spec, authored once, checked at certify
time (Probatio) and pre-broadcast (Custos):

```bash
cd engine && cargo run --bin mandate_demo
#   same 800-token payment →  default bank: Green   |   authored max_value_out=500: Red  [M1-mandate]
```

---

## Status: Stage 1 — engine + live firewall working

The technical gate is GREEN and the engine now runs end-to-end: it screens an agent's
prospective transaction before broadcast, scans real mainnet transactions, flags real
prospective drainers (RED) while passing benign swaps (GREEN), and also serves a
wallet-approval UI for the human-signer caller. Five malice invariants (F1–F5) plus the authored-mandate **M1**
(the screen station of the `reexec-spec` mandate shared with Probatio), with unit tests; a pre-sign
`/api/scan` endpoint; measured warm latency ~250 ms. Below is how it got here.

### Gate history (Stage 0)

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
[agent / solver pipeline]  --unsigned tx-->  Custos      (or a dApp, for the human signer)
                                              |
  1. State Loader  --getMultipleAccounts-->  mainnet RPC   (clone touched accounts + programs)
  2. Sim Engine    --LiteSVM.execute------->               (run the REAL programs, capture pre/post + logs)
  3. Invariant Bank --run detectors-------->               (fire on the simulated outcome)
  4. Verdict + plain-language reason
                                              |
                              GREEN / YELLOW / RED  -->  [broadcast, or refuse]
```

Because Custos **replays a concrete transaction** (rather than *constructing*
malicious instruction variants the way a fuzzer does), it sidesteps solinv's most
expensive cost — per-protocol `InstructionSpec` authoring and Anchor-version ABI
gates. It loads the real on-chain bytecode and real accounts and just executes.

## Invariant catalog (the IP)

Invariants that fire on the simulated post-state — protecting the **account owner**
(the agent's principal, or the human signer), not the protocol:

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
| **M1** | **mandate-conformance** *(authored tier — shared with Probatio)* | realized per-mint token outflow from your accounts exceeds the authored `max_value_out` | RED |

F1–F6 are new (user-safety); F7–F8 are transferred from solinv (protocol-safety); **M1 is a separate
*authored-mandate* tier** (a spend cap you declare, not a malice pattern — the screen station of the
`reexec-spec::MandateSpec` shared with Probatio).
**Implemented today: F1 (drain), F2 (delegate), F3 (authority), F4 (account-close),
F5 (unknown-program), and M1 (mandate-conformance)** — 13 lib tests, and they cover **both SPL Token and Token-2022**
accounts (drainers increasingly use Token-2022; the base layout is shared and mints are
disambiguated by the account-type byte). F5 stays silent on real DeFi (majors are
allowlisted: System, Token/Token-2022, ATA, Memo, Jupiter, Raydium AMM+CLMM, Orca,
Meteora, Phoenix), so benign real swaps remain GREEN. F6 needs a declared-intent
input and is deferred — but it is the **killer invariant for the agent / solver lane**:
an autonomous agent's own policy layer *declares* its intent (trustworthy by
construction, unlike a phishing dApp's word), so F6 becomes *does the transaction do
only what the agent declared?* — full intent-conformance.

## Differentiation (honest)

Blockaid / Blowfish already ship simulation-based previews **for the consumer wallet**,
inside closed SaaS. On that lane a newcomer competes on invariant depth alone and is a
second mover. **This repo does not try to win there.**

**The moat is agent-only, and it is structural.** The complete answer is F6 /
intent-conformance — *the transaction does only what was declared* — which subsumes
F1–F5 (a hidden delegate, an authority grab, an extra transfer are all "beyond the
declared intent"). F6 needs a **declared intent trustworthy enough to check against**:

- For a **consumer wallet**, that input is a phishing site's own description of what it
  is asking you to sign. Checking a transaction against the attacker's claim is
  worthless — so the incumbents cannot build F6 on their lane, and not for want of
  engineering.
- For an **agent**, the declared intent is the agent's own policy layer: signed
  identity, action class, amount, counterparty. **Trustworthy by construction.**

So the axes are:

1. **Intent-conformance is buildable here and only here.** F6 is the north star and is
   **not shipped**; [M1](#authored-mandate-conformance-m1--the-screen-station-of-a-shared-mandate)
   is the first authored slice of it that is.
2. **Catch by structure, not by list** — invariants on the simulated post-state, with
   Solana account-model specificity (SPL Token *and* Token-2022, which drainers use).
3. **OSS / self-hostable** — the closed SaaS incumbents are not, and a solver can run
   the gate inside its own pipeline with no third party on the payment road.

**Not claimed**: a head-to-head win over a specific simulating incumbent. The
[`proof_f2`](#gate-history-stage-0) demos show the blind spots of *balance-diff* and
*instruction-parsing* previews — a narrower statement. See `STAGE0_DESIGN.md` §8b.

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
    ├── src/lib.rs          #   Outcome + malice bank (F1–F5) + M1 MandateConformance + bank_with_mandate + Verdict
    ├── src/sim.rs          #   LiteSVM capture (pre/post snapshot)
    ├── src/spl.rs          #   SPL Token wire helpers
│   ├── src/scenarios.rs    #   shared built-in scenarios (demo + API)
│   ├── src/bin/demo.rs       #   4 scenarios, one engine, vs balance-diff
│   ├── src/bin/agent_demo.rs #   agent/solver: authorization vs Custos (payment+hidden-delegate hero)
│   ├── src/bin/scan.rs       #   LIVE path: scan a real mainnet tx by signature
│   ├── src/bin/live_red.rs   #   LIVE RED: real account state × prospective drainer
│   └── src/bin/mandate_demo.rs # authored mandate: same tx Green (default) → Red (bank_with_mandate)
├── api/                    # axum HTTP service over the engine
│   └── src/main.rs         #   GET /api/demo (verdicts) + GET / (UI)
├── web/
│   └── index.html          # wallet-approval UI (balance-diff vs Custos panels)
└── scripts/
    └── solver_gate.py      # solver pre-broadcast gate over the live /api/scan
```

### Second caller — the human signer (wallet UI)

**Secondary lane.** The engine is the same; only the caller changes. It is kept because
it is the cheapest way to *see* an invariant fire, not because the consumer wallet is
the market — that argument is in [Differentiation](#differentiation-honest).

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
2026-09-04, public RPC, in-RAM program cache:
COLD (cache empty)     : 304 ms  = resolve 59 + accounts 110 + programs  92 + sim 5
WARM (programs cached) : 249 ms  = resolve 43 + accounts  90 + programs  88 + sim 3
```

The only live-bound term is **account state (~90 ms)**, which drops further on a
dedicated RPC (Helius/Triton); programs are static, so a service pre-warms the top ~50
DeFi programs and nearly every real scan is WARM. **~250 ms is well under a broadcast
or signing budget.** The absolute numbers move with the public RPC — an earlier run
(2026-08-02) measured 235 ms warm, and a genuinely first-sight program fetched by CLI
dump rather than RPC cost ~1 s, a one-time per-program cost. Re-run it yourself:
`cargo run --bin profile`.

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
