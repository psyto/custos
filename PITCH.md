# Custos — a pre-broadcast execution firewall for agent payments

*Re-execute the transaction an autonomous agent is about to broadcast, against real
mainnet state, and refuse to broadcast it when it does more than the agent declared.*

---

## The problem

**Every guard shipped for agent payments today is an authorization layer**: amount under
a limit, destination on an allowlist, policy signed. Authorization reads the payment the
agent *declares*. It cannot read the transaction.

A 5 USDC payment to an allowlisted merchant, carrying an unlimited token approval in the
same transaction, passes every authorization policy in production right now. And the
malicious half moves **zero balance** — it grants a delegate, reassigns an account's
authority, or routes through an unknown program — so the drain lands **one transaction
later**, with no human in the loop to notice.

> **Authorization** asks *is this payment allowed?*  ·  **Custos** asks *does this
> transaction do only that — and nothing else?*

## Why today's checks miss it

| Approach | Blind spot |
| -------- | ---------- |
| **Authorization policies** (agent lane) | check the payment the agent *declares*; blind to everything else the raw transaction carries. |
| **Signature / allowlist scanners** | only catch drainers they've *already seen*; a fresh contract or address is invisible. |
| **Balance-diff simulators** | judge safety by whether tokens move *in this tx* — so an `Approve(unlimited)` or `SetAuthority` reads as SAFE (nothing moved). |
| **Instruction parsers** | enumerate known-dangerous instruction shapes — bypassed when the dangerous op is hidden inside an unknown program's CPI. |

## The Custos approach

Custos **simulates the exact transaction** against **current mainnet state** in a local
VM (LiteSVM), then judges by the **post-simulation state of the principal's accounts**,
not by instruction shapes:

> *Did any account the principal owns gain a delegate, change authority, get closed, or
> move more value out than was authored?*

Because it reads end-state, it is **pattern-agnostic** — it catches novel encodings and
CPI-hidden operations a parser never enumerated. Because it simulates against real state,
it needs no prior knowledge of the scam.

**Invariants today:** F1 drain · F2 delegate-grant · F3 authority-change ·
F4 account-close · F5 unknown-program · **M1 authored-mandate conformance**.
(6, 13 lib tests, covering SPL Token *and* Token-2022.)

## Evidence (all runnable, on real mainnet data)

- **Authorization passes, Custos refuses — on the same transaction.** A real
  declared-intent policy PASSES a 5 USDC payment to an allowlisted merchant; the same
  transaction hides an unlimited `Approve`; Custos re-executes and returns **RED (F2)**
  → *refuse to broadcast*. *(`agent_demo`.)*
- **An authored spend cap survives a tricked agent.** The same 800-token payment is
  Green under the default bank and **Red under `max_value_out = 500`** — M1 measures
  *realized* per-mint outflow after re-execution, not the declared amount.
  *(`mandate_demo`.)*
- **Catches what balance-diff misses.** A tx that leaves USDC balance unchanged but
  grants an unlimited delegate: balance-diff → GREEN, Custos → **RED (F2)**, and the
  attacker drains the account one tx later. *(`live_red`, real on-chain USDC account.)*
- **Catches what parsers miss.** The same `Approve` hidden inside an unknown program's
  CPI: an instruction parser → GREEN, Custos → **RED (F2 + F5)**. *(demo scenario 4.)*
- **No false alarms on real DeFi.** Real Jupiter swaps (19–54 accounts, 4–8 programs)
  scan **GREEN**. *(`scan <signature>`.)*
- **Works on real multi-CPI txs.** A real Jupiter swap assembles + executes to CPI
  depth 3 in the local VM. *(Gate D.)*
- **Fast enough to sign against.** Warm scan ≈ **235 ms** (programs cached; only account
  state is fetched live). Cold first-sight ≈ 1.2 s, a one-time per-program cost.
  *(`profile`.)*

## What it is / isn't (honest scope)

- Custos is an **engine + HTTP API + solver gate**, verified end-to-end on real data
  (plus a wallet-approval UI for the second caller).
- It is **not** claimed to beat a specific mature incumbent (Blockaid/Blowfish also
  simulate) on the consumer lane — see *The moat, and why it is agent-only* below. The
  durable edge is **architectural**: verdicts are state invariants, robust to novel
  encodings, and the engine is **open + self-hostable**, so a solver can run it inside
  its own pipeline with no third party on the payment road.
- The Phantom signing flow is real code but not yet browser-verified; the pre-sign
  `build → scan → verdict` backend is curl-verified on real accounts.

## The moat, and why it is agent-only

North star: **F6 / intent-conformance** — *the transaction does only what was declared* —
which subsumes F1–F5. It needs a declared intent **trustworthy enough to check against**.
For a consumer wallet that input is a phishing site's own description, so checking
against it is worthless and the incumbents cannot build F6 on their lane. For an agent it
is the agent's own policy layer — signed identity, action class, amount, counterparty —
**trustworthy by construction**. M1 is the first authored slice of F6 that ships today.

Honest scope: Custos blocks structural drains (F1–F5) plus the authored cap (M1). A
hidden *partial* transfer to an attacker is the next invariant, not a claim made today.

## Second caller — the human signer

The same engine, one caller down: given the transaction a dApp is asking a person to
sign, it renders a wallet-style approval card (`cd api && cargo run`). Kept because it is
the cheapest way to *see* an invariant fire — not because the consumer wallet is the
market.

## Ask / next

Design partners on the agent lane — **solvers and agent-wallet operators** wanting a
pre-broadcast gate (white-label) — to embed the `/api/scan` check; and a dedicated RPC to
drive live-state latency below 100 ms.

*Repo: [`psyto/custos`](https://github.com/psyto/custos). Built on the LiteSVM/invariant
substrate from [`psyto/solinv`](https://github.com/psyto/solinv).*
