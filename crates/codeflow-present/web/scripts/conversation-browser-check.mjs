// TSK-193: the built service and CLI, driven through task-owned browser profiles.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium, firefox, webkit } from "playwright-core";
import { codeflowBinary } from "./codeflow-binary.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const binary = codeflowBinary(repo);
const evidence = resolve(process.env.CF_PRESENT_EVIDENCE ?? join(repo,"target/tsk193-evidence/browser"));
await mkdir(evidence,{recursive:true});
const cache = join(homedir(),"Library/Caches/ms-playwright");
async function cached(prefix,suffix) {
  const versions = (await readdir(cache)).filter(n => n.startsWith(prefix)).sort((a,b) => Number(b.slice(prefix.length))-Number(a.slice(prefix.length)));
  if (!versions.length) throw new Error(`No installed ${prefix} browser`);
  return join(cache,versions[0],suffix);
}
const engines = {
  chrome: [chromium,"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"],
  firefox: [firefox,await cached("firefox-","firefox/Nightly.app/Contents/MacOS/firefox")],
  webkit: [webkit,await cached("webkit-","pw_run.sh")],
};
const selected = process.env.CF_PRESENT_ENGINES?.split(",") ?? Object.keys(engines);
const widths = process.env.CF_PRESENT_WIDTHS?.split(",").map(Number) ?? [1280,375];
const results = [];
for (const engine of selected) for (const width of widths) {
  const root = await mkdtemp(join(evidence,`${engine}-${width}-`));
  const project = join(root,"project");
  const env = {PATH:process.env.PATH,HOME:join(root,"home"),TMPDIR:join(root,"tmp"),XDG_STATE_HOME:join(root,"state"),CF_PRESENT_OWNER_PID:String(process.pid),LANG:"C.UTF-8"};
  for (const path of [project,env.HOME,env.TMPDIR,env.XDG_STATE_HOME]) await mkdir(path,{recursive:true});
  const cli = (args) => {
    const r = spawnSync(binary,args,{cwd:project,env,encoding:"utf8",timeout:60_000});
    if (r.error) throw r.error;
    assert.equal(r.status,0,`${args.join(" ")}: ${r.stderr}`); return r.stdout;
  };
  const parsed = args => JSON.parse(cli(args));
  const git = spawnSync("git",["init","--quiet"],{cwd:project,env,encoding:"utf8"}); assert.equal(git.status,0,git.stderr);
  const document = {schema_version:2,title:"Conversation qualification",summary:"A bounded review round trip",blocks:[
    {type:"narrative",id:"intro",markdown:"The reviewer marks this explanation before the agent changes it."},
    {type:"form",id:"question",title:"Keep this approach?",fields:[{id:"keep",label:"Keep the approach",kind:"boolean"}],required:["keep"]},
    {type:"narrative",id:"removed",markdown:"This block will be removed."},
  ]};
  const file = join(project,"document.json"); await writeFile(file,JSON.stringify(document));
  let id; let context;
  try {
    const opened = cli(["present","open",file,"--no-launch"]);
    id = opened.match(/session ([0-9a-f-]+) ready/u)?.[1];
    const bootstrap = opened.match(/owner-private bootstrap file (.+?) in a qualified/u)?.[1];
    assert.ok(id && bootstrap,opened);
    const [browser, executablePath] = engines[engine];
    context = await browser.launchPersistentContext(join(root,"profile"),{executablePath,headless:true,viewport:{width,height:900}});
    const page = context.pages()[0] ?? await context.newPage();
    const errors=[]; page.on("pageerror",e => { errors.push(e.message); console.error(`${engine} page error: ${e.message}`); });
    page.on("response", async response => {
      if (response.status() >= 400 && response.url().includes("/app/assets/")) console.error(`${engine} asset ${response.status()} ${response.url()} accept-encoding=${(await response.request().allHeaders())["accept-encoding"]}`);
    });
    await page.goto(pathToFileURL(bootstrap).href,{waitUntil:"commit"});
    await page.waitForURL(/http:\/\/127\.0\.0\.1:\d+\/app\//u,{waitUntil:"domcontentloaded"});
    await page.locator("#cf-comment-toggle").waitFor();
    async function openComments() {
      await page.getByTestId("comment-btn").click();
      await page.locator('#cf-feedback-panel[data-open="true"]').waitFor();
      if (width < 880) {
        const header = page.locator("#cf-feedback-panel > .hd");
        await header.click({trial:true});
        const box = await header.boundingBox(); assert.ok(box);
        await page.mouse.move(box.x + 60,Math.min(box.y + box.height - 8,899 - 8));
        await page.mouse.down(); await page.mouse.move(box.x + 60,box.y + 6,{steps:4}); await page.mouse.up();
        await page.locator('#cf-feedback-panel[data-expanded="true"]').waitFor();
      }
    }
    await openComments();
    // The keyboard picker is a supported, visible commenting route at either width.
    await page.locator("details.cf-tools > summary").click();
    await page.getByTestId("tool-pick-element").click();
    await page.locator("#cf-present-document[data-cf-capture-mode='element'] :focus").waitFor();
    await page.keyboard.press("Enter");
    await page.getByTestId("composer-text").fill("Please clarify this explanation.");
    await page.getByTestId("composer-save").click();
    await page.getByTestId("composer").waitFor({state:"detached"});
    await page.getByTestId("submit-all").click();
    await page.locator('.cf-chrome-frame[data-commenting="false"]').waitFor({state:"attached"});
    const form = page.locator("article[data-cf-form='question']");
    await form.locator("input[type=radio][value=true]").check();
    await form.locator("[data-cf-form-action='submit']").click();
    await page.locator("article[data-cf-form='question'][data-cf-form-state='stored']").waitFor();
    const delivered = cli(["present","feedback",id,"--format","v2"]).trim().split("\n").map(JSON.parse);
    const review = delivered.find(e => e.kind === "review"); const answer = delivered.find(e => e.kind === "answer");
    assert.ok(review?.notes[0] && answer);
    cli(["present","resolve",id,review.event_id,"--event-version","2","--status","addressed"]);
    cli(["present","ack",id,answer.event_id]);
    cli(["present","reply",id,review.event_id,"--note",review.notes[0].id,"I will clarify the explanation."]);
    cli(["present","reply",id,answer.event_id,"I will keep this approach."]);
    await openComments();
    await page.locator("[data-testid=feedback-history] > summary").click();
    const note = page.locator(`[data-note='${review.notes[0].id}']`);
    await note.getByTestId("thread-reply").waitFor();
    assert.match(await note.innerText(),/I will clarify the explanation/u);
    await note.getByRole("button",{name:"Reopen",exact:true}).click();
    await note.getByTestId("thread-reopened").waitFor();
    const reopened = cli(["present","feedback",id,"--format","v2"]).trim().split("\n").map(JSON.parse);
    assert.deepEqual(reopened.map(e => [e.kind,e.target,e.note_id]),[["reopen",review.event_id,review.notes[0].id]]);
    cli(["present","ack",id,reopened[0].event_id]);
    // The target revision is authored through the real CLI, then read in the page.
    document.blocks[0].markdown += " The revision now gives the reason.";
    document.blocks = document.blocks.filter(b => b.id !== "removed");
    document.blocks.push({type:"narrative",id:"added",markdown:"A new conclusion."});
    await writeFile(file,JSON.stringify(document)); cli(["present","update",id,file]);
    const diff = parsed(["present","diff",id,"--from","1","--to","2"]);
    assert.equal(diff.blocks.find(b => b.id === "intro").status,"changed");
    assert.equal(diff.blocks.find(b => b.id === "added").status,"added");
    assert.equal(diff.blocks.find(b => b.id === "removed").status,"removed");
    assert.equal(diff.blocks.flatMap(b => b.notes)[0].carried,true);
    assert.equal(diff.blocks.flatMap(b => b.answers)[0].carried,true);
    await page.reload({waitUntil:"domcontentloaded"}); await openComments();
    await page.locator("[data-testid=feedback-history] > summary").click();
    await note.getByTestId("thread-reply").waitFor();
    await note.getByTestId("thread-reply").scrollIntoViewIfNeeded();
    await page.screenshot({path:join(evidence,`${engine}-${width}-conversation.png`),fullPage:true});
    await note.getByRole("button",{name:"Delete note",exact:true}).click();
    await note.getByTestId("thread-tombstone").waitFor();
    assert.doesNotMatch(await page.locator("[data-testid=feedback-history]").innerText(),/Please clarify this explanation|I will clarify the explanation/u);
    assert.equal(parsed(["present","history",id]).response_events.filter(e => e.event === "tombstone")[0].revision,2);
    for (const path of ["/sample.rs","/app/repo/sample.rs","/app/api/files/sample.rs"]) {
      const response = await page.request.get(new URL(path,page.url()).href); assert.equal(response.status(),404);
    }
    assert.deepEqual(errors,[]);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
    assert.equal(overflow,false,`${engine}/${width} horizontal overflow`);
    results.push({engine,version:context.browser()?.version() ?? "persistent",width,checks:12,session:id});
    console.log(`${engine} ${width}px: 12 checks passed; open, comment, answer, reply, reopen, diff, carry, reload, delete, revision, route refusal, layout`);
  } catch(error) {
    if (context) { const page = context.pages()[0]; if (page) await page.screenshot({path:join(evidence,`${engine}-${width}-failure.png`),fullPage:true}).catch(()=>{}); }
    throw error;
  } finally {
    if(context) await context.close();
    if(id) { cli(["present","close",id]); }
    await rm(root,{recursive:true,force:true});
  }
}
await writeFile(join(evidence,"results.json"),JSON.stringify(results,null,2)+"\n");
console.log(`Conversation journey: ${results.length} engine/width runs passed`);
