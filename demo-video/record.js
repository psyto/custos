// Drives Chromium (via Puppeteer) through demo.html and screen-records it to custos-agent-demo.mp4.
import path from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer";
import { PuppeteerScreenRecorder } from "puppeteer-screen-recorder";

const dir = path.dirname(fileURLToPath(import.meta.url));
const pageUrl = "file://" + path.join(dir, "demo.html");
const outFile = path.join(dir, "custos-agent-demo.mp4");

const browser = await puppeteer.launch({
  headless: "new",
  defaultViewport: { width: 1280, height: 720, deviceScaleFactor: 1 },
  args: ["--no-sandbox", "--hide-scrollbars", "--window-size=1280,720", "--force-device-scale-factor=1"],
});
const page = await browser.newPage();

const recorder = new PuppeteerScreenRecorder(page, {
  fps: 30,
  ffmpeg_Path: "/opt/homebrew/bin/ffmpeg",
  videoFrame: { width: 1280, height: 720 },
  aspectRatio: "16:9",
});

await recorder.start(outFile);
await page.goto(pageUrl, { waitUntil: "load" });
await page.waitForFunction("window.__DEMO_DONE === true", { timeout: 180000 });
await new Promise((r) => setTimeout(r, 600));
await recorder.stop();
await browser.close();
console.log("wrote", outFile);
