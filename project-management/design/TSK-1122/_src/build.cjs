#!/usr/bin/env node
// TSK-1122 prototype builder, round 3 (fresh): one self-contained page from facts.json (the numbers and record texts),
// report.json (hand facts with sources, short phrases) and the previous report (change marks). Usage: node build.cjs
// The operator's rules (2026-11-02 17:34 EST): bite-sized bullets, one point each; a figure is the main view of any
// flow, comparison or status; no text block over two lines at desktop width outside collapsed detail; every number
// has its unit; names before ids; times are wall clock with a zone; sources go in collapsed detail.
const fs = require('fs'); const path = require('path');
const { STAGE_LINE_FILL } = require('./stage-colours.cjs');
const { blockText } = require('./record-blocks.cjs');
const HERE = __dirname, OUT = path.resolve(HERE, '../prototype.html');
const F = JSON.parse(fs.readFileSync(path.join(HERE, 'facts.json'), 'utf8'));
const R = JSON.parse(fs.readFileSync(path.join(HERE, 'report.json'), 'utf8'));
const P = JSON.parse(fs.readFileSync(path.join(HERE, 'previous-2026-11-01-1431.json'), 'utf8'));
const kit = n => fs.readFileSync(path.join(HERE, 'kit', n), 'utf8');

// ---- helpers ------------------------------------------------------------------------------------------------
const esc = s => String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const MON = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const when = s => { const m = String(s || '').match(/^(\d{4})-(\d\d)-(\d\d)[ T](\d\d:\d\d)(?::\d\d)?\s*([A-Z]{2,4})?$/); return m ? `${m[4]} ${m[5] || 'EST'}, ${+m[3]} ${MON[+m[2] - 1]}${m[1] === '2026' ? '' : ' ' + m[1]}` : String(s || ''); };
const day = s => { const m = String(s || '').match(/^(\d{4})-(\d\d)-(\d\d)/); return m ? `${+m[3]} ${MON[+m[2] - 1]}${m[1] === '2026' ? '' : ' ' + m[1]}` : String(s || ''); };
const first = s => { const t = String(s || '').replace(/\s+/g, ' ').trim(); const m = t.match(/^.*?[.!?](?=\s+[A-Z(`"]|$)/); return m ? m[0] : t; };
// a lead short enough for two lines at desktop width: the first sentence, or its first clause when the sentence runs long
const lead = s => { const f = first(s); if (f.length <= 150) return f; let cut = -1; for (const m of f.matchAll(/[,;:] /g)) { if (m.index > 50 && m.index <= 140) cut = m.index; } if (cut < 0) { const sp = f.lastIndexOf(' ', 140); cut = sp > 50 ? sp : 140; } return f.slice(0, cut).replace(/[,;:]$/, '') + '.'; };
const md = s => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>');
const pct1 = (a, b) => b ? (Math.round(1000 * a / b) / 10).toFixed(1) : '0.0';
const S = F.scope, overall = pct1(S.complete, S.total), EP = Object.values(F.epics), T = F.tasks, CAPS = F.capabilities || {}, CE = F.capabilityEntries || {};
const READ_AT = when(F.read.finished), PREV_W = when(P.snapshotAt), TRACK_AT = when(R.readAt), BLOCK_AT = when(R.blockersEvidenceAt || R.readAt);
const nameOf = id => R.names[id] || (T[id] && T[id].title) || (F.epics[id] && F.epics[id].title) || CAPS[id] || null;
const idTag = id => `<span class="id">${esc(id)}</span>`;
const named = text => String(text || '').replace(/\b((?:TSK|EPC|CAP|SPC)-\d+)\b/g, (m, id, at, t) => { const before = t.slice(0, at); const open = (before.match(/\(/g) || []).length > (before.match(/\)/g) || []).length; const n = nameOf(id); if (!n || open || /\b(?:the|a|an)\s+\S+(?:\s+\S+){0,6}\s*\($/.test(before + '(')) return m; return `${n} (${id})`; });
const mdn = s => md(named(s));
const link = (id, label) => `<a href="#${id}" data-view-link="${id}">${label}</a>`;
const taskName = id => T[id] ? `${link('k-' + id, esc(T[id].title))} ${idTag(id)}` : `${esc(nameOf(id) || 'a task not on the line')} ${idTag(id)}`;
const crumbs = parts => `<nav class="crumbs" aria-label="Breadcrumb">${parts.map((p, i) => i < parts.length - 1 ? `${link(p[0], esc(p[1]))}<span class="sep" aria-hidden="true">/</span>` : `<span aria-current="page">${esc(p[1])}</span>`).join('')}</nav>`;
const chip = (text, cls = 'plain') => `<span class="chip ${cls}">${text}</span>`;
const facts = (rows, label = 'Context') => `<dl class="facts" aria-label="${esc(label)}">${rows.map(([k, v]) => `<div${/wide-cell/.test(v) ? ' class="wide"' : ''}><dt>${esc(k)}</dt><dd>${v}</dd></div>`).join('')}</dl>`;
const details = (summary, body, cls = '') => `<details class="more ${cls}"><summary>${esc(summary)}</summary><div class="more-body">${body}</div></details>`;
const points = (items, cls = '') => `<ul class="points ${cls}">${items.map(i => `<li${typeof i === 'object' ? ` class="${i.cls || ''}"` : ''}><span>${typeof i === 'object' ? i.html : i}</span></li>`).join('')}</ul>`;
const figA11y = (id, title, desc) => `role="img" aria-labelledby="${id}-t" aria-describedby="${id}-d"><title id="${id}-t">${esc(title)}</title><desc id="${id}-d">${esc(desc)}</desc>`;
const dataTable = (head, rows, label) => `<details class="fig-data"><summary>The figure as a table</summary><table aria-label="${esc(label)}"><thead><tr>${head.map(h => `<th scope="col">${esc(h)}</th>`).join('')}</tr></thead><tbody>${rows.map(r => `<tr>${r.map((c, i) => i ? `<td data-l="${esc(head[i])}">${c}</td>` : `<th scope="row" data-l="${esc(head[0])}">${c}</th>`).join('')}</tr>`).join('')}</tbody></table></details>`;
let FIGN = 0;
const view = () => { FIGN = 0; return ''; };
const fig = (block, title, inner, caption, table, numbered = true) => { const n = numbered ? ++FIGN : null; return `<figure class="fig" data-block="${esc(block)}"><div class="fig-head"><span class="fig-t">${esc(title)}</span>${n ? `<span class="fig-n">Figure ${n}</span>` : ''}</div>${inner}${caption ? `<figcaption>${caption}</figcaption>` : ''}${table || ''}</figure>`; };
const vt = o => { const h = o.head || []; return `<div class="vt ${o.cls || ''}" role="table"${o.label ? ` aria-label="${esc(o.label)}"` : ''} style="--vt-cols:${o.cols}">${h.length ? `<div class="vt-head" role="row">${h.map(x => `<span role="columnheader">${esc(x)}</span>`).join('')}</div>` : ''}${o.rows.map(r => `<div role="row"${r.attrs || ''}>${r.cells.map((c, i) => `<span role="cell" data-l="${esc(h[i] || '')}">${c}</span>`).join('')}</div>`).join('')}</div>`; };
const tabs = (id, list, label = 'Aspects') => `<div class="tabs" role="tablist" aria-label="${esc(label)}">${list.map(([k, l, n], i) => `<button class="tab" role="tab" id="${id}-tab-${k}" aria-controls="${id}-${k}" aria-selected="${i === 0}" tabindex="${i === 0 ? 0 : -1}" type="button">${esc(l)}${n != null ? ` <span class="count">${n}</span>` : ''}</button>`).join('')}</div>${list.map(([k, , , body], i) => `<div class="tabpanel" role="tabpanel" id="${id}-${k}" aria-labelledby="${id}-tab-${k}"${i ? ' hidden' : ''}>${body}</div>`).join('')}`;

// ---- marks and small figures ------------------------------------------------------------------------------
const STAGES = c => [['on main', c.onMain, 'st-main'], ['complete on the line only', c.complete - c.onMain, 'st-line'], ['in a pull request', c.inPr, 'st-pr'], ['todo, no open pull request', c.todo, 'st-todo'], ['blocked', c.blocked, 'st-block']];
const partition = (c, label) => { const tot = c.total || 1; const seg = STAGES(c).filter(s => s[1] > 0); return `<div class="part" role="img" aria-label="${esc(label)}: ${seg.map(s => `${s[1]} ${s[0]}`).join(', ')}">${seg.map(s => `<i class="${s[2]}" style="flex:${s[1]}"></i>`).join('')}</div><ul class="key">${STAGES(c).map(s => `<li><i class="sw ${s[2]}"></i><b>${s[1]}</b> ${esc(s[0])} <span class="mute">(${pct1(s[1], tot)}%)</span></li>`).join('')}</ul>`; };
const miniBar = c => { const tot = c.total || 1; return `<div class="part mini" role="img" aria-label="${c.complete} of ${c.total} tasks complete, ${c.onMain} on main, ${c.inPr} in a pull request">${STAGES(c).filter(s => s[1] > 0).map(s => `<i class="${s[2]}" style="flex:${s[1]}"></i>`).join('')}</div>`; };
const stMark = t => t.status === 'complete' ? (t.onMain ? '<i class="sm st-main" title="on main"></i>' : '<i class="sm st-line" title="complete on the line only"></i>') : t.status === 'blocked' ? '<i class="sm st-block" title="blocked"></i>' : t.location === 'pull request' ? '<i class="sm st-pr" title="in a pull request"></i>' : '<i class="sm st-todo" title="todo, no open pull request"></i>';
const locChip = t => chip(`${esc(t.location)}${t.prs.length ? ' ' + t.prs.map(n => '#' + n).join(' ') : ''}`, t.onMain ? 'plain' : t.status === 'complete' ? 'accent' : t.location === 'pull request' ? 'warn' : t.status === 'blocked' ? 'danger' : 'plain');
const stepDots = (n, of) => `<span class="steps" role="img" aria-label="step ${n} of ${of}">${Array.from({ length: of }, (_, i) => `<i class="${i + 1 < n ? 'done' : i + 1 === n ? 'now' : ''}"></i>`).join('')}</span>`;
const delta = (now, prev, unit = 'tasks') => { if (prev == null || now === prev) return ''; const d = Math.round((now - prev) * 10) / 10; return `<span class="delta ${d > 0 ? 'up' : 'down'}" title="was ${prev} ${esc(unit)} at ${esc(PREV_W)}">${d > 0 ? '+' : '\u2212'}${Math.abs(d)} ${esc(unit)}</span>`; };
const pl = (n, w, ws) => `${n} ${n === 1 ? w : (ws || w + 's')}`;
const cap = s => { s = String(s || ''); return s.charAt(0).toUpperCase() + s.slice(1); };
const epicState = e => e.status === 'in_progress' ? 'in progress' : e.status;
const readyIn = e => e.tasks.filter(i => T[i].next && T[i].next.state === 'ready').length;
const waitingIn = e => e.tasks.filter(i => T[i].next && T[i].next.state === 'waiting').length;
const taskRow = t => ({ cells: [`<span class="tc">${stMark(t)}<span class="tt">${taskName(t.id)}</span></span>`, locChip(t)] });
const taskTable = (ids, label = 'Tasks') => vt({ cls: 'tasks', cols: 'minmax(0,1fr) auto', head: ['Task', 'Where it is'], label, rows: ids.map(i => taskRow(T[i])) });
const themeKey = (eid, name) => `t-${eid}-${Object.keys(F.epics[eid].themes).indexOf(name)}`;
// the stage path of one task: todo, pull request, the line, main; the current stage filled
const taskPath = t => { const st = t.status === 'complete' ? (t.onMain ? 3 : 2) : t.location === 'pull request' ? 1 : 0; const names = ['todo', 'pull request', 'on the line', 'on main']; return `<svg class="path" viewBox="0 0 420 44" ${figA11y('tp-' + t.id, `Where ${t.title} is: ${t.status === 'blocked' ? 'blocked, ' : ''}${names[st]}`, `Four stages from todo to main; the task is at ${names[st]}.`)}<path class="ink2" d="M20 16H400"/>${names.map((n, i) => `<circle class="${i < st ? 'st-line-f' : i === st ? (t.status === 'blocked' ? 'st-block-f' : 'st-now') : 'st-todo-f'}" cx="${20 + i * 126.6}" cy="16" r="${i === st ? 8 : 6}"/><text x="${20 + i * 126.6}" y="40" text-anchor="${i === 0 ? 'start' : i === 3 ? 'end' : 'middle'}" class="${i === st ? 'b' : 's'}">${n}</text>`).join('')}</svg>`; };

// ---- change marks ------------------------------------------------------------------------------------------
const prevEpic = id => P.epics.find(e => e.id === id) || {};
const changes = [
  { what: 'tasks in scope', unit: 'tasks', prev: P.scope.total, now: S.total, view: 'overview' },
  { what: 'tasks complete', unit: 'tasks', prev: P.scope.complete, now: S.complete, view: 'overview' },
  { what: 'complete on main', unit: 'tasks', prev: P.scope.onMain, now: S.onMain, view: 'overview' },
  { what: 'tasks in a pull request', unit: 'tasks', prev: P.scope.inPr, now: S.inPr, view: 'overview' },
  { what: 'pull requests open against the line', unit: 'pull requests', prev: P.prs.openAgainstLine, now: F.gh.open.length, view: 'overview' },
  { what: 'commits the line is ahead of main', unit: 'commits', prev: P.aheadOfMain, now: F.gh.ahead, view: 'overview' },
  ...EP.flatMap(e => [{ what: `tasks of ${e.title}`, unit: 'tasks', prev: prevEpic(e.id).total, now: e.counts.total, view: 'o-' + e.id }, { what: `complete in ${e.title}`, unit: 'tasks', prev: prevEpic(e.id).done, now: e.counts.complete, view: 'o-' + e.id }]),
];
const changed = changes.filter(c => c.prev !== c.now);
let prevComplete = new Set(); try { const lc = JSON.parse(fs.readFileSync(path.join(HERE, 'previous-live-counts-2026-11-01-1431.json'), 'utf8')); for (const e of Object.values(lc.epics)) for (const i of e.completeIds) prevComplete.add(i); } catch {}
const newlyComplete = Object.keys(T).filter(i => T[i].status === 'complete' && !prevComplete.has(i)).sort();
const viewsChanged = new Set(changed.map(c => c.view));
const prevPct = pct1(P.scope.complete, P.scope.total), ptsDelta = Math.round((overall - prevPct) * 10) / 10;

// ---- overview ----------------------------------------------------------------------------------------------
const E22 = F.epics['EPC-022'], ES = R.epicShort;
const openPrs = F.gh.open, conflicting = openPrs.filter(p => p.mergeable === 'CONFLICTING').length, mergeable = openPrs.filter(p => p.mergeable === 'MERGEABLE').length, unknown = openPrs.filter(p => p.mergeable === 'UNKNOWN').length;
const streamsFig = `<div class="streams" role="img" aria-label="Four streams and their step">${R.streams.map(st => `<div class="stream"><span class="st-name">${esc(st.title)}</span>${stepDots(st.step, st.steps)}<span class="st-n">step ${st.step} of ${st.steps}</span><span class="st-owner">${esc(st.owner)}</span></div>`).join('')}</div>`;
const overview = `${view()}<section class="view is-active" id="overview" data-title="Overview">
  <div class="hero" data-block="Where the release stands">
    <div class="num">${overall}<small>%</small> ${ptsDelta ? `<span class="delta ${ptsDelta > 0 ? 'up' : 'down'}" title="was ${prevPct}% at ${esc(PREV_W)}">${ptsDelta > 0 ? '+' : '\u2212'}${Math.abs(ptsDelta)} pts</span>` : ''}</div>
    <div class="hero-side"><span><b>${S.complete} of ${S.total} tasks complete</b>${delta(S.complete, P.scope.complete)}</span><span class="mute">three epics on the line, cancelled tasks excluded</span></div>
  </div>
  <p class="one">${esc(ES['EPC-022'].oneLine)}</p>
  ${facts([['Delivers', esc(ES['EPC-022'].delivers)], ['For whom', esc(ES['EPC-022'].forWhom)], ['Why now', esc(ES['EPC-022'].whyNow)], ['Done means', 'one pull request from the line into main, ready for the operator']], 'The release in four facts')}
  ${fig('Where the tasks are', `Where the ${S.total} tasks are`, partition(S, `${S.total} tasks by stage`), `${S.onMain} tasks are on main; ${S.complete - S.onMain} more are complete on the line only.`, dataTable(['Stage', 'Tasks', 'Share'], STAGES(S).map(s => [esc(s[0]), `${s[1]} tasks`, pct1(s[1], S.total) + '%']), 'Where the tasks are, as a table'))}
  <h2 class="section">Three outcomes</h2>
  <div class="grid grid-3 outcomes">${EP.map(e => `<a class="card outcome" href="#o-${e.id}" data-view-link="o-${e.id}" data-block="Outcome: ${esc(e.title)}">
      <span class="t">${esc(cap(e.title))}</span> ${idTag(e.id)}
      <span class="p">${pct1(e.counts.complete, e.counts.total)}<small>%</small>${delta(pct1(e.counts.complete, e.counts.total) * 1, prevEpic(e.id).total ? pct1(prevEpic(e.id).done, prevEpic(e.id).total) * 1 : null, 'pts')}</span>
      ${miniBar(e.counts)}
      <span class="nums"><span><b>${e.counts.complete}</b> of <b>${e.counts.total}</b> tasks done</span><span><b>${readyIn(e)}</b> ready</span><span><b>${waitingIn(e)}</b> waiting</span></span>
      <span class="one">${esc(ES[e.id].card)}</span></a>`).join('')}</div>
  ${fig('Four streams', 'Four streams, each at its step', streamsFig, 'Each stream is one Coordinator\u2019s line of work; the filled dot is the step it is on.', dataTable(['Stream', 'Step', 'Owner', 'Now'], R.streams.map(s => [esc(s.title), `${s.step} of ${s.steps}`, esc(s.owner), esc(s.now[0])]), 'Streams as a table'))}
  <div class="grid grid-4 tiles">
    ${link('blockers', `<span class="card tile"><b>${R.blockers.length}</b> blockers<span class="mute">${R.blockers.filter(b => b.who === 'you').length} yours to clear</span></span>`)}
    ${link('o-EPC-022', `<span class="card tile"><b>${openPrs.length}</b> open pull requests<span class="mute">${mergeable} mergeable, ${conflicting} conflicting, ${unknown} not computed</span></span>`)}
    ${link('lanes', `<span class="card tile"><b>${R.lanes.length}</b> lane entries<span class="mute">${R.lanes.filter(l => l.stateShort === 'building').length} building now</span></span>`)}
    ${link('inputs', `<span class="card tile"><b>${R.inputs.length}</b> decisions asked<span class="mute">${R.actionsOwed.length} actions owed by you</span></span>`)}
  </div>
  <h2 class="section">What moved since ${esc(PREV_W)}</h2>
  ${points(changed.slice(0, 6).map(c => `${esc(c.what)}: ${c.prev} to <b>${c.now}</b> ${esc(c.unit)}`))}
  <p class="mute">${link('since', 'All changes')}, ${newlyComplete.length} tasks newly complete.</p>
  ${details('Basis, terms and sources', `${points([esc(R.basis.split('. ')[0]) + '.', 'On main: the task\u2019s own record on origin/main says complete.', 'In a pull request: an open pull request against the line names the task in its branch name.', `Read at ${esc(READ_AT)}: records at ${esc(F.read.lineTip)} (line) and ${esc(F.read.mainTip)} (main), GitHub read only, codeflow work next.`, `Trackers read at ${esc(TRACK_AT)}; blocker evidence at ${esc(BLOCK_AT)}.`, `Previous report: ${esc(PREV_W)}, line tip ${esc(P.lineTip)}.`])}<dl class="terms">${R.terms.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${mdn(v)}</dd></div>`).join('')}</dl>`)}
</section>`;

// ---- epic (topic) views, theme views -----------------------------------------------------------------------
const themeBars = e => { const rows = Object.entries(e.themes).map(([name, ids]) => ({ name, ids, done: ids.filter(i => T[i].status === 'complete').length, key: themeKey(e.id, name) })).sort((a, b) => b.ids.length - a.ids.length); const max = Math.max(1, ...rows.map(r => r.ids.length)); return { rows, html: `<div class="bars" role="group" aria-label="${rows.length} themes, tasks done over tasks in each; each bar opens its theme">${rows.map(r => `<div class="bar-row"><span class="bar-l">${link(r.key, esc(r.name))}</span><span class="bar-t"><i class="st-todo-bg" style="width:${(100 * r.ids.length / max).toFixed(1)}%"><i class="st-line" style="width:${(100 * r.done / r.ids.length).toFixed(1)}%"></i></i></span><span class="bar-n">${r.done} of ${r.ids.length}</span></div>`).join('')}</div>` }; };
const reasonCell = t => { const r = t.next && t.next.reason || ''; const m = r.match(/^task TSK-\d+ (?:waits on|depends on) (TSK-\d+)/); const short = m ? `waits on ${taskName(m[1])}` : r.length <= 90 ? esc(r.replace(/^task TSK-\d+ /, '')) : null; return short ? `<span class="mute">${short}</span>` : details('why it waits', `<p class="small">${esc(r)}</p>`, 'inline'); };
function topicView(e) {
  const id = 'o-' + e.id, c = e.counts, sh = ES[e.id], tb = themeBars(e);
  const ready = e.tasks.filter(i => T[i].next && T[i].next.state === 'ready'), waiting = e.tasks.filter(i => T[i].next && T[i].next.state === 'waiting'), blocked = e.tasks.filter(i => T[i].status === 'blocked' || (T[i].next && T[i].next.state === 'blocked'));
  const done = e.tasks.filter(i => T[i].status === 'complete'), open = e.tasks.filter(i => T[i].status !== 'complete' && !ready.includes(i) && !waiting.includes(i) && !blocked.includes(i));
  const prs = openPrs.filter(p => e.tasks.some(i => p.headRefName.includes(i)));
  view(); const progress = fig(`${e.title}: progress`, `${c.complete} of ${pl(c.total, 'task')} complete`, partition(c, `${pl(c.total, 'task')} of ${e.title} by stage`), `${pl(c.onMain, 'task')} on main, ${c.inPr} in a pull request, ${c.todo} todo.`, dataTable(['Stage', 'Tasks'], STAGES(c).map(s => [esc(s[0]), `${s[1]} tasks`]), 'Progress as a table'))
    + fig(`${e.title}: themes`, `${pl(tb.rows.length, 'theme')}, tasks done in each`, tb.html, 'A theme is the record section that introduced its tasks; each bar opens the theme.', dataTable(['Theme', 'Done', 'Tasks'], tb.rows.map(r => [link(r.key, esc(r.name)), `${r.done} tasks`, `${r.ids.length} tasks`]), 'Themes as a table'));
  const order = `${ready.length ? `<h3>${pl(ready.length, 'task')} ready to start</h3>${taskTable(ready, 'Ready tasks')}` : '<p class="mute">No task is ready.</p>'}${waiting.length ? `<h3 class="gap">${pl(waiting.length, 'task')} waiting on a predecessor</h3>${vt({ cls: 'tasks', cols: 'minmax(0,1fr) auto', head: ['Task and why it waits', 'Where it is'], label: 'Waiting tasks', rows: waiting.map(i => ({ cells: [`<span class="tc">${stMark(T[i])}<span class="tt">${taskName(i)}</span></span><span class="why">${reasonCell(T[i])}</span>`, locChip(T[i])] })) })}` : ''}${open.length ? `<h3 class="gap">${pl(open.length, 'task')} open and not listed by work next</h3>${taskTable(open, 'Open tasks not listed')}` : ''}${details(`${pl(done.length, 'complete task')}`, taskTable(done, 'Complete tasks'))}`;
  const issues = `${blocked.length ? `<h3>${pl(blocked.length, 'task')} blocked</h3>${taskTable(blocked, 'Blocked tasks')}` : '<p class="mute">No task of this epic is blocked on the line.</p>'}<h3 class="gap">${pl(prs.length, 'open pull request')}</h3>${prs.length ? vt({ cols: 'minmax(0,1fr) 7ch auto', head: ['Pull request', 'Number', 'State'], label: 'Open pull requests', rows: prs.map(p => ({ cells: [esc(p.title), `<span class="mono">#${p.number}</span>`, chip(`${p.isDraft ? 'draft, ' : ''}${p.mergeable === 'UNKNOWN' ? 'not computed' : p.mergeable.toLowerCase()}`, p.mergeable === 'MERGEABLE' ? 'ok' : p.mergeable === 'CONFLICTING' ? 'warn' : 'plain')] })) }) : '<p class="mute">None names a task of this epic.</p>'}`;
  return `${view()}<section class="view" id="${id}" data-title="${esc(cap(e.title))}">
  ${crumbs([['overview', 'Release'], [id, cap(e.title)]])}
  <div class="vhead"><h2>${esc(cap(e.title))}</h2>${idTag(e.id)}${chip(esc(epicState(e)), e.status === 'in_progress' ? 'accent' : 'plain')}</div>
  <p class="one">${esc(sh.oneLine)}</p>
  ${facts([['Delivers', esc(sh.delivers)], ['For whom', esc(sh.forWhom)], ['Why now', esc(sh.whyNow)]], `${e.title} in three facts`)}
  <div class="chips gap">${e.capabilities.map(cp => chip(`${CE[cp] ? link('f-' + cp, esc(CAPS[cp] || CE[cp].name)) : esc(CAPS[cp] || 'capability')} ${idTag(cp)}`)).join('')}${chip(`${e.criteria} epic criteria`)}${chip('verification not recorded on the line', 'warn')}</div>
  ${tabs(id, [['progress', 'Progress', null, progress], ['order', 'Order', ready.length + waiting.length, order], ['issues', 'Issues', blocked.length + prs.length, issues]])}
  ${details('Sources and the whole summary', `${points([`Short phrases: ${esc(sh.source)}.`, ...(sh.forWhomNote ? [`For whom: ${esc(sh.forWhomNote)}.`] : []), ...(sh.whyNowMore ? [`Why now, also: ${esc(sh.whyNowMore)}.`] : []), `Counts: the task records on the line at ${esc(F.read.lineTip)} and on main at ${esc(F.read.mainTip)}, read at ${esc(READ_AT)}.`, 'Ready and waiting: codeflow work next at ' + esc(when(F.next.at)) + '.'])}<h4>The record\u2019s summary, whole</h4>${e.whatItIs.split(/\n\n+/).map(p => `<p class="small">${md(p)}</p>`).join('')}${e.doneMeans !== 'not recorded' ? `<h4>Done means</h4><p class="small">${md(e.doneMeans)}</p>` : ''}`)}
</section>`;
}
function themeView(e, name, idx) {
  const id = `t-${e.id}-${idx}`, ids = e.themes[name], done = ids.filter(i => T[i].status === 'complete').length, c = { total: ids.length, complete: done, onMain: ids.filter(i => T[i].onMain).length, inPr: ids.filter(i => T[i].location === 'pull request').length, blocked: ids.filter(i => T[i].status === 'blocked').length };
  c.todo = c.total - c.complete - c.inPr - c.blocked;
  return `${view()}<section class="view" id="${id}" data-title="${esc(name)}">
  ${crumbs([['overview', 'Release'], ['o-' + e.id, cap(e.title)], [id, name]])}
  <div class="vhead"><h2>${esc(name)}</h2>${chip('theme')}</div>
  <p class="one">The tasks the section \u201c${esc(name)}\u201d of ${esc(e.title)} (${esc(e.id)}) introduced.</p>
  ${fig(`${name}: progress`, `${done} of ${pl(ids.length, 'task')} complete`, partition(c, `${pl(ids.length, 'task')} by stage`), '', dataTable(['Stage', 'Tasks'], STAGES(c).map(s => [esc(s[0]), `${s[1]} tasks`]), 'Theme progress as a table'), false)}
  <h3 class="gap">${pl(ids.length, 'task')}</h3>${taskTable(ids, 'Tasks of the theme')}
</section>`;
}
// "serves EPC-024 AC-4" lines grouped by epic: one bullet per epic, its criteria listed after the name
const servesLines = t => { const by = {}; for (const s of t.serves || []) { const m = String(s).match(/^(EPC-\d+)\s*(.*)$/); const k = m ? m[1] : s, v = m ? m[2] : ''; (by[k] ||= []).push(v); } return Object.entries(by).map(([k, v]) => `serves ${esc(nameOf(k) || k)} ${idTag(k)}${v.filter(Boolean).length ? ': ' + esc(v.filter(Boolean).join(', ')) : ''}`); };
function taskView(t) {
  const e = F.epics[t.epic], id = 'k-' + t.id, nr = '<span class="nr">not recorded</span>';
  const parts = [['overview', 'Release']]; if (e) { parts.push(['o-' + e.id, cap(e.title)]); parts.push([themeKey(e.id, t.theme), t.theme]); } parts.push([id, t.title]);
  const one = lead(t.about), whole = one.length < t.about.replace(/\s+/g, ' ').trim().length;
  return `${view()}<section class="view" id="${id}" data-title="${esc(t.title)}">
  ${crumbs(parts)}
  <div class="vhead"><h2>${esc(t.title)}</h2>${idTag(t.id)}${chip(esc(t.status), t.status === 'complete' ? 'ok' : t.status === 'blocked' ? 'danger' : 'plain')}</div>
  <p class="one">${esc(one)}</p>
  ${whole || (t.context || []).length ? details('The whole outcome and context, from the record', `<h4>Outcome</h4><p class="small">${esc(t.about)}</p>${(t.context || []).length ? `<h4>Context</h4>${recBlocks(t.context)}` : ''}`) : ''}
  ${fig(`${t.title}: where it is`, 'Where the task is on its way to main', taskPath(t), '', '', false)}
  ${facts([['Outcome', e ? `${link('o-' + e.id, esc(cap(e.title)))} ${idTag(e.id)}` : nr], ['Theme', e ? `<span class="wide-cell">${link(themeKey(e.id, t.theme), esc(t.theme))}</span>` : nr], ['Where it is', locChip(t)], ['State', esc(t.status)], ['Lane', nr], ['Window', nr], ['Record last changed', esc(when(t.lastMoved))], ['Pull request', t.prs.length ? t.prs.map(n => `<span class="mono">#${n}</span>`).join(' ') : 'none open']], 'Eight facts')}
  <div class="grid deps gap">
    <div class="card" data-block="${esc(t.title)}: criteria"><h4>Criteria</h4>${points([`<b>${t.criteria}</b> acceptance ${t.criteria === 1 ? 'criterion' : 'criteria'} in the record`, ...servesLines(t)])}</div>
    <div class="card" data-block="${esc(t.title)}: depends"><h4>Depends on</h4>${t.depends_on.length ? points(t.depends_on.map(d => ({ cls: T[d] && T[d].status === 'complete' ? 'ok' : 'todo', html: taskName(d) }))) : '<p class="mute">a root: no predecessor</p>'}</div>
  </div>
  ${details('Evidence', vt({ cols: '12ch minmax(0,1fr)', head: ['Fact', 'Source'], label: 'Evidence', rows: [['record', `<code>project-management/tasks/${t.id}.md</code> on the line at ${esc(F.read.lineTip)}`], ['read at', esc(READ_AT)], ['where it is', 'origin/main and the open pull requests, read only'], ['ready or waiting', t.next ? `codeflow work next at ${esc(when(F.next.at))}: ${esc(t.next.state)}` : 'not listed by codeflow work next']].map(r => ({ cells: [`<span class="mono">${r[0]}</span>`, `<span class="mute">${r[1]}</span>`] })) }))}
</section>`;
}
// Record prose carries Markdown marks; the page draws them as plain text.
const plain = s => s.replace(/\*\*|`/g, '');
// A record's blocks on the page: a list stays a list, a table a table, a subheading a subheading.
const recText = s => esc(plain(s));
const recBlock = b => {
  if (typeof b === 'string') return `<p class="small">${recText(b)}</p>`;
  if (b.heading) return `<h5 class="rec-h">${recText(b.heading)}</h5>`;
  if (b.table) return `<div class="rec-table" tabindex="0"><table><thead><tr>${b.table.head.map(c => `<th scope="col">${recText(c)}</th>`).join('')}</tr></thead><tbody>${b.table.rows.map(r => `<tr>${r.map(c => `<td>${recText(c)}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`;
  const tag = b.list, start = tag === 'ol' && b.start !== 1 ? ` start="${b.start}"` : '';
  return `<${tag} class="small rec-list"${start}>${b.items.map(it => `<li>${recText(it.text)}${it.sub.map(recBlock).join('')}</li>`).join('')}</${tag}>`;
};
const recBlocks = bs => bs.map(recBlock).join('');
// A feature is its capability entry (TSK-1098 AC-13): what it is, then its status, then the epics that build it; no empty group.
function featureView(id) {
  const c = CE[id], name = cap(CAPS[id] || c.name), about = plain(c.about.map(blockText).join(' ')) || 'not recorded', firstBlock = c.about[0], one = lead(plain(firstBlock ? (firstBlock.list ? firstBlock.items[0].text : blockText(firstBlock)) : about)), eps = c.epics.filter(x => F.epics[x]);
  return `${view()}<section class="view" id="f-${id}" data-title="${esc(name)}">
  ${crumbs([['overview', 'Release'], ['f-' + id, name]])}
  <div class="vhead"><h2>${esc(name)}</h2>${idTag(id)}${chip(esc(c.status || 'not recorded'), c.status === 'shipped' ? 'ok' : 'plain')}</div>
  <p class="one">${esc(one)}</p>
  ${one.length < about.length ? details('The whole entry, from the record', recBlocks(c.about)) : ''}
  <div class="gap">${facts([['State', esc(c.status || 'not recorded')], ['Area', esc(c.area || 'not recorded')]], 'Two facts')}</div>
  ${eps.length ? `<div class="card gap" data-block="${esc(name)}: epics"><h4>Built by</h4>${points(eps.map(x => `${link('o-' + x, esc(cap(F.epics[x].title)))} ${idTag(x)}`))}</div>` : ''}
  ${details('Evidence', `<p class="small">The entry for ${esc(id)} in <code>docs/capabilities.md</code> on the line at ${esc(F.read.lineTip)}, read at ${esc(READ_AT)}.</p>`)}
</section>`;
}
const featureViews = [...new Set(EP.flatMap(e => e.capabilities))].filter(id => CE[id]).map(featureView).join('\n');
const topicViews = EP.map(topicView).join('\n');
const themeViews = EP.flatMap(e => Object.keys(e.themes).map((n, i) => themeView(e, n, i))).join('\n');
const taskViews = Object.values(T).map(taskView).join('\n');

// ---- epics, blockers, since, lanes, evidence, inputs, script --------------------------------------------------
const epicsView = `${view()}<section class="view" id="epics" data-title="Epics">
  <h2>Three epics on the line</h2>
  <p class="one">What each delivers, for whom and why now; done, not done and the percent from each task\u2019s record.</p>
  <div class="grid epics">${EP.map(e => `<div class="card epic" data-block="${esc(e.title)}">
    <div class="head"><span class="p">${pct1(e.counts.complete, e.counts.total)}<small>%</small></span><span>${link('o-' + e.id, esc(cap(e.title)))} ${idTag(e.id)}<br><span class="mute">${esc(epicState(e))}, created ${esc(day(e.created))}</span></span></div>
    ${miniBar(e.counts)}
    <div class="nums"><span><b>${e.counts.complete}</b> done</span><span><b>${e.counts.onMain}</b> on main</span><span><b>${e.counts.total - e.counts.complete - e.counts.blocked}</b> not done</span><span><b>${e.counts.inPr}</b> in pull requests</span><span><b>${e.counts.total}</b> in all</span></div>
    ${facts([['Delivers', esc(ES[e.id].delivers)], ['For whom', esc(ES[e.id].forWhom)], ['Why now', esc(ES[e.id].whyNow)]], `${e.title} in three facts`)}
  </div>`).join('')}</div>
</section>`;

const BS = R.blockerShort;
const blockersView = `${view()}<section class="view" id="blockers" data-title="Blockers">
  <h2>${R.blockers.length} blockers</h2>
  <p class="one">Who clears each, what to do, why, and the alternative. Trackers read at ${esc(TRACK_AT)}; blocker evidence at ${esc(BLOCK_AT)}.</p>
  ${fig('Blockers by owner', 'Who clears each blocker', `<div class="owners" role="group" aria-label="${R.blockers.filter(b => b.who === 'you').length} blockers for you, ${R.blockers.filter(b => b.who !== 'you').length} for the primary">${['you', 'the primary'].map(w => `<div class="owner-row"><span class="bar-l">${esc(w)}</span><span class="dots">${R.blockers.filter(b => b.who === w).map(b => `<a href="#blk-${b.id}" class="dot st-block" title="${esc(b.title)}"><span class="visually-hidden">${esc(b.title)}</span></a>`).join('')}</span><span class="bar-n">${R.blockers.filter(b => b.who === w).length} blockers</span></div>`).join('')}</div>`, 'Each square is one blocker; the rows are who clears it.', '', false)}
  <div class="grid blks">${R.blockers.map(b => `<div class="card blk" id="blk-${b.id}" data-block="Blocker: ${esc(b.title)}">
    <h3>${mdn(b.title)}</h3>
    <div class="chips">${chip(esc(b.who === 'you' ? 'yours to clear' : 'the primary clears it'), b.who === 'you' ? 'danger' : 'warn')}${chip(esc(b.state))}</div>
    ${points([`<b>Do:</b> ${mdn(BS[b.id].do)}`, `<b>Why:</b> ${mdn(BS[b.id].why)}`, `<b>Else:</b> ${mdn(BS[b.id].alt)}`])}
    ${details('The full recommendation and its source', `<p class="small"><b>Recommended:</b> ${mdn(b.rec)}</p><p class="small"><b>Why:</b> ${mdn(b.why)}</p><p class="small"><b>Alternative:</b> ${mdn(b.alt)}</p><p class="small mute">Source: ${esc(b.source)}, read ${esc(BLOCK_AT)}.</p>`)}
  </div>`).join('')}</div>
</section>`;

const sinceView = `${view()}<section class="view" id="since" data-title="What changed">
  <h2>What changed since ${esc(PREV_W)}</h2>
  <p class="one">Previous report at ${esc(PREV_W)} (line tip ${esc(P.lineTip)}); this one at ${esc(READ_AT)} (line tip ${esc(F.read.lineTip)}).</p>
  ${fig('Changes since the previous report', `${changed.length} numbers moved`, `<div class="bars delta-bars" role="group" aria-label="${changed.map(c => `${c.what} ${c.prev} to ${c.now}`).join('; ')}">${changed.map(c => { const max = Math.max(c.prev, c.now, 1); return `<div class="bar-row"><span class="bar-l">${link(c.view, esc(c.what))}</span><span class="bar-t two"><i class="st-todo-bg" style="width:${(100 * c.prev / max).toFixed(1)}%"></i><i class="st-line" style="width:${(100 * c.now / max).toFixed(1)}%"></i></span><span class="bar-n">${c.prev} to <b>${c.now}</b> ${esc(c.unit)}</span></div>`; }).join('')}</div>`, 'Grey is the previous value, green the value now.', dataTable(['What', 'Then', 'Now'], changed.map(c => [link(c.view, esc(c.what)), `${c.prev} ${esc(c.unit)}`, `${c.now} ${esc(c.unit)}`]), 'Changes as a table'))}
  <h3 class="gap">${pl(newlyComplete.length, 'task')} complete now that were not at ${esc(PREV_W)}</h3>
  ${newlyComplete.length ? taskTable(newlyComplete, 'Newly complete tasks') : '<p class="mute">None.</p>'}
  <h3 class="gap">The previous report\u2019s blockers today</h3>
  ${vt({ cols: 'minmax(0,1fr) minmax(0,1fr)', head: [`Then (${PREV_W})`, 'Now'], label: 'Previous blockers today', rows: P.blockers.map(b => ({ cells: [esc(b.title), `<span class="mute">${esc(b.id === 'b2' ? `${conflicting} conflicting; ${unknown} of ${openPrs.length} not computed by GitHub` : b.id === 'b3' ? 'unchanged: the quality defaults line is still at e283948c6' : b.id === 'b4' ? 'unchanged: the hook binary differs from the line' : 'not rechecked today')}</span>`] })) })}
</section>`;

const LANE_STATES = ['building', 'fixing', 'preparing', 'waiting', 'idle'];
const coords = [...new Set(R.lanes.map(l => l.coordinator))];
const lanesView = `${view()}<section class="view" id="lanes" data-title="Lanes">
  <h2>${R.lanes.length} lane entries in the trackers</h2>
  <p class="one">By Coordinator, from the trackers at ${esc(TRACK_AT)}; a lane is one builder, seat or subagent on one unit.</p>
  ${fig('Lane board', 'Lanes by Coordinator and state', `<div class="board" role="group" aria-label="${coords.map(c => `${c}: ${R.lanes.filter(l => l.coordinator === c).length} lanes`).join('; ')}"><div class="board-head"><span></span>${LANE_STATES.map(s => `<span>${s}</span>`).join('')}</div>${coords.map(c => `<div class="board-row"><span class="bar-l">${esc(c)}</span>${LANE_STATES.map(s => `<span class="cell">${R.lanes.filter(l => l.coordinator === c && l.stateShort === s).map(l => `<a href="#lane-${R.lanes.indexOf(l)}" class="dot ${s === 'building' ? 'st-line' : s === 'fixing' ? 'st-block' : s === 'waiting' ? 'st-pr' : 'st-todo'}" title="${esc(l.lane)}"><span class="visually-hidden">${esc(l.lane)}</span></a>`).join('')}</span>`).join('')}</div>`).join('')}</div>`, 'Each dot is one lane; its column is the state the tracker gives it.', dataTable(['Coordinator', ...LANE_STATES], coords.map(c => [esc(c), ...LANE_STATES.map(s => String(R.lanes.filter(l => l.coordinator === c && l.stateShort === s).length) + ' lanes')]), 'Lane board as a table'))}
  ${coords.map(c => `<h3 class="gap">${esc(c)} Coordinator</h3>${vt({ cols: 'minmax(0,1.4fr) minmax(0,1fr) auto', head: ['Lane', 'Who', 'State'], label: `${c} lanes`, rows: R.lanes.filter(l => l.coordinator === c).map(l => ({ attrs: ` id="lane-${R.lanes.indexOf(l)}"`, cells: [`${mdn(l.lane)}${details('what and when', `<p class="small">${mdn(l.what)}</p><p class="small">${esc(l.state)}; expected ${esc(l.expected)}.</p><p class="small mute">Source: ${esc(l.source)}.</p>`, 'inline')}`, `<span class="mute">${esc(l.who)}</span>`, chip(esc(l.stateShort), l.stateShort === 'building' ? 'ok' : l.stateShort === 'fixing' ? 'danger' : l.stateShort === 'waiting' ? 'warn' : 'plain')] })) })}`).join('')}
</section>`;

const SOURCES = [['records on the line', `aa84d8544, ${READ_AT}`], ['records on main', `b2bb61af3, ${READ_AT}`], ['GitHub', `read only, ${when(F.gh.at)}`], ['work next', when(F.next.at)], ['trackers', TRACK_AT]];
const MEASURES = [['percent and counts', [1, 1, 0, 0, 0]], ['on main', [0, 1, 0, 0, 0]], ['in a pull request', [0, 0, 1, 0, 0]], ['ready and waiting', [0, 0, 0, 1, 0]], ['open pull requests', [0, 0, 1, 0, 0]], ['what it delivers, for whom, why now', [1, 0, 0, 0, 0]], ['blockers, lanes, decisions', [0, 0, 0, 0, 1]], ['lane and window per task', [0, 0, 0, 0, 0]]];
const evidenceView = `${view()}<section class="view" id="evidence" data-title="Evidence">
  <h2>Where each number was read</h2>
  <p class="one">One read from ${esc(when(F.read.started))} to ${esc(READ_AT)}; record texts re-read at ${esc(when(F.read.recordsRefreshedAt))}, no count changed.</p>
  ${fig('Coverage matrix', 'Which source each measure was read from', `<div class="matrix" role="img" aria-label="${MEASURES.map(m => `${m[0]}: ${m[1].some(x => x) ? SOURCES.filter((s, j) => m[1][j]).map(s => s[0]).join(' and ') : 'not recorded anywhere'}`).join('; ')}"><div class="board-head"><span></span>${SOURCES.map(s => `<span>${esc(s[0])}</span>`).join('')}</div>${MEASURES.map(m => `<div class="board-row"><span class="bar-l">${esc(m[0])}</span>${m[1].map(x => `<span class="cell">${x ? '<i class="dot st-line"></i>' : '<i class="dot none"></i>'}</span>`).join('')}</div>`).join('')}</div>`, 'A filled dot means the measure was read from that source; an empty row is a fact the records do not carry yet.', dataTable(['Measure', 'Read from'], MEASURES.map(m => [esc(m[0]), m[1].some(x => x) ? SOURCES.filter((s, j) => m[1][j]).map(s => esc(s[0])).join(', ') : 'not recorded']), 'Coverage as a table'))}
  <h3 class="gap">Sources and their read time</h3>
  ${vt({ cols: 'minmax(0,1fr) minmax(0,1fr)', head: ['Source', 'Read'], label: 'Sources', rows: SOURCES.map(s => ({ cells: [esc(s[0]), `<span class="mute">${esc(s[1])}</span>`] })) })}
  <h3 class="gap">Not verified</h3>
  ${points(['Lane and window per task: the fields do not exist on the line yet.', 'Epic verification: no epic record on the line carries a result.', 'GitHub mergeability: not computed for ' + unknown + ' of ' + openPrs.length + ' open pull requests at the read.', 'Tracker facts were not re-read after ' + esc(BLOCK_AT) + '.'])}
</section>`;

const ANSWERS_JSON = 'coordination/coordinators/visual-docs/published/status-prototype/answers.json';
const saveRow = id => `<div class="save-row"><button class="btn save" type="button" data-save="${id}">Save</button><span class="saved" id="saved-${id}" role="status" aria-live="polite"></span></div>`;
const inputsView = `${view()}<section class="view" id="inputs" data-title="Your decisions">
  <h2>${R.inputs.length} decisions for you</h2>
  <p class="one">Each asks one question; the recommended answer is marked, never preselected; the Script view writes exactly what you chose.</p>
  <form class="form" data-answers="tsk-1122-walkthrough" id="decisions" data-answers-file="${esc(ANSWERS_JSON)}">${R.inputs.map((q, i) => `<div class="card" data-block="Decision ${i + 1}" data-field="${q.id}" data-label="${esc(q.title)}"><div class="q">${i + 1}. ${esc(q.title)}</div><div class="opts">${q.options.map(o => `<label class="opt"><input type="radio" name="${q.id}" value="${o.value}" data-label="${esc(o.label)}"><span>${mdn(o.label)}</span>${o.recommended ? '<span class="rec">recommended</span>' : '<span></span>'}<span class="cost">cost: ${mdn(o.cost)}</span></label>`).join('')}</div>${saveRow(q.id)}${details('Context and why', `<p class="small">${mdn(q.context)}</p><p class="small"><b>Why:</b> ${mdn(q.reasoning)}</p>`)}</div>`).join('')}
    <div class="card" data-block="Changes named" data-field="changes" data-label="Changes you name"><label class="q" for="changes">Changes you name, if any</label><textarea id="changes" name="changes" rows="3" placeholder="One change per line"></textarea>${saveRow('changes')}</div>
  </form>
  ${points(['Save writes every answer on this page to <code>answers.json</code> in the repository; the confirmation names the path.', 'The first save asks for the repository folder once.', 'A draft of your typing stays in this browser until you save; a reload keeps it.', 'Where the browser cannot write files (Safari, Firefox), Save downloads <code>answers.json</code> and the confirmation says where to move it.'])}
  <h3 class="gap">Decided today</h3>
  ${vt({ cols: '16ch minmax(0,1fr)', head: ['When', 'Decided'], label: 'Decided today', rows: R.decided.map(d => ({ cells: [esc(d.when), mdn(d.what)] })) })}
  <h3 class="gap">${R.actionsOwed.length} actions owed by you</h3>
  ${vt({ cols: 'minmax(0,1fr) 20ch', head: ['Action', 'Since'], label: 'Actions owed', rows: R.actionsOwed.map(a => ({ cells: [mdn(a.what), `<span class="mute">${esc(a.since)}</span>`] })) })}
</section>`;

const scriptView = `${view()}<section class="view" id="script" data-title="Script">
  <h2>One script: recheck, then record your answers</h2>
  ${points(['Part 1 rechecks the numbers, read only, from any folder.', 'Part 2 appears only for answered decisions and writes one file on integration/workspace.'])}
  <div class="copy-wrap gap"><button class="btn" type="button" data-copy="#script-text">Copy</button><pre id="script-text" tabindex="0" aria-label="The script"></pre></div>
</section>`;

// ---- report kinds as navigable specimens ---------------------------------------------------------------------
const slug = s => s.toLowerCase().replace(/[^a-z0-9]+/g, '-');
const VIEW_KINDS = { overview: 'Overview', topic: 'Topic', item: 'Item', list: 'List', issues: 'Issues', decisions: 'Decisions', evidence: 'Evidence', commands: 'Commands', timeline: 'Timeline' };
const specFig = family => { const W = 600; let o = ''; switch (family) {
  case 'partition': o = `<rect class="st-main" x="10" y="20" width="200" height="30" rx="4"/><rect class="st-line" x="212" y="20" width="90" height="30" rx="4"/><rect class="st-pr" x="304" y="20" width="60" height="30" rx="4"/><rect class="st-todo" x="366" y="20" width="224" height="30" rx="4"/>`; break;
  case 'criteria': o = [0, 1, 2, 3, 4].map(i => `<circle class="${i < 3 ? 'st-line-f' : 'st-todo-f'}" cx="22" cy="${18 + i * 22}" r="7"/><rect class="node-soft" x="40" y="${11 + i * 22}" width="${320 - i * 30}" height="14" rx="3"/>`).join(''); break;
  case 'severity': o = [['st-block', 60], ['st-pr', 160], ['st-todo', 300]].map(([c, w], i) => `<rect class="${c}" x="10" y="${14 + i * 30}" width="${w}" height="20" rx="3"/>`).join(''); break;
  case 'timeline': o = `<path class="ink2" d="M20 50H580"/>` + [0, 1, 2, 3, 4, 5].map(i => `<circle class="${i < 4 ? 'st-line-f' : 'st-todo-f'}" cx="${40 + i * 104}" cy="50" r="7"/><rect class="node-soft" x="${20 + i * 104}" y="${i % 2 ? 66 : 14}" width="60" height="14" rx="3"/>`).join(''); break;
  case 'pair': o = `<rect class="node-soft" x="10" y="10" width="280" height="90" rx="8"/><rect class="acc-wash" x="310" y="10" width="280" height="90" rx="8"/><path class="ink2" d="M292 55h16"/>`; break;
  case 'structure': o = `<rect class="acc-wash" x="230" y="8" width="140" height="28" rx="6"/>` + [0, 1, 2].map(i => `<path class="ink2" d="M300 36V58H${90 + i * 210}V74"/><rect class="node-soft" x="${30 + i * 210}" y="74" width="120" height="26" rx="6"/>`).join(''); break;
  case 'owners': o = [0, 1, 2].map(i => `<rect class="node-soft" x="10" y="${12 + i * 32}" width="110" height="18" rx="3"/>` + [0, 1, 2, 3].slice(0, 4 - i).map(j => `<rect class="st-pr" x="${130 + j * 26}" y="${12 + i * 32}" width="20" height="18" rx="3"/>`).join('')).join(''); break;
  case 'options': o = [0, 1, 2].map(i => `<circle class="${i === 0 ? 'st-now' : 'st-todo-f'}" cx="22" cy="${22 + i * 30}" r="7"/><rect class="node-soft" x="40" y="${14 + i * 30}" width="${380 - i * 60}" height="16" rx="3"/>`).join(''); break;
  default: o = `<rect class="st-todo" x="10" y="30" width="580" height="22" rx="4"/><rect class="st-line" x="10" y="30" width="240" height="22" rx="4"/>`; }
  return `<svg viewBox="0 0 ${W} 110" ${figA11y('sf-' + family + Math.random().toString(36).slice(2, 6), `A ${family} figure, as a specimen`, `A placeholder drawing of the ${family} figure family with labels in place of data.`)}${o}</svg>`; };
const specPanel = (k, v) => { switch (v) {
  case 'overview': return `${fig(`${k.kind}: opening figure`, `Opening figure: ${k.lead}`, specFig(k.figure), 'Specimen of layout, not of data.', '', false)}<div class="grid grid-3 gap">${[1, 2, 3].map(i => `<span class="card tile spec"><b>n</b> unit<span class="mute">outcome ${i}</span></span>`).join('')}</div>`;
  case 'topic': return `${facts([['Delivers', 'a short phrase'], ['For whom', 'a short phrase'], ['Why now', 'a short phrase']], 'Specimen facts')}${fig(`${k.kind}: topic figure`, 'The topic\u2019s main figure', specFig(k.figure), 'Specimen of layout, not of data.', '', false)}`;
  case 'item': return `<p class="one">One sentence on what the item is.</p>${facts([['Outcome', 'name (ID)'], ['Where it is', chip('stage')], ['State', 'state'], ['Last changed', '00:00 EST, 0 Mon']], 'Specimen strip')}${fig(`${k.kind}: item figure`, 'Where the item is', specFig('timeline'), '', '', false)}`;
  case 'list': return vt({ cols: 'minmax(0,1fr) auto', head: ['Item', 'Where it is'], label: 'Specimen list', rows: [1, 2, 3].map(i => ({ cells: [`<span class="tc"><i class="sm st-line"></i><span class="tt">Item ${i} in words ${idTag('ID-00' + i)}</span></span>`, chip('stage')] })) });
  case 'issues': return `${fig(`${k.kind}: issues figure`, 'Issues by consequence', specFig('severity'), '', '', false)}<div class="grid grid-2 gap">${[1, 2].map(i => `<div class="card"><h4>Issue ${i} in words</h4>${points(['<b>Do:</b> one step', '<b>Why:</b> one reason', '<b>Else:</b> one alternative'])}</div>`).join('')}</div>`;
  case 'decisions': return `<div class="form"><div class="card"><div class="q">1. One question</div><div class="opts">${['Option A', 'Option B'].map((o, i) => `<label class="opt"><input type="radio" name="spec-${slug(k.kind)}" value="${i}"><span>${o}</span>${i ? '<span></span>' : '<span class="rec">recommended</span>'}<span class="cost">cost: one phrase</span></label>`).join('')}</div></div></div>`;
  case 'evidence': return fig(`${k.kind}: evidence figure`, 'Which source each measure was read from', `<div class="matrix" role="img" aria-label="Three measures against three sources, as a specimen"><div class="board-head"><span></span>${['source A', 'source B', 'source C'].map(s => `<span>${s}</span>`).join('')}</div>${['measure 1', 'measure 2', 'measure 3'].map((m, i) => `<div class="board-row"><span class="bar-l">${m}</span>${[0, 1, 2].map(j => `<span class="cell"><i class="dot ${j === i || j === 0 ? 'st-line' : 'none'}"></i></span>`).join('')}</div>`).join('')}</div>`, 'Specimen of layout, not of data.', '', false);
  case 'commands': return `<div class="copy-wrap"><button class="btn" type="button" data-copy="#spec-cmd-${slug(k.kind)}">Copy</button><pre id="spec-cmd-${slug(k.kind)}" tabindex="0">#!/bin/sh\n# one labelled block: the ${k.kind.toLowerCase()} commands\necho 'specimen'</pre></div>`;
  case 'timeline': return fig(`${k.kind}: timeline`, 'Events on one clock', specFig('timeline'), 'Every event carries a wall-clock time with its zone.', '', false);
  default: return '<p class="mute">specimen</p>'; } };
const kindsView = `${view()}<section class="view" id="kinds" data-title="Ten report kinds">
  <h2>Ten report kinds on one shell</h2>
  <p class="one">Each kind is a view tree from the shell\u2019s registry; the status report is this page, the other nine open as navigable specimens.</p>
  <div class="grid grid-2">${R.kinds.map(k => `<a class="card kind" href="#${k.kind === 'Status report' ? 'overview' : 's-' + slug(k.kind)}" data-view-link="${k.kind === 'Status report' ? 'overview' : 's-' + slug(k.kind)}" data-block="Kind: ${esc(k.kind)}"><span class="t">${esc(k.kind)}</span><span class="chips">${k.views.map(v => chip(esc(VIEW_KINDS[v] || v)))}</span><span class="mute">leads with ${esc(k.lead)}</span></a>`).join('')}</div>
</section>`;
const specimenViews = R.kinds.filter(k => k.kind !== 'Status report').map(k => { const id = 's-' + slug(k.kind); return `${view()}<section class="view" id="${id}" data-title="${esc(k.kind)}">
  ${crumbs([['kinds', 'Report kinds'], [id, k.kind]])}
  <div class="vhead"><h2>${esc(k.kind)}</h2>${chip('specimen', 'warn')}</div>
  <p class="one">Leads with ${esc(k.lead)}; reads ${esc(k.reads)}.</p>
  ${tabs(id, k.views.map((v, i) => [v, VIEW_KINDS[v] || v, null, specPanel(k, v)]), `${k.kind} views`)}
  <p class="mute">A specimen of layout with labels in place of data; the report and portal samples task (TSK-1127) builds the real one.</p>
</section>`; }).join('\n');

// ---- portal, docs topic, docs changes -------------------------------------------------------------------------
const DC = (F.docsChanged || []).slice().sort((a, b) => (b.added + b.removed) - (a.added + a.removed));
const dcMax = Math.max(1, ...DC.map(d => d.added + d.removed));
const FG = require('./figures.cjs')({ esc, figA11y, dataTable, F, DC, dcMax });
const portalFig = `<svg class="wide" viewBox="0 0 1000 170" ${figA11y('pl', 'Three portal layers, each on a shell kind', 'Overview map on the overview kind, area page on the topic kind with tabs, reference page on the list and commands kinds; a reader moves between the portal and a report without relearning the page.')}${R.portalLayers.map((l, i) => `<rect class="${i === 1 ? 'acc-wash' : 'node-soft'}" x="${20 + i * 330}" y="20" width="300" height="110" rx="12"/><text x="${40 + i * 330}" y="52" class="b">${esc(l.layer)}</text><text x="${40 + i * 330}" y="78" class="s">${esc(l.same)}</text>${i < 2 ? `<path class="ink2" d="M${322 + i * 330} 75h26"/>` : ''}`).join('')}</svg><svg class="narrow" viewBox="0 0 330 230" ${figA11y('pln', 'Three portal layers, stacked', 'Overview map, area page, reference page.')}${R.portalLayers.map((l, i) => `<rect class="${i === 1 ? 'acc-wash' : 'node-soft'}" x="10" y="${10 + i * 74}" width="310" height="60" rx="10"/><text x="24" y="${36 + i * 74}" class="b">${esc(l.layer)}</text><text x="24" y="${56 + i * 74}" class="s">${esc(l.same)}</text>`).join('')}</svg>`;
const portalView = `${view()}<section class="view" id="portal" data-title="Docs portal layers">
  <h2>The docs portal on the same tokens, three layers</h2>
  ${fig('Portal layers', 'Overview map, area page, reference page', portalFig, 'Each layer maps onto a shell kind.', dataTable(['Layer', 'What it holds', 'Same as'], R.portalLayers.map(l => [esc(l.layer), esc(l.what), esc(l.same)]), 'Portal layers as a table'), false)}
  ${points([`${link('docs', 'A docs topic')}: one topic with its three tabs.`, `${link('docs-since', 'Docs: what changed')}: the portal\u2019s own change page.`, 'The report and portal samples task (TSK-1127) ships one sample of each layer.'])}
</section>`;
const docsView = `${view()}<section class="view" id="docs" data-title="A docs topic">
  ${crumbs([['portal', 'Docs portal'], ['docs', 'Git enforcement: the four planes']])}
  <div class="vhead"><h2>Git enforcement: the four planes</h2>${chip('docs topic')}</div>
  ${tabs('docs', [
    ['concept', 'Concept', null, `${fig('Docs concept figure', 'Four planes around a change', FG.planesConcept(), 'The local planes are fast feedback; required CI and remote rules are the server-side boundary.', '', false)}${points(['Two planes are local: git hooks and in-session guards.', 'Two planes are the server-side boundary: scaffolded CI and remote branch protection.', 'All four read one policy file.'])}`],
    ['architecture', 'Architecture', null, `${fig('Docs architecture figure', 'Where each plane runs', FG.planesArchitecture(), 'Hooks and CI share the policy file and the codeflow ci checks.', '', false)}${points(['The guards are wired in each harness\u2019s settings.', 'Remote protection is armed from the policy and read back by codeflow doctor.'])}`],
    ['technical', 'Technical', null, `${fig('Docs technical figure', 'The five hook shims in event order', FG.planesTechnical(), 'Every tier ships the same armed policy and all five shims.', '', false)}${vt({ cols: 'minmax(0,1fr) minmax(0,1.3fr)', head: ['Command', 'What it does'], label: 'Commands', rows: [['codeflow remote protect', 'arms the remote plane where the host supports it'], ['codeflow doctor --check remote-perimeter', 'reads the live remote rules'], ['codeflow ci', 'the checks the hooks and CI share']].map(r => ({ cells: [`<code>${esc(r[0])}</code>`, `<span class="mute">${esc(r[1])}</span>`] })) })}`],
  ], 'Docs tabs')}
  ${details('Source', '<p class="small">The Enforcement section of <code>.codeflow/rules/git-rules.md</code> (lines 6 to 36 in this worktree); decision d2 of 2 Nov asks for three tabs per topic.</p>')}
</section>`;
const docsSinceView = `${view()}<section class="view" id="docs-since" data-title="Docs: what changed">
  ${crumbs([['portal', 'Docs portal'], ['docs-since', 'What changed']])}
  <h2>What changed in the docs between main and the line</h2>
  ${fig('Docs changed', 'Lines added and removed per docs file', FG.docsSinceFig() + `<ul class="key"><li><i class="sw st-line"></i>lines added</li><li><i class="sw st-block"></i>lines removed</li></ul>`, `${DC.length} files differ: ${DC.filter(d => d.kind === 'new').length} new, ${DC.filter(d => d.kind === 'changed').length} changed, ${DC.filter(d => d.kind === 'removed').length} removed.`, dataTable(['File', 'Kind', 'Added', 'Removed'], DC.map(d => [`<code>${esc(d.file)}</code>`, esc(d.kind), `${d.added} lines`, `${d.removed} lines`]), 'Docs changes as a table'), false)}
  ${details('Source', `<p class="small"><code>git diff --numstat ${esc(F.read.mainTip)} ${esc(F.read.lineTip)} -- docs/</code>, read at ${esc(when(F.read.recordsRefreshedAt || F.read.finished))}.</p>`)}
</section>`;

// ---- menu --------------------------------------------------------------------------------------------------
const row = (id, label, count, icon) => `<a class="row" data-icon="${icon || id}" href="#${id}" data-view-link="${id}"><span class="label">${esc(label)}</span>${viewsChanged.has(id) ? '<span class="dot-ch" title="changed since the previous report"></span>' : ''}${count !== '' && count != null ? `<span class="count">${count}</span>` : ''}</a>`;
const menu = `<section class="area" data-area="release" data-title="Where the release stands" data-icon="status">
        ${row('overview', 'Overview', '', 'overview')}
        <span class="grp">Outcomes</span>
        ${EP.map(e => row('o-' + e.id, ES[e.id].menu, pct1(e.counts.complete, e.counts.total) + '%', 'outcome')).join('\n        ')}
        <span class="grp">Open items</span>
        ${row('blockers', 'Blockers', R.blockers.length)}
        ${row('since', 'What changed', changed.length, 'next')}</section>
      <section class="area" data-area="across" data-title="Across the outcomes" data-icon="epics">
        ${row('epics', 'Epics', EP.length)}
        ${row('lanes', 'Lanes', R.lanes.length, 'units')}
        ${row('evidence', 'Evidence', '')}</section>
      <section class="area" data-area="act" data-title="What you can act on" data-icon="inputs">
        ${row('inputs', 'Your decisions', R.inputs.length, 'decisions')}
        ${row('script', 'Script', '')}</section>
      <section class="area" data-area="kinds" data-title="Report kinds and the portal" data-icon="pages" data-util>
        ${row('kinds', 'Ten report kinds', R.kinds.length, 'pages')}
        ${row('portal', 'Docs portal layers', R.portalLayers.length, 'docs')}
        ${row('docs', 'A docs topic', '', 'docs')}
        ${row('docs-since', 'Docs: what changed', DC.length, 'next')}</section>`;

// ---- local style -------------------------------------------------------------------------------------------
const localStyle = `
.page h1{margin-bottom:8px;font-size:var(--fs-5)} .notice{display:flex;flex-wrap:wrap;gap:8px 12px;align-items:center;color:var(--text-2);font-size:var(--fs-1);margin:0 0 6px} .notice b{color:var(--text);font-weight:500}
.topmeta{display:flex;flex-wrap:wrap;gap:6px;margin:0 0 22px} .topmeta .chip{font-weight:400}
.menu .row{height:auto;min-height:30px;white-space:normal;padding:4px 10px;align-items:flex-start} .menu .row svg{margin-top:2px} .menu .row .label{overflow:visible;text-overflow:clip;line-height:1.3;overflow-wrap:anywhere} .menu .row .count{margin-top:2px}
.menu .grp{display:block;padding:14px 10px 4px;font-size:var(--fs-0);font-weight:600;letter-spacing:.04em;text-transform:uppercase;color:var(--text-2)} .dot-ch{width:7px;height:7px;border-radius:50%;background:var(--accent);flex:none;margin-top:7px}
.crumbs{display:flex;flex-wrap:wrap;gap:4px 8px;font-size:var(--fs-1);color:var(--text-2);margin-bottom:12px} .crumbs .sep{color:var(--text-3)} .crumbs [aria-current]{color:var(--text)}
.vhead{display:flex;flex-wrap:wrap;gap:8px 12px;align-items:center;margin-bottom:8px} .vhead h2{margin-right:4px}
.id{color:var(--text-2);font-size:var(--fs-0);font-weight:500;letter-spacing:.02em;white-space:nowrap} .chip>.id{color:inherit} .mute{color:var(--text-2)} .nr{color:var(--text-3);font-style:italic} .small{font-size:var(--fs-1)} .gap{margin-top:18px} h3.gap{margin-top:22px} h4{font-size:var(--fs-2);font-weight:600;margin:0 0 6px}
p.one{font-size:var(--fs-3);color:var(--text-2);max-width:none;margin:4px 0 14px} .card .one,.outcome .one{display:block;font-size:var(--fs-1);color:var(--text-2);margin-top:10px}
.hero{margin-top:6px} .hero .num{display:flex;align-items:baseline;gap:10px} .hero-side{display:grid;gap:2px;font-size:var(--fs-2);align-content:center} .hero-side b{font-size:var(--fs-3)} .page{padding-bottom:32px}
.delta{display:inline-block;font-size:var(--fs-0);font-weight:600;padding:1px 7px;border-radius:var(--r-pill);background:var(--sunken);color:var(--text-2);vertical-align:middle;margin-left:6px} .delta.up{background:var(--ok-wash);color:var(--ok)} .delta.down{background:var(--danger-wash);color:var(--danger)}
.facts{display:grid;grid-template-columns:repeat(auto-fit,minmax(230px,1fr));background:var(--surface);border:1px solid var(--line);border-right:0;border-bottom:0;border-radius:var(--r-2);overflow:hidden;box-shadow:var(--shadow-1);margin:0 0 4px} .facts>div{padding:10px 14px;display:grid;gap:3px;min-width:0;border-right:1px solid var(--line);border-bottom:1px solid var(--line)} .facts dt{font-size:var(--fs-0);font-weight:600;letter-spacing:.04em;text-transform:uppercase;color:var(--text-2)} .facts dd{margin:0;font-size:var(--fs-1);overflow-wrap:anywhere} .facts>div.wide{grid-column:span 2}
.part{display:flex;gap:2px;height:30px;border-radius:6px;overflow:hidden} .part i{display:block;height:100%;min-width:2px} .part.mini{height:10px;border-radius:5px;margin:10px 0 8px}
.st-main{background:var(--accent)} .st-line{background:${STAGE_LINE_FILL};box-shadow:inset 0 0 0 1px var(--accent)} .st-pr{background:var(--warn-mark)} .st-todo{background:var(--stage-todo)} .st-block{background:var(--danger)} .st-todo-bg{background:var(--sunken)}
.fig .st-main,.fig .st-line,.fig .st-pr,.fig .st-todo,.fig .st-block{stroke:none} svg .st-main{fill:var(--accent)} svg .st-line{fill:${STAGE_LINE_FILL}} svg .st-pr{fill:var(--warn-mark)} svg .st-todo{fill:var(--stage-todo)} svg .st-block{fill:var(--danger)}
svg .st-line-f{fill:${STAGE_LINE_FILL};stroke:var(--accent);stroke-width:1} :is(.fig,.marks) svg .st-pr{fill:var(--warn-mark)} svg .st-todo-f{fill:var(--stage-todo)} svg .st-block-f{fill:var(--danger)} svg .st-now{fill:var(--surface);stroke:var(--accent);stroke-width:3} svg text{fill:var(--text-2);font-size:12.5px} svg text.b{fill:var(--text);font-weight:600} svg text.s{fill:var(--text-2)}
.key{display:flex;flex-wrap:wrap;gap:6px 16px;margin:10px 0 0;padding:0;list-style:none;color:var(--text-2);font-size:var(--fs-0)} .key li{display:inline-flex;align-items:center;gap:6px} .key b{color:var(--text)} .sw{width:12px;height:12px;border-radius:3px;display:inline-block}
.sm{display:inline-block;width:10px;height:10px;border-radius:50%;flex:none} .tc{display:inline-flex;gap:8px;align-items:baseline;min-width:0} .why{display:block;font-size:var(--fs-0);padding-left:18px} .tc .sm{transform:translateY(1px)} .tt{min-width:0;overflow-wrap:anywhere}
.outcomes .outcome{display:block;color:inherit;text-decoration:none} .outcome:hover{box-shadow:var(--shadow-2)} .outcome .t,.kind .t,.epic .head span:not(.p){font-weight:600} .outcome .p,.epic .p{display:block;font-size:var(--fs-5);font-weight:600;letter-spacing:-.02em;margin-top:6px;font-variant-numeric:tabular-nums} .outcome .p small,.epic .p small,.hero .num small{font-size:var(--fs-3);color:var(--text-2);font-weight:500}
.nums{display:flex;flex-wrap:wrap;gap:4px 14px;font-size:var(--fs-1);color:var(--text-2)} .nums b{color:var(--text)} .epic .head{display:flex;gap:14px;align-items:flex-start} .epic .facts{margin-top:12px}
.grid-4{grid-template-columns:repeat(auto-fit,minmax(200px,1fr))} .tiles{margin-top:14px} .tiles a{text-decoration:none;color:inherit} .tile{display:grid;gap:2px;font-size:var(--fs-1)} .tile b{font-size:var(--fs-4);color:var(--text)} .tile .mute{font-size:var(--fs-0)} .tile.spec{color:var(--text-2)}
.streams{display:grid;gap:8px} .stream{display:grid;grid-template-columns:minmax(0,1.4fr) auto 9ch minmax(0,1fr);gap:14px;align-items:center;font-size:var(--fs-1)} .st-name{font-weight:600;color:var(--text)} .st-owner{color:var(--text-2)}
.steps i{width:12px;height:12px} .steps i.done{background:var(--accent);box-shadow:none} .steps i.now{background:var(--surface);box-shadow:inset 0 0 0 3px var(--accent)}
.bars{display:grid;gap:6px} .bar-row,.owner-row{display:grid;grid-template-columns:minmax(0,1.2fr) minmax(0,2fr) 10ch;gap:12px;align-items:center;font-size:var(--fs-1)} .bar-l{min-width:0;overflow-wrap:anywhere} .bar-t{display:block;height:12px} .bar-t>i{display:block;height:100%;border-radius:4px;overflow:hidden} .bar-t>i>i{display:block;height:100%} .bar-t.two{display:grid;gap:2px;height:auto} .bar-t.two>i{height:7px} .bar-n{color:var(--text-2);text-align:right;font-variant-numeric:tabular-nums}
.dots{display:flex;flex-wrap:wrap;gap:4px} .dot{display:inline-block;width:14px;height:14px;border-radius:4px} .dot.none{background:var(--sunken)} a.dot:focus-visible{outline:2px solid var(--focus);outline-offset:2px}
.board,.matrix{display:grid;gap:4px;font-size:var(--fs-1)} .board-head,.board-row{display:grid;grid-template-columns:minmax(0,1.6fr) repeat(var(--cols,5),minmax(0,1fr));gap:6px;align-items:center} .board-head span{font-size:var(--fs-0);font-weight:600;letter-spacing:.04em;text-transform:uppercase;color:var(--text-2);overflow-wrap:anywhere} .board-row .cell{min-height:22px;display:flex;flex-wrap:wrap;gap:3px;align-items:center;padding:3px 0;border-top:1px solid var(--line)} .board-row .bar-l{border-top:1px solid var(--line);padding:3px 0}
.matrix .board-head,.matrix .board-row{--cols:5}
.more-body{grid-template-columns:minmax(0,1fr)} .more-body p,.rec-list li{overflow-wrap:anywhere} .rec-list{margin:0;padding-left:20px;display:grid;gap:4px} .rec-list .rec-list{margin-top:4px} .rec-list>li>p{margin:4px 0 0} .rec-h{margin:4px 0 0;font-size:var(--fs-1);font-weight:600} .rec-table{overflow-x:auto;font-size:var(--fs-1)} .rec-table table{border-collapse:collapse;width:100%} .rec-table th,.rec-table td{text-align:left;vertical-align:top;padding:4px 8px;border-bottom:1px solid var(--line)} .rec-table th{font-weight:600}
.more{margin-top:12px;border-top:1px solid var(--line);padding-top:8px} .more>summary{cursor:pointer;color:var(--accent-ink);font-size:var(--fs-1);font-weight:500} .more-body{margin-top:8px;display:grid;gap:8px} .more.inline{margin:4px 0 0;border:0;padding:0} .more.inline>summary{font-size:var(--fs-0)} .terms{display:grid;gap:4px;margin:0} .terms div{display:grid;grid-template-columns:14ch minmax(0,1fr);gap:10px;font-size:var(--fs-1)} .terms dt{font-weight:600} .terms dd{margin:0;color:var(--text-2)}
.fig-data{margin-top:10px;font-size:var(--fs-1)} .fig-data summary{cursor:pointer;color:var(--accent-ink);font-weight:500} .fig-data table{border-collapse:collapse;width:100%;margin-top:8px} .fig-data th,.fig-data td{text-align:left;padding:6px 8px;border-top:1px solid var(--line);font-weight:400;vertical-align:top} .fig-data thead th{font-size:var(--fs-0);text-transform:uppercase;letter-spacing:.04em;color:var(--text-2)}
.path{max-width:460px}
.blks{grid-template-columns:1fr} .deps{grid-template-columns:minmax(0,1fr) minmax(0,1fr)} @media (max-width:1499px){.deps{grid-template-columns:1fr}} @media (max-width:1300px){ .bar-row,.owner-row{grid-template-columns:minmax(0,1fr) 10ch} .bar-row .bar-t,.owner-row .dots{grid-column:1/3} } .blk h3{font-size:var(--fs-2);line-height:1.35;margin-bottom:8px} .blk .chips{margin-bottom:8px} .points li b{font-weight:600}
.form .q{display:block} .save-row{display:flex;flex-wrap:wrap;gap:8px 12px;align-items:center;margin-top:10px} .save-row .btn{height:30px;padding:0 12px;font-size:var(--fs-1)} .saved{font-size:var(--fs-1);color:var(--text-2)} .saved.ok{color:var(--ok)} .saved.warn{color:var(--warn)} .saved b{color:var(--text)} .form textarea{width:100%;margin-top:8px;font:inherit;font-size:var(--fs-1);padding:8px 10px;border-radius:var(--r-2);border:1px solid var(--line-2);background:var(--raised);color:var(--text)} .opt .cost{grid-column:2/4;color:var(--text-2);font-size:var(--fs-0)} .opt{grid-template-rows:auto auto}
pre{margin:0;padding:14px 16px;background:var(--sunken);border-radius:var(--r-2);font-family:var(--mono);font-size:12.5px;line-height:1.5;overflow:auto;white-space:pre}
.page p a:not(.btn),.page li a,.page dd a,.page figcaption a,.vt [role=cell] a,.bar-l a{text-decoration:underline;text-underline-offset:2px;text-decoration-color:color-mix(in srgb,currentColor 45%,transparent)} .kind{display:grid;gap:8px;color:inherit;text-decoration:none} .kind:hover{box-shadow:var(--shadow-2)}
.tabpanel>.fig:first-child{margin-top:0}
@media (max-width:640px){ .fig-data thead{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0)} .fig-data tr{display:grid;gap:2px;padding:6px 0;border-top:1px solid var(--line)} .fig-data th,.fig-data td{border:0;padding:0} .fig-data [data-l]::before{content:attr(data-l) ": ";font-size:var(--fs-0);font-weight:600;letter-spacing:.04em;text-transform:uppercase;color:var(--text-2)}
  .stream{grid-template-columns:1fr 1fr} .st-name{grid-column:1/3} .bar-row,.owner-row{grid-template-columns:minmax(0,1fr) 10ch} .bar-row .bar-t,.owner-row .dots{grid-column:1/3} .board-head,.board-row{--cols:5;font-size:var(--fs-0)} .terms div{grid-template-columns:1fr} }
@media (min-width:768px){ .page{padding-right:calc(var(--page-x, 24px) + 118px)} }
@media (max-width:767px){ .comment-btn{position:static;display:block;margin:24px auto 0;box-shadow:var(--shadow-1)} .page{padding-bottom:24px} }
`;

// ---- runtime: tabs, copy, script -----------------------------------------------------------------------------
const runtime = `
(() => {
  const selectTab = b => { const list = b.closest('[role=tablist]'); list.querySelectorAll('[role=tab]').forEach(x => { const on = x === b; x.setAttribute('aria-selected', String(on)); x.tabIndex = on ? 0 : -1; const p = document.getElementById(x.getAttribute('aria-controls')); if (p) p.hidden = !on; }); };
  document.addEventListener('click', e => { const b = e.target.closest('.tab[role=tab]'); if (b) selectTab(b); });
  document.addEventListener('keydown', e => { const b = e.target.closest('.tab[role=tab]'); if (!b) return; const tabs = [...b.closest('[role=tablist]').querySelectorAll('[role=tab]')]; let n = null; const i = tabs.indexOf(b);
    if (e.key === 'ArrowRight') n = tabs[(i + 1) % tabs.length]; else if (e.key === 'ArrowLeft') n = tabs[(i - 1 + tabs.length) % tabs.length]; else if (e.key === 'Home') n = tabs[0]; else if (e.key === 'End') n = tabs[tabs.length - 1]; if (n) { e.preventDefault(); selectTab(n); n.focus(); } });
})();`;
const scriptJs = `
(() => {
  const RECHECK = ${JSON.stringify(R.recheck)}, FILE = ${JSON.stringify(R.answersFile)}, AS_OF = ${JSON.stringify(F.read.finished)};
  const form = document.getElementById('decisions'), out = document.getElementById('script-text');
  const q = s => "'" + String(s).replace(/'/g, "'\\\\''") + "'";
  function build() {
    const answers = [...form.querySelectorAll('input[type=radio]:checked')].map(i => ({ id: i.name, value: i.value, label: i.dataset.label }));
    const changes = (form.querySelector('textarea[name=changes]').value || '').trim();
    let t = '#!/bin/sh\\n# CodeFlow 4.0.0 status prototype, facts as of ' + AS_OF + '. Part 1 rechecks (read only). Part 2 records only the decisions answered on the page.\\nset -eu\\nW=/Volumes/DATA/Local/software-workspace/projects/agentic-systems\\n\\n# Part 1: recheck\\n' + RECHECK + '\\n\\n# Part 2: record\\n';
    if (!answers.length && !changes) t += "echo 'No decision was answered on the page: nothing is written, nothing is committed.'\\n";
    else {
      t += 'F=' + FILE + '\\n[ "$(git -C "$W" rev-parse --abbrev-ref HEAD)" = "integration/workspace" ] || { echo "the workspace is not on integration/workspace; nothing written"; exit 1; }\\nmkdir -p "$W/$(dirname "$F")"\\n[ -f "$W/$F" ] || printf \\'%s\\\\n\\' \\'# TSK-1122 walk-through answers\\' > "$W/$F"\\n';
      for (const a of answers) t += 'printf \\'%s\\\\n\\' ' + q('- ' + AS_OF + ', ' + a.id + ': ' + a.value + ' (' + a.label + ')') + ' >> "$W/$F"\\n';
      if (changes) t += 'printf \\'%s\\\\n\\' ' + q('- ' + AS_OF + ', changes named: ' + changes) + ' >> "$W/$F"\\n';
      t += 'git -C "$W" add -- "$F"\\ngit -C "$W" commit -m "docs(coordination): record the operator\\'s TSK-1122 walk-through answers" -- "$F"\\necho "Recorded ' + (answers.length + (changes ? 1 : 0)) + ' line(s) in $F on integration/workspace; nothing else changed."\\n';
    }
    out.textContent = t;
  }
  form.addEventListener('change', build); form.addEventListener('input', build); build();
  // answers: a draft in localStorage, Save through the File System Access API, a download where it is missing
  const FILE_PATH = form.dataset.answersFile, DRAFT = 'cf4.answers.draft', HANDLE_DB = 'cf4-answers';
  const zone = () => { try { const n = new Intl.DateTimeFormat('en-US', { timeZoneName: 'short' }).formatToParts(new Date()).find(x => x.type === 'timeZoneName'); if (n && n.value) return n.value; } catch {} return 'UTC'; };
  const clock = d => { const p = n => String(n).padStart(2, '0'); const mon = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec']; return p(d.getHours()) + ':' + p(d.getMinutes()) + ' ' + zone() + ', ' + d.getDate() + ' ' + mon[d.getMonth()]; };
  const stamp = d => { const p = n => String(n).padStart(2, '0'); return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + ' ' + p(d.getHours()) + ':' + p(d.getMinutes()) + ' ' + zone(); };
  const fields = () => [...form.querySelectorAll('[data-field]')].map(card => { const id = card.dataset.field; const r = card.querySelector('input[type=radio]:checked'); const ta = card.querySelector('textarea'); return { id, label: card.dataset.label, value: r ? r.value : ta ? ta.value : '', text: r ? r.dataset.label : ta ? ta.value : '' }; });
  const draftRead = () => { try { return JSON.parse(localStorage.getItem(DRAFT) || '{}'); } catch { return {}; } };
  const draftWrite = () => { try { const d = {}; for (const f of fields()) d[f.id] = f.value; localStorage.setItem(DRAFT, JSON.stringify(d)); } catch {} };
  // restore finds controls by name and sets values directly (never inside a selector), so quotes, backslashes and
  // newlines in a draft cannot break it; a failure here must not stop the Save listeners below from attaching
  const restore = () => { const d = draftRead(); for (const [k, v] of Object.entries(d)) { if (!v) continue; const got = form.elements.namedItem(k); if (!got) continue; const els = typeof got.length === 'number' && !got.tagName ? [...got] : [got]; for (const el of els) { if (el.type === 'radio') { if (String(el.value) === String(v)) el.checked = true; } else if (el.tagName === 'TEXTAREA') el.value = String(v); } } build(); };
  try { restore(); } catch (err) { console.error('draft restore failed', err); } form.addEventListener('input', draftWrite); form.addEventListener('change', draftWrite);
  const payload = () => { const now = new Date(); return JSON.stringify({ page: 'CodeFlow 4.0.0 status prototype (TSK-1122)', factsAsOf: AS_OF, savedAt: stamp(now), answers: fields().filter(f => f.value).map(f => ({ id: f.id, label: f.label, text: f.text, value: f.value, page: 'Your decisions', time: stamp(now) })) }, null, 2) + '\\n'; };
  const idb = () => new Promise((ok, no) => { const r = indexedDB.open(HANDLE_DB, 1); r.onupgradeneeded = () => r.result.createObjectStore('h'); r.onsuccess = () => ok(r.result); r.onerror = () => no(r.error); });
  const handleGet = async () => { try { const db = await idb(); return await new Promise((ok, no) => { const t = db.transaction('h').objectStore('h').get('repo'); t.onsuccess = () => ok(t.result || null); t.onerror = () => no(t.error); }); } catch { return null; } };
  const handlePut = async h => { try { const db = await idb(); await new Promise((ok, no) => { const t = db.transaction('h', 'readwrite').objectStore('h').put(h, 'repo'); t.onsuccess = ok; t.onerror = () => no(t.error); }); } catch {} };
  const isRepo = async h => { try { await h.getDirectoryHandle('coordination'); return true; } catch { return false; } };
  const repoHandle = async () => { let h = await handleGet(); if (h && h.queryPermission) { let p = await h.queryPermission({ mode: 'readwrite' }); if (p !== 'granted') p = await h.requestPermission({ mode: 'readwrite' }); if (p !== 'granted') h = null; } if (!h || !(await isRepo(h))) { h = await window.showDirectoryPicker({ mode: 'readwrite', id: 'agentic-systems' }); if (!(await isRepo(h))) throw new Error('That folder has no coordination/ directory; pick the agentic-systems workspace root.'); await handlePut(h); } return h; };
  const writeFile = async (root, path, text) => { const parts = path.split('/'); let dir = root; for (const p of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(p, { create: true }); const fh = await dir.getFileHandle(parts[parts.length - 1], { create: true }); const w = await fh.createWritable(); await w.write(text); await w.close(); };
  const download = text => { const a = document.createElement('a'); a.href = URL.createObjectURL(new Blob([text], { type: 'application/json' })); a.download = 'answers.json'; document.body.appendChild(a); a.click(); setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000); };
  const say = (id, cls, html) => { const el = document.getElementById('saved-' + id); if (!el) return; el.className = 'saved ' + cls; el.innerHTML = html; };
  form.addEventListener('click', async e => { const b = e.target.closest('[data-save]'); if (!b) return; const id = b.dataset.save; const text = payload(); const now = clock(new Date()); b.disabled = true;
    try {
      if (window.showDirectoryPicker) { const root = await repoHandle(); await writeFile(root, FILE_PATH, text); say(id, 'ok', 'Saved to <b>' + FILE_PATH + '</b> at ' + now + '.'); }
      else { download(text); say(id, 'warn', 'This browser cannot write into the repository: <b>answers.json</b> was downloaded to your browser\\'s download folder at ' + now + '; move it to <b>' + FILE_PATH + '</b>.'); }
      window.cfToast && window.cfToast('Saved');
    } catch (err) { say(id, 'warn', 'Not saved: ' + String(err && err.message || err)); }
    b.disabled = false; });
  // the Comment button: fixed bottom-right on desktop with a reserved lane at the page's right edge, in the flow under 768 px
  const cb = document.querySelector('.comment-btn'), pg = document.querySelector('.page'); if (cb && pg) pg.appendChild(cb);
})();`;

// ---- page --------------------------------------------------------------------------------------------------
const html = `<!doctype html>
<html lang="en" data-theme="graphite">
<head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>CodeFlow 4.0.0 status prototype</title>
<meta name="description" content="A design prototype for the status report on real 4.0.0 facts read at ${esc(READ_AT)}: figures first, short points, drill-down from outcome to task.">
<script>(()=>{try{const m=localStorage.getItem('cf4.mode'),t=localStorage.getItem('cf4.theme'),a=localStorage.getItem('cf4.accent'),s=localStorage.getItem('cf4.size');const r=document.documentElement;if(m&&m!=='system')r.setAttribute('data-mode',m);if(t)r.setAttribute('data-theme',t);if(a)r.setAttribute('data-accent',a);if(s)r.setAttribute('data-size',s);}catch{}})();</script>
<style>${kit('cf.css')}</style>
<style>${localStyle}</style>
</head>
<body>
<div class="shell">
  <header class="bar" data-brand="present" data-crumb="4.0.0 status" data-crumb-view="Overview"></header>
  <aside class="menu" aria-label="Report">
    <div class="menu-scroll">${menu}</div>
    <div class="grip" role="separator" tabindex="0" aria-orientation="vertical" aria-label="Resize navigation" aria-valuemin="220" aria-valuemax="360" aria-valuenow="260" title="Drag or use the arrow keys. Double-click resets."></div>
  </aside>
  <main class="main" id="main"><div class="page">
    <h1>CodeFlow 4.0.0 status</h1>
    <div class="notice" data-block="Prototype notice">${chip('prototype', 'accent')}<span>A design prototype for the status report, on real facts read once at <b>${esc(READ_AT)}</b>; not a report to act on.</span></div>
    <div class="topmeta">${chip(`line tip ${esc(F.read.lineTip)}`)}${chip(`main ${esc(F.read.mainTip)}`)}${chip(`previous report ${esc(PREV_W)}`)}${chip(`trackers ${esc(TRACK_AT)}`)}</div>
    ${overview}
    ${topicViews}
    ${themeViews}
    ${taskViews}
    ${featureViews}
    ${epicsView}
    ${blockersView}
    ${sinceView}
    ${lanesView}
    ${evidenceView}
    ${inputsView}
    ${scriptView}
    ${kindsView}
    ${specimenViews}
    ${portalView}
    ${docsView}
    ${docsSinceView}
  </div></main>
</div>
<aside class="rail" aria-label="Comments"><b>Comments</b> <span class="mute">click a block to mark it</span><ol></ol></aside>
<script>${kit('cf-icons.js')}</script>
<script>${runtime}</script>
<script>${kit('cf.js')}</script>
<script>${scriptJs}</script>
</body></html>
`;
fs.writeFileSync(OUT, html);
const split = html.replace('<style>' + kit('cf.css') + '</style>', '<link rel="stylesheet" href="cf.css">').replace('<script>' + kit('cf-icons.js') + '</script>', '<script src="cf-icons.js"></script>').replace('<script>' + kit('cf.js') + '</script>', '<script src="cf.js"></script>');
fs.writeFileSync(path.join(HERE, 'kit', 'index.html'), split);
console.log('wrote', OUT, html.length, 'bytes;', Object.keys(T).length, 'task views,', EP.reduce((n, e) => n + Object.keys(e.themes).length, 0), 'theme views,', (html.match(/Figure \d+</g) || []).length, 'numbered figures,', changed.length, 'changed numbers,', DC.length, 'docs files changed');
