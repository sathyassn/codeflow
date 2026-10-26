// Drive headless Chrome over CDP: open the present bootstrap link, then capture the live page in six skin and mode pairs.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const [,, chrome, bootstrap, outDir] = process.argv;
const port = 9333;
const proc = spawn(chrome, ["--headless", "--disable-gpu", "--hide-scrollbars", `--remote-debugging-port=${port}`, "--window-size=1280,1000", "--user-data-dir=/tmp/claude-501/tsk444/profile", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await sleep(1500);
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0; const pending = new Map();
ws.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true })).result?.result?.value;

await send("Page.enable");
await send("Page.navigate", { url: bootstrap });
await sleep(6000);
console.log("at", await evaluate("location.href"), "blocks", await evaluate("document.querySelectorAll('[data-cf-block-id]').length"));
for (const skin of ["graphite", "slate", "sage"]) for (const mode of ["light", "dark"]) {
  await evaluate(`localStorage.setItem('cf-present-theme','${skin}'); localStorage.setItem('cf-present-mode','${mode}'); 'ok'`);
  await send("Page.reload");
  await sleep(5000);
  const h = await evaluate("document.documentElement.scrollHeight");
  const probe = await evaluate(`JSON.stringify({skin: document.documentElement.dataset.cfTheme, mode: document.documentElement.dataset.cfModeResolved, h: ${h}, notDrawn: document.body.innerText.match(/not drawn|stopped at/g)?.length ?? 0, cat1: getComputedStyle(document.querySelector('.opt-cat') ?? document.body).getPropertyValue('--cf-fig-cat1'), h2: [...document.querySelectorAll('#cf-present-document h2')].slice(0,3).map(e => getComputedStyle(e).fontSize)})`);
  console.log(probe);
  await send("Emulation.setDeviceMetricsOverride", { width: 1280, height: Math.min(h, 16000), deviceScaleFactor: 1, mobile: false });
  await sleep(800);
  const shot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: true });
  writeFileSync(`${outDir}/live-${skin}-${mode}.png`, Buffer.from(shot.result.data, "base64"));
  console.log("wrote", `live-${skin}-${mode}.png`, h);
}
ws.close(); proc.kill();
