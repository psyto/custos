# Custos — demo videos

Two cuts, both rendered headlessly (Puppeteer → Chromium → ffmpeg). No screen
recording, no narration track, no external assets.

| cut | script | output | length |
|---|---|---|---|
| **60 s — the application cut** | `demo60.html` + `record60.js` | `custos-60s.mp4` | ~59 s |
| 30 s — build-in-public / outreach | `demo.html` + `record.js` | `custos-agent-demo.mp4` | ~30 s |

```bash
npm install
npm run record60   # → custos-60s.mp4   (needs ffmpeg on PATH)
npm run record     # → custos-agent-demo.mp4
```

`FFMPEG_PATH` overrides the ffmpeg location (default `/opt/homebrew/bin/ffmpeg`).
Output mp4s are gitignored.

## What the 60-second cut shows

Four shots, no voice — the narration from the storyboard is carried as on-screen
captions.

1. **0:00–0:08** — the claim. 2026-05-04, a prompt injection moved ~$155,000 out of an
   AI agent's wallet (Bankr/Grok; publicly reported at $155–175K). The authorization
   policy passed it.
2. **0:08–0:38** — `agent_demo`. A real declared-intent policy **PASSES** a 5 USDC
   payment to an allowlisted merchant; the same transaction hides an unlimited
   `Approve`; Custos re-executes and returns **RED (F2)** → *refuse to broadcast*.
3. **0:38–0:52** — `mandate_demo`. The same payment is Green under the default bank and
   **Red under an authored `max_value_out = 500`** — M1 measures realized outflow, not
   the declared amount.
4. **0:52–1:00** — close card.

Storyboard and narration source: `_applications/demo-script.md` (outside this repo).

## Why the terminal in the video is trustworthy

**`record60.js` runs the binaries and animates their actual stdout.** It shells out to
`cargo run -q --bin agent_demo` and `--bin mandate_demo` in `../engine`, captures the
output, injects it into the page as `window.__CUSTOS_OUTPUT`, and only then records. The
page colours lines by pattern but **never rewrites them**, and the recorder aborts if the
hero lines (`Authorization policy`, `RED`, `Decision:`, `M1-mandate`) are missing from
the captured output.

So the account addresses on screen are the ones that run generated — they differ every
time, which is the visible sign that the output is real rather than transcribed. The
literal block inside `demo60.html` is a fallback for opening the page by hand.

The one thing it is not: a capture of a human typing in a terminal. It is an animation of
bytes the binaries printed seconds earlier.
