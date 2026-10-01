// Measure cold page response bodies through the built service, without header overrides.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium, webkit } from "playwright-core";
import { codeflowBinary } from "./codeflow-binary.mjs";
import { installedChrome, playwrightBuild } from "./browser-executables.mjs";
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const binary = codeflowBinary(repoRoot);
const evidence = join(repoRoot,"target/tsk193-evidence/transport");
await mkdir(evidence,{recursive:true});
const manifest = JSON.parse(await readFile(join(repoRoot,"crates/codeflow-present/assets/manifest.json"),"utf8"));
const framed = JSON.parse(await readFile(join(repoRoot,"crates/codeflow-present/tests/fixtures/contract-v2/documents/v2-framed.json"),"utf8"));
const engines = [
  ["chrome",chromium,await installedChrome(),"br"],
  ["webkit",webkit,await playwrightBuild(webkit),"gzip"],
];
const cases = [
  ["prose",{type:"narrative",id:"prose",markdown:"A bounded page for transport measurement."},null,215_000],
  ["code",{type:"code",id:"code",language:"rust",code:"fn main() {}"},"[data-cf-highlight='ready']",275_000],
  ["figure",framed.blocks.find(b => b.type === "figure"),"[data-cf-figure-block='ready']",250_000],
];
const results=[];
for (const [engine,browser,executablePath,encoding] of engines) for (const [name,block,ready,budget] of cases) {
  const root=await mkdtemp(join(evidence,`${engine}-${name}-`));
  const project=join(root,"project");
  const env={PATH:process.env.PATH,HOME:join(root,"home"),TMPDIR:join(root,"tmp"),XDG_STATE_HOME:join(root,"state"),CF_PRESENT_OWNER_PID:String(process.pid)};
  for(const path of [project,env.HOME,env.TMPDIR,env.XDG_STATE_HOME]) await mkdir(path,{recursive:true});
  const cli=args=>{const r=spawnSync(binary,args,{cwd:project,env,encoding:"utf8",timeout:60_000});assert.equal(r.status,0,r.stderr);return r.stdout;};
  const file=join(project,"page.json"); await writeFile(file,JSON.stringify({schema_version:2,title:"Transport measurement",blocks:[block]}));
  let id,context;
  try {
    const opened=cli(["present","open",file,"--no-launch"]);
    id=opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const bootstrap=opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];assert.ok(id&&bootstrap,opened);
    context=await browser.launchPersistentContext(join(root,"profile"),{executablePath,headless:true});
    const page=context.pages()[0];const responses=[];page.on("response",r=>responses.push(r));
    await page.goto(pathToFileURL(bootstrap).href,{waitUntil:"commit"});
    await page.waitForURL(/http:\/\/127\.0\.0\.1:\d+\/app\//u,{waitUntil:"domcontentloaded"});
    await page.locator("#cf-comment-toggle").waitFor();
    if(ready) await page.locator(ready).first().waitFor();
    await page.evaluate(()=>document.fonts.ready);
    assert.ok(responses.some(r=>r.url().includes("/chunk-fonts-")),"font request observed");
    let assetBytes=0,htmlBytes=0;
    for(const response of responses) {
      const path=new URL(response.url()).pathname;
      if(path.startsWith("/app/assets/")) {
        assert.equal(response.status(),200);
        assert.equal(response.headers()["content-encoding"],encoding);
        const asset=manifest.service.assets.find(a=>a.request_path===path&&a.content_encoding===encoding);assert.ok(asset);
        assetBytes+=asset.encoded_bytes;
      } else if(path==="/app/") htmlBytes+=(await response.body()).length;
    }
    const total=assetBytes+htmlBytes;assert.ok(total<=budget,`${name}: ${total} > ${budget}`);
    results.push({engine,name,encoding,assetBytes,htmlBytes,total,budget});
    console.log(`${engine} ${name}: ${total} B (${assetBytes} assets + ${htmlBytes} HTML), budget ${budget} B`);
  } finally {if(context) await context.close();if(id)cli(["present","close",id]);await rm(root,{recursive:true,force:true});}
}
await writeFile(join(evidence,"results.json"),JSON.stringify(results,null,2)+"\n");
console.log("Transport budgets: 6 cold page loads passed");
