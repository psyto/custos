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

## Status: Stage 0 — technical gate GREEN

Custos is at the earliest stage: the load-bearing technical assumption has been
validated, and no product exists yet.

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
| F5 | unknown-program CPI guard | CPI into a non-allowlisted program, esp. with authority ops | YELLOW→RED |
| F6 | hidden-instruction guard | the UI-described action ≠ the instructions actually in the tx | RED |
| F7 | CPI-reentrancy (from solinv) | anomalous CPI structure | YELLOW |
| F8 | CU-anomaly (from solinv) | near compute-unit ceiling | INFO |

F1–F6 are new (user-safety); F7–F8 are transferred from solinv (protocol-safety).

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
    └── src/bin/demo.rs     #   3 scenarios, one engine, vs balance-diff
```

### Stage 1 (in progress): engine core

`engine/` is the reusable core: `simulate → snapshot → invariants → verdict`.
`cargo run --bin demo` (in `engine/`) runs three prospective transactions
through the same engine beside a balance-diff-only scanner:

```
Benign claim (memo)          balance-diff GREEN | Custos GREEN
Hidden delegate (Approve MAX) balance-diff GREEN | Custos RED  (F2)
Silent ownership theft (SetAuthority) balance-diff GREEN | Custos RED (F3)
```

The invariant bank (`F1Drain`, `F2DelegateGrant`, `F3AuthorityChange`) judges by
post-simulation **state**, with unit tests. Next: a `scan` CLI/HTTP surface over
a live State Loader, then the wallet UI.

## Related

- [`psyto/solinv`](https://github.com/psyto/solinv) — invariant fuzzing framework; the simulation engine Custos reuses.
- [`psyto/pinocchio-bench`](https://github.com/psyto/pinocchio-bench) — CU benchmark + differential-verification harnesses.

---

*Internal working name. Latin* custos *= guardian / sentinel.*
