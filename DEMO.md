# Custos — demo walkthrough (≈3 min)

A guided script for a live demo or screen recording. Each step lists **what to run**,
**what to show**, and **what to say**.

Prereqs: Rust toolchain, the Solana CLI (for program dumps), network access. First run
is slower (programs are dumped once, then cached).

---

## 0. Setup (before recording)

```bash
git clone https://github.com/psyto/custos && cd custos
# warm the program cache so the live demo is snappy:
cd engine && cargo run --bin scan -- 3aRTnj6iE1YJqmuM6V7NL6PinwSaE6hJ4M2BPTSUsPS68BiUEzvEzib5kNyqM5QwYJGehYyrYXTGJMM2G7UfkXNN >/dev/null 2>&1
```

---

## 1. The gap (30s) — `cargo run --bin demo`

**Show:** the four-scenario table.

**Say:** "A balance-diff scanner asks *do tokens move?* Custos asks *what happens to the
accounts you control?* Watch the difference."

- Scenario 1 (benign memo): both GREEN — no false alarms.
- Scenario 2 (hidden delegate): balance-diff **GREEN**, Custos **RED (F2)** — an
  `Approve(unlimited)` moves no tokens now but hands an attacker your funds later.
- Scenario 3 (ownership theft): balance-diff **GREEN**, Custos **RED (F3)**.
- Scenario 4 (unknown program): the `Approve` is hidden inside an unknown program's CPI.
  An instruction parser sees only an opaque call — Custos reads the resulting state:
  **RED (F2 + F5)**.

---

## 2. It's real, not a mock (40s) — the pre-sign firewall

```bash
cd api && cargo run    # → http://127.0.0.1:8787
```

In a second terminal, show the actual product loop over HTTP:

```bash
# a malicious dApp builds an unsigned tx targeting a REAL mainnet USDC account
curl -s localhost:8787/api/build | tee /tmp/b.json
# Custos checks that exact tx BEFORE signing
TX=$(python3 -c "import json;print(json.load(open('/tmp/b.json'))['tx_base64'])")
OWNER=$(python3 -c "import json;print(json.load(open('/tmp/b.json'))['owner'])")
curl -s -X POST localhost:8787/api/scan -H 'content-type: application/json' \
  -d "{\"tx_base64\":\"$TX\",\"user\":\"$OWNER\"}" | python3 -m json.tool
```

**Show:** `level: RED`, `F2-delegate`, `naive_level: GREEN`, `timing.total_ms ≈ 250–360`.

**Say:** "This is a real on-chain account. The tx grants an unlimited delegate. Balance-diff
says safe; Custos blocks it in a quarter second — before Phantom is ever asked to sign."

---

## 3. The wallet experience (30s) — the UI

Open `http://127.0.0.1:8787`.

**Show:** the four approval cards (Custos: BLOCK vs balance-diff panel), then the
**Live pre-sign firewall** section:

- *Connect Phantom.*
- Click **"Receive a 'claim airdrop' tx (drainer)"** → RED banner: *"Signing blocked —
  Phantom was never prompted."* (the wallet is never asked to sign.)
- Click **"Receive a harmless tx"** → GREEN → Phantom pops up to sign → *"Signed by
  Phantom (not broadcast)."*

**Say:** "RED never reaches the wallet. GREEN goes straight to Phantom. Same engine,
before you sign."

---

## 4. No false alarms + speed (30s)

```bash
cd engine
cargo run --bin scan -- <any recent Jupiter swap signature>   # → GREEN
cargo run --bin profile                                        # → cold vs warm latency
```

**Say:** "Real Jupiter swaps scan GREEN — no crying wolf. And it's fast: warm scans are
~235 ms because programs are cached; only live account state is fetched."

---

## 5. Agent / solver gate (45s) — no human in the loop

**Say:** "Same engine, one layer up — for an AI agent that pays on its own, with nobody
watching."

```bash
# authorization PASS vs Custos BLOCK on the SAME transaction (offline, deterministic)
cd engine && cargo run --bin agent_demo
```

**Show:** the **HERO** block — a real authorization policy (amount ≤ limit, destination
allowlisted) returns **PASS** on a 5 USDC payment to an allowlisted merchant; that same
tx also hides an unlimited `Approve`; Custos returns **RED (F2)** → *REFUSE TO
BROADCAST*. The 5 USDC transfer raises no finding (it matches the declared payment) — the
RED is driven purely by the hidden delegate.

```bash
# the same gate as a solver wires it — over the live HTTP API
cd api && cargo run          # if not already running (step 2)
python3 scripts/solver_gate.py
```

**Show:** `Authorization policy (declared intent): PASS` → `Custos verification (actual
tx): RED` → `Decision: REFUSE TO BROADCAST — principal capital preserved`.

**Say:** "Authorization checks what the agent *declared*; Custos re-executes what the
transaction *does*. Put it on the one road before broadcast, and a compromised agent
can't drain the desk."

---

## Closing line

"Signature scanners catch yesterday's scams. Custos simulates your next transaction and
reads what it does to *your* accounts — so it stops drainers no one has seen yet.
Open-source, self-hostable, ~235 ms."
