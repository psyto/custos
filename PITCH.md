# Custos — the pre-sign firewall for Solana

*Simulate the transaction you're about to sign against real mainnet state, and check
what happens to **your** accounts — before you sign.*

---

## The problem

Solana wallet drainers rarely "steal tokens" in the transaction you sign. The malicious
transaction moves **zero balance** — it grants a delegate, reassigns an account's
authority, or routes through an unknown program — and the drain happens **one
transaction later**. The user sees "no funds leaving" and signs.

## Why today's checks miss it

| Approach | Blind spot |
| -------- | ---------- |
| **Signature / allowlist scanners** | only catch drainers they've *already seen*; a fresh contract or address is invisible. |
| **Balance-diff simulators** | judge safety by whether tokens move *in this tx* — so an `Approve(unlimited)` or `SetAuthority` reads as SAFE (nothing moved). |
| **Instruction parsers** | enumerate known-dangerous instruction shapes — bypassed when the dangerous op is hidden inside an unknown program's CPI. |

## The Custos approach

Custos **simulates the exact transaction** against **current mainnet state** in a local
VM (LiteSVM), then judges by the **post-simulation state of the accounts you control**,
not by instruction shapes:

> *Did any account I own gain a delegate, change authority, get closed, or lose value?*

Because it reads end-state, it is **pattern-agnostic** — it catches novel encodings and
CPI-hidden operations a parser never enumerated. Because it simulates against real state,
it needs no prior knowledge of the scam.

**Invariants today:** F1 drain · F2 delegate-grant · F3 authority-change ·
F4 account-close · F5 unknown-program. (5, with unit tests.)

## Evidence (all runnable, on real mainnet data)

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

- Custos is an **engine + API + wallet-approval UI**, verified end-to-end on real data.
- It is **not** claimed to beat a specific mature incumbent (Blockaid/Blowfish also
  simulate). The durable edge is **architectural**: verdicts are state invariants, robust
  to novel encodings, and the engine is **open + self-hostable**.
- The Phantom signing flow is real code but not yet browser-verified; the pre-sign
  `build → scan → verdict` backend is curl-verified on real accounts.

## Ask / next

Design partners (wallets, dApps) to embed the pre-sign `/api/scan` check; and a dedicated
RPC to drive live-state latency below 100 ms.

*Repo: [`psyto/custos`](https://github.com/psyto/custos). Built on the LiteSVM/invariant
substrate from [`psyto/solinv`](https://github.com/psyto/solinv).*
