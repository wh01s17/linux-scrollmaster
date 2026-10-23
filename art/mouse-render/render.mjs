// SPDX-License-Identifier: GPL-3.0-or-later
// Render the original mouse model as a turntable image sequence for the app.
//   node art/mouse-render/render.mjs --preview 0,40,320 [--out dir]
//   node art/mouse-render/render.mjs                    frames + stills + anchors
// Development only: needs Chromium and the three devDependency; the app ships the
// rendered WebP files, never three.js or a mesh.
import { createServer } from "node:http";
import { spawn, execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : fallback; };
export const FRAMES = 72;
export const ZONES = ["pointer", "wheel", "thumb", "buttons"];
// Turntable angle that shows each part; the stills are rendered there.
export const ANGLES = { pointer: 20, wheel: 335, thumb: 290, buttons: 245 };
const SIZE = { w: 840, h: 680 };

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript" };
const server = createServer((req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)).replace(/^\/+/, "");
  const file = join(ROOT, path);
  if (!file.startsWith(ROOT) || !existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" }).end(readFileSync(file));
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const port = server.address().port;

const profile = mkdtempSync(join(tmpdir(), "mouse-render-"));
const chrome = spawn("chromium", ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--hide-scrollbars", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
const wsUrl = await new Promise((ok, fail) => {
  let text = "";
  chrome.stderr.on("data", (chunk) => { text += chunk; const m = text.match(/DevTools listening on (ws:\/\/\S+)/); if (m) ok(m[1]); });
  setTimeout(() => fail(new Error("Chromium did not start")), 20000);
});
const browser = new WebSocket(wsUrl);
await new Promise((ok) => browser.addEventListener("open", ok, { once: true }));
let id = 0;
const pending = new Map();
browser.addEventListener("message", (event) => { const msg = JSON.parse(event.data); if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); } });
const send = (method, params = {}, sessionId) => new Promise((ok, fail) => { const n = ++id; pending.set(n, (m) => (m.error ? fail(new Error(`${method}: ${m.error.message}`)) : ok(m.result))); browser.send(JSON.stringify({ id: n, method, params, sessionId })); });
const { targetId } = await send("Target.createTarget", { url: `http://127.0.0.1:${port}/art/mouse-render/page.html?w=${SIZE.w}&h=${SIZE.h}&ss=2${opt("--query", "")}` });
const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
const evaluate = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }, sessionId);
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
};
for (let i = 0; i < 200 && !(await evaluate("window.ready === true").catch(() => false)); i++) await new Promise((ok) => setTimeout(ok, 100));

const png = async (file, angle, highlight = null) => {
  const url = await evaluate(`renderFrame(${angle}, ${JSON.stringify(highlight)})`);
  writeFileSync(file, Buffer.from(url.split(",")[1], "base64"));
};
// Supersampled PNG → final-size WebP with alpha.
const webp = (from, to) => execFileSync("magick", [from, "-filter", "Lanczos", "-resize", `${SIZE.w}x${SIZE.h}`, "-quality", "82", "-define", "webp:alpha-quality=90", to]);

try {
  if (argv.includes("--debug")) {
    await evaluate("renderFrame(0)");
    console.log(JSON.stringify(await evaluate("window.__debug ?? null")));
  } else if (argv.includes("--ortho")) {
    const dir = opt("--out", join(ROOT, "art", "mouse-render", "preview"));
    mkdirSync(dir, { recursive: true });
    for (const view of opt("--ortho", "top").split(",")) {
      const url = await evaluate(`ortho(${JSON.stringify(view)})`);
      writeFileSync(join(dir, `ortho-${view}.png`), Buffer.from(url.split(",")[1], "base64"));
    }
  } else if (argv.includes("--preview")) {
    const dir = opt("--out", join(ROOT, "art", "mouse-render", "preview"));
    mkdirSync(dir, { recursive: true });
    for (const spec of opt("--preview", "0").split(",")) {
      const [angle, zone] = spec.split(":");
      const file = join(dir, `a${angle}${zone ? "-" + zone : ""}.png`);
      await png(file, Number(angle), zone ?? null);
      console.log(file);
    }
  } else {
    const out = join(ROOT, "src", "mouse3d", "frames");
    rmSync(out, { recursive: true, force: true });
    mkdirSync(out, { recursive: true });
    const tmp = mkdtempSync(join(tmpdir(), "mouse-frames-"));
    const anchors = [];
    for (let f = 0; f < FRAMES; f++) {
      const angle = (360 * f) / FRAMES;
      await png(join(tmp, "f.png"), angle);
      webp(join(tmp, "f.png"), join(out, `f${String(f).padStart(2, "0")}.webp`));
      anchors.push(await evaluate(`anchors(${angle})`));
      process.stdout.write(`\rframe ${f + 1}/${FRAMES}`);
    }
    for (const zone of ZONES) {
      await png(join(tmp, "s.png"), ANGLES[zone], zone);
      webp(join(tmp, "s.png"), join(out, `lit-${zone}.webp`));
    }
    rmSync(tmp, { recursive: true, force: true });
    writeFileSync(join(ROOT, "src", "mouse3d", "turntable.json"), JSON.stringify({ frames: FRAMES, width: SIZE.w, height: SIZE.h, angles: ANGLES, anchors }));
    console.log(`\nwrote ${FRAMES} frames, ${ZONES.length} stills and anchors`);
  }
} finally {
  browser.close();
  chrome.kill();
  await new Promise((ok) => (chrome.exitCode !== null ? ok() : chrome.once("exit", ok)));
  server.close();
  rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
