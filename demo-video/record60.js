// Records the 60-second demo (demo60.html) to custos-60s.mp4.
//
// Unlike a hand-authored storyboard, this runs the two binaries first and feeds
// their ACTUAL stdout into the page, so the terminal in the video shows the bytes
// this run produced — not a transcription of an older run.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import puppeteer from "puppeteer";
import { PuppeteerScreenRecorder } from "puppeteer-screen-recorder";

const dir = path.dirname(fileURLToPath(import.meta.url));
const engine = path.join(dir, "..", "engine");
const pageUrl = "file://" + path.join(dir, "demo60.html");
const outFile = path.join(dir, "custos-60s.mp4");

function runBin(name) {
  process.stderr.write(`• running ${name} …\n`);
  return execFileSync("cargo", ["run", "-q", "--bin", name], {
    cwd: engine,
    encoding: "utf8",
    maxBuffer: 8 * 1024 * 1024,
  }).replace(/\s+$/, "");
}

// Keep only the hero block of agent_demo — the four corroborating scenarios that
// follow are printed under "Custos also blocks" and do not fit a 60s cut.
const agentFull = runBin("agent_demo");
const agent = agentFull.split("\nCustos also blocks")[0].replace(/\s+$/, "");
const mandate = runBin("mandate_demo");

for (const [label, text] of [["agent_demo", agent], ["mandate_demo", mandate]]) {
  if (!text.trim()) throw new Error(`${label} produced no output`);
}
if (!/Authorization policy/.test(agent) || !/RED/.test(agent) || !/Decision:/.test(agent)) {
  throw new Error("agent_demo output is missing the hero lines — refusing to record a misleading video");
}
if (!/M1-mandate/.test(mandate)) {
  throw new Error("mandate_demo output is missing the M1 finding — refusing to record");
}

const browser = await puppeteer.launch({
  headless: "new",
  defaultViewport: { width: 1280, height: 720, deviceScaleFactor: 1 },
  args: ["--no-sandbox", "--hide-scrollbars", "--window-size=1280,720", "--force-device-scale-factor=1"],
});
const page = await browser.newPage();
page.on("console", (m) => {
  if (m.text().startsWith("DEMO_SECONDS")) process.stderr.write(`• page reports ${m.text()}\n`);
});
await page.evaluateOnNewDocument((payload) => { window.__CUSTOS_OUTPUT = payload; }, { agent, mandate });

const recorder = new PuppeteerScreenRecorder(page, {
  fps: 30,
  ffmpeg_Path: process.env.FFMPEG_PATH || "/opt/homebrew/bin/ffmpeg",
  videoFrame: { width: 1280, height: 720 },
  aspectRatio: "16:9",
});

await recorder.start(outFile);
await page.goto(pageUrl, { waitUntil: "load" });
await page.waitForFunction("window.__DEMO_DONE === true", { timeout: 180000 });
await new Promise((r) => setTimeout(r, 400));
await recorder.stop();
await browser.close();
console.log("wrote", outFile);
