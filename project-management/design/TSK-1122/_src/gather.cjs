#!/usr/bin/env node
// TSK-1122 prototype: gather the live facts. Every number the prototype shows comes from this read.
// Reads: the task and epic records on the integration line and on origin/main (no fetch; run `git fetch origin`
// first), the record files' last change on the line, `gh pr list` and `gh api compare` (read only), and
// `codeflow work next` from the worktree. Writes _src/facts.json with the read's start and end time.
// Usage: node gather.cjs [--no-gh] [--no-next]   (gh and codeflow need host state: run outside the Bash sandbox)
//        node gather.cjs --refresh-records       re-reads only the record texts (task outcomes, epic summaries,
//        capability names, the docs diff) at the line tip already pinned in facts.json; counts, GitHub and work next
//        stay as read. Used when the page needs more of a record than the last read kept.
const { spawnSync } = require('child_process'); const fs = require('fs'); const path = require('path');
const { blocks } = require('./record-blocks.cjs');
const HERE = __dirname;
const REPO = path.resolve(HERE, '../../../..');            // the codeflow worktree this script lives in
const LINE = 'origin/integration/EPC-022-the-3-2-0-milestone-fixes', MAIN = 'origin/main';
const EPICS = ['EPC-022', 'EPC-024', 'EPC-025'];
const argv = process.argv.slice(2); const useGh = !argv.includes('--no-gh'), useNext = !argv.includes('--no-next'), refresh = argv.includes('--refresh-records');
const PREV = refresh ? JSON.parse(fs.readFileSync(path.join(HERE, 'facts.json'), 'utf8')) : null;
const est = d => new Date(d).toLocaleString('sv-SE', { timeZone: 'America/New_York' }).slice(0, 16).replace('T', ' ') + ' EST';
const started = new Date();
const run = (cmd, args, opts = {}) => { const r = spawnSync(cmd, args, { encoding: 'utf8', maxBuffer: 1 << 28, ...opts }); if (r.status !== 0 && !opts.soft) throw new Error(`${cmd} ${args.join(' ')} failed: ${r.stderr}`); return r.stdout; };
const git = (...a) => run('git', ['-C', REPO, ...a]);
const LINE_AT = refresh ? PREV.read.lineTip : LINE, MAIN_AT = refresh ? PREV.read.mainTip : MAIN; // the refs the record texts are read at
const fm = (t, k) => (t.match(new RegExp('^' + k + ':\\s*(.+?)\\s*(#.*)?$', 'm')) || [])[1] || null;
const unq = s => s == null ? null : s.replace(/^"(.*)"$/, '$1');

// 1. task records on the line and on main
function readTasks(ref) {
  const out = {};
  for (const f of git('ls-tree', '--name-only', ref, 'project-management/tasks/').split('\n').filter(x => /TSK-\d+\.md$/.test(x))) {
    const id = path.basename(f, '.md'); const t = git('show', `${ref}:${f}`);
    const outcome = (t.match(/\n## Outcome\s*\n+([\s\S]*?)(?=\n## |\s*$(?![\s\S]))/) || [])[1];
    // the record's Context is its ## Description: paragraphs, lists, tables and subheadings kept, fenced figures left out because a disclosure draws no figure
    const description = (t.match(/\n## Description\s*\n+([\s\S]*?)(?=\n## |\s*$(?![\s\S]))/) || [])[1];
    const context = description ? blocks(description) : [];
    const serves = [...t.matchAll(/serves (EPC-\d+) (AC-\d+)/g)].map(m => m[1] + ' ' + m[2]);
    out[id] = { id, epic: fm(t, 'epic_id'), status: fm(t, 'status'), work_type: fm(t, 'work_type'), title: unq(fm(t, 'title')), created: fm(t, 'created'),
      depends_on: (fm(t, 'depends_on') || '[]').replace(/[\[\]\s]/g, '').split(',').filter(Boolean).filter(x => /^TSK-\d+$/.test(x)),
      about: outcome ? outcome.replace(/\s+/g, ' ').trim() : 'not recorded',   // the whole Outcome section, never cut
      context, criteria: (t.match(/^- AC-\d+/gm) || []).length, serves: [...new Set(serves)].slice(0, 6) };
  }
  return out;
}
const line = readTasks(LINE_AT), main = readTasks(MAIN_AT);

// 2. epics: outcome line, criteria count, status, and the section that introduced each task (TSK-1095 AC-15 themes)
const epics = {};
for (const e of EPICS) {
  const t = git('show', `${LINE_AT}:project-management/epics/${e}.md`);
  const secs = []; let cur = null;
  for (const ln of t.split('\n')) { const h = ln.match(/^## (.+)$/); if (h) { cur = { title: h[1].trim(), ids: new Set() }; secs.push(cur); continue; } if (!cur) continue; for (const m of ln.matchAll(/TSK-\d+/g)) cur.ids.add(m[0]); }
  // TSK-1095 AC-15: a theme is the record section that introduced the task, which in this repository's epics is an
  // amendment heading (or the original Planning notes); the Summary, criteria and graph sections mention tasks but
  // do not introduce them, so they do not group.
  const themeOf = {}; for (const s of secs) if (/amendment|planning note/i.test(s.title)) for (const id of s.ids) if (!(id in themeOf)) themeOf[id] = /^planning notes?$/i.test(s.title) ? 'Original plan (Planning notes)' : s.title;
  const outcome = (t.match(/^## Outcome\s*\n+([\s\S]*?)(?=\n## )/m) || [])[1];
  const valueLine = (t.match(/Value line:\s*([\s\S]*?)(?=\n\n)/) || [])[1];
  const doneMeans = (t.match(/Done means\s*([\s\S]*?)(?=\n\n|$)/) || [])[1];
  // the epic's own context, in its words: the Summary's first paragraph (what it is), the "It serves" sentence (for whom)
  const summary = (t.match(/^## Summary\s*\n+([\s\S]*?)(?=\n## )/m) || [])[1] || '';
  const paras = summary.split(/\n\s*\n/).map(x => x.replace(/\s+/g, ' ').trim()).filter(Boolean);
  const servesSentence = (summary.replace(/\s+/g, ' ').match(/It serves [^.]*\./) || [])[0] || null;
  epics[e] = { id: e, title: unq(fm(t, 'title')), status: fm(t, 'status'), created: fm(t, 'created'), criteria: (t.match(/^- AC-\d+/gm) || []).length,
    capabilities: (fm(t, 'capabilities') || '').replace(/[\[\]\s]/g, '').split(',').filter(Boolean),
    deliverable: (outcome || valueLine || doneMeans || 'not recorded').replace(/\s+/g, ' ').trim(),
    deliverableSource: outcome ? '## Outcome' : valueLine ? 'Summary, value line' : doneMeans ? 'Summary, done means' : 'not recorded',
    whatItIs: paras[0] || 'not recorded', serves: servesSentence || 'not recorded', doneMeans: doneMeans ? doneMeans.replace(/\s+/g, ' ').trim() : 'not recorded',
    themeOf, sections: secs.map(s => s.title) };
}

// 2b. capability names, from the registry table in docs/capabilities.md at the same ref (so a CAP id can be shown in words)
const capabilities = {};
for (const m of git('show', `${LINE_AT}:docs/capabilities.md`).matchAll(/^\| (CAP-\d+) \| ([a-z0-9-]+) \|/gm)) capabilities[m[1]] = m[2].replace(/-/g, ' ');

// 2b2. each capability's entry for its feature view: the yaml fence's facts and the prose after the fence (tables left out)
const capabilityEntries = {};
for (const m of git('show', `${LINE_AT}:docs/capabilities.md`).matchAll(/```yaml\n([\s\S]*?)```\n([\s\S]*?)(?=\n#{2,4} |\s*$(?![\s\S]))/g)) {
  const y = k => (m[1].match(new RegExp(`^${k}: (.*)$`, 'm')) || [])[1] || '';
  if (!/^CAP-\d+$/.test(y('id'))) continue;
  const about = blocks(m[2].replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')).filter(b => !b.table);
  capabilityEntries[y('id')] = { name: y('name').replace(/-/g, ' '), area: y('area'), status: y('status'), epics: y('epics').replace(/[\[\]\s]/g, '').split(',').filter(Boolean), about };
}

// 2c. the docs pages that changed between main and the line (the portal's "What changed" page): one numstat, read only
const docsKind = {}; for (const l of git('diff', '--name-status', MAIN_AT, LINE_AT, '--', 'docs/').split('\n').filter(Boolean)) { const [k, f] = l.split('\t'); docsKind[f] = k === 'A' ? 'new' : k === 'D' ? 'removed' : 'changed'; }
const docsChanged = git('diff', '--numstat', MAIN_AT, LINE_AT, '--', 'docs/').split('\n').filter(Boolean).map(l => { const [a, d, f] = l.split('\t'); return { file: f, added: +a || 0, removed: +d || 0, kind: docsKind[f] || 'changed' }; });

// 3. last change of each task record on the line (one git log pass)
const lastMoved = {}; let when = null;
for (const ln of git('log', '--format=%x01%ci', '--name-only', LINE, '--', 'project-management/tasks/').split('\n')) {
  if (ln.startsWith('\x01')) { when = ln.slice(1); continue; } const m = ln.match(/tasks\/(TSK-\d+)\.md$/); if (m && !(m[1] in lastMoved)) lastMoved[m[1]] = when;
}

if (refresh) {
  const at = est(new Date());
  for (const [id, t] of Object.entries(PREV.tasks)) if (line[id]) { t.about = line[id].about; t.context = line[id].context; t.criteria = line[id].criteria; t.serves = line[id].serves; }
  for (const e of EPICS) if (PREV.epics[e]) Object.assign(PREV.epics[e], { whatItIs: epics[e].whatItIs, serves: epics[e].serves, doneMeans: epics[e].doneMeans, deliverable: epics[e].deliverable, deliverableSource: epics[e].deliverableSource });
  PREV.capabilities = capabilities; PREV.capabilityEntries = capabilityEntries; PREV.docsChanged = docsChanged; PREV.read.recordsRefreshedAt = at;
  fs.writeFileSync(path.join(HERE, 'facts.json'), JSON.stringify(PREV, null, 1));
  console.log(`refreshed the record texts at ${PREV.read.lineTip} (main ${PREV.read.mainTip}) at ${at}: ${Object.keys(PREV.tasks).length} tasks, ${EPICS.length} epics, ${Object.keys(capabilities).length} capability names, ${docsChanged.length} docs files changed`);
  process.exit(0);
}

// 4. GitHub, read only
let gh = { read: false };
if (useGh) {
  const open = JSON.parse(run('gh', ['pr', 'list', '-R', 'sathyassn/codeflow', '--state', 'open', '--base', LINE.replace('origin/', ''), '--json', 'number,isDraft,mergeable,title,headRefName,updatedAt', '--limit', '100']));
  const openToMain = JSON.parse(run('gh', ['pr', 'list', '-R', 'sathyassn/codeflow', '--state', 'open', '--base', 'main', '--json', 'number,isDraft,title,headRefName', '--limit', '100']));
  const mergedToday = JSON.parse(run('gh', ['pr', 'list', '-R', 'sathyassn/codeflow', '--state', 'merged', '--search', `merged:>=${est(started).slice(0, 10)}`, '--json', 'number,title,baseRefName,mergedAt,headRefName', '--limit', '100']));
  const cmp = JSON.parse(run('gh', ['api', `repos/sathyassn/codeflow/compare/main...${LINE.replace('origin/', '')}`, '--jq', '{ahead_by,behind_by}']));
  gh = { read: true, at: est(new Date()), open, openToMain, mergedToday, ahead: cmp.ahead_by, behind: cmp.behind_by };
}

// 5. codeflow work next from this worktree (ready, waiting and blocked tasks with reasons)
let next = { read: false };
if (useNext) {
  const out = run('codeflow', ['work', 'next'], { cwd: REPO, soft: true, timeout: 600000 }) || '';
  const rows = []; let last = null;
  for (const ln of out.split('\n')) { const m = ln.match(/^(ready|waiting|blocked)\s+(TSK-\d+)\s+(.*?)\s+\((\S+) -> (\S+)\)\s*$/); if (m) { last = { state: m[1], id: m[2], title: m[3], epic: m[4], target: m[5], reason: '' }; rows.push(last); } else if (last && /^\s+\S/.test(ln)) last.reason += (last.reason ? ' ' : '') + ln.trim(); }
  next = { read: rows.length > 0, at: est(new Date()), rows, head: out.split('\n')[0].slice(0, 300) };
}

// 6. derive the scope
const scope = { total: 0, complete: 0, onMain: 0, inPr: 0, todo: 0, blocked: 0, cancelled: 0 };
const prByTask = {}; for (const p of (gh.open || [])) { const m = p.headRefName.match(/TSK-\d+/); if (m) (prByTask[m[0]] ||= []).push(p.number); }
const tasks = {};
for (const e of EPICS) {
  const ids = Object.keys(line).filter(i => line[i].epic === e);
  const ep = epics[e]; ep.tasks = ids.sort((a, b) => +a.slice(4) - +b.slice(4)); ep.counts = { total: 0, complete: 0, onMain: 0, inPr: 0, todo: 0, blocked: 0, cancelled: 0 };
  for (const i of ids) {
    const t = line[i]; const onMain = t.status === 'complete' && main[i] && main[i].status === 'complete';
    const prs = prByTask[i] || []; const inPr = t.status === 'todo' && prs.length > 0;
    const location = onMain ? 'main' : t.status === 'complete' ? 'line' : inPr ? 'pull request' : t.status === 'cancelled' ? 'cancelled' : (t.status || 'todo').replace(/_/g, ' ') + ', no open PR';
    const theme = ep.themeOf[i] || 'Original scope (no introducing section)';
    tasks[i] = { ...t, onMain, prs, location, theme, lastMoved: lastMoved[i] ? est(lastMoved[i]) : 'not recorded', next: null };
    const c = ep.counts; if (t.status === 'cancelled') { c.cancelled++; continue; } c.total++;
    if (t.status === 'complete') { c.complete++; if (onMain) c.onMain++; } else if (t.status === 'blocked') c.blocked++; else if (inPr) c.inPr++; else c.todo++;
  }
  for (const k of Object.keys(scope)) scope[k] += ep.counts[k];
  ep.themes = {}; for (const i of ep.tasks) if (tasks[i].status !== 'cancelled') (ep.themes[tasks[i].theme] ||= []).push(i);
}
for (const r of (next.rows || [])) if (tasks[r.id]) tasks[r.id].next = { state: r.state, reason: r.reason };

const facts = {
  read: { started: est(started), finished: est(new Date()), lineRef: LINE, lineTip: git('rev-parse', '--short=9', LINE).trim(), mainTip: git('rev-parse', '--short=9', MAIN).trim(), repo: REPO, codeflow: run('codeflow', ['--version'], { soft: true }).trim().split(' ').slice(0, 2).join(' ') },
  scope, epics, tasks, gh, next: { read: next.read, at: next.at, head: next.head, counts: next.rows ? next.rows.reduce((a, r) => (a[r.state] = (a[r.state] || 0) + 1, a), {}) : {} },
  capabilities, capabilityEntries, docsChanged,
};
fs.writeFileSync(path.join(HERE, 'facts.json'), JSON.stringify(facts, null, 1));
console.log(JSON.stringify({ started: facts.read.started, finished: facts.read.finished, lineTip: facts.read.lineTip, mainTip: facts.read.mainTip, scope, epics: Object.fromEntries(EPICS.map(e => [e, { ...epics[e].counts, themes: Object.keys(epics[e].themes).length }])), gh: gh.read ? { open: gh.open.length, mergedToday: gh.mergedToday.length, ahead: gh.ahead, behind: gh.behind } : 'not read', next: facts.next.counts }));
