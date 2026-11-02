#!/usr/bin/env node
// TSK-1122 prototype builder: one self-contained page (kit CSS and JS inlined, facts embedded) from _src/facts.json
// (every number), _src/report.json (prose facts with sources) and _src/previous-2026-11-01-1431.json (the change marks'
// baseline). Usage: node build.cjs   -> ../prototype.html
const fs = require('fs'); const path = require('path');
const HERE = __dirname, OUT = path.resolve(HERE, '../prototype.html');
const F = JSON.parse(fs.readFileSync(path.join(HERE, 'facts.json'), 'utf8'));
const R = JSON.parse(fs.readFileSync(path.join(HERE, 'report.json'), 'utf8'));
const P = JSON.parse(fs.readFileSync(path.join(HERE, 'previous-2026-11-01-1431.json'), 'utf8'));
const kit = n => fs.readFileSync(path.join(HERE, 'kit', n), 'utf8');
const esc = s => String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const md = s => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>');
const pct1 = (a, b) => b ? (Math.round(1000 * a / b) / 10).toFixed(1) : '0.0';
const clip = (s, n = 150) => { s = String(s || '').replace(/\s+/g, ' ').trim(); if (s.length <= n) return s; const cut = s.slice(0, n); return cut.slice(0, Math.max(cut.lastIndexOf(' '), 40)) + ' …'; };
const S = F.scope, overall = pct1(S.complete, S.total), PREV_AT = P.snapshotAt, EP = Object.values(F.epics), T = F.tasks;

// ---- change marks since the previous report -----------------------------------------------------------------
const delta = (now, prev, label = '') => { if (prev == null || now === prev) return ''; const d = Math.round((now - prev) * 10) / 10; return `<span class="delta ${d > 0 ? 'up' : 'down'}" title="was ${prev} at ${esc(PREV_AT)}">${d > 0 ? '+' : ''}${d}${label ? ' ' + esc(label) : ''}</span>`; };
const prevEpic = id => P.epics.find(e => e.id === id) || {};
const changes = [];
changes.push({ what: 'tasks in scope', prev: P.scope.total, now: S.total, view: 'overview' });
changes.push({ what: 'tasks complete', prev: P.scope.complete, now: S.complete, view: 'overview' });
changes.push({ what: 'complete on main', prev: P.scope.onMain, now: S.onMain, view: 'overview' });
changes.push({ what: 'tasks in a pull request', prev: P.scope.inPr, now: S.inPr, view: 'overview' });
changes.push({ what: 'pull requests open against the line', prev: P.prs.openAgainstLine, now: F.gh.open.length, view: 'overview' });
changes.push({ what: 'commits the line is ahead of main', prev: P.aheadOfMain, now: F.gh.ahead, view: 'overview' });
for (const e of EP) { const p = prevEpic(e.id); changes.push({ what: `${e.id} tasks`, prev: p.total, now: e.counts.total, view: 'o-' + e.id }); changes.push({ what: `${e.id} complete`, prev: p.done, now: e.counts.complete, view: 'o-' + e.id }); }
const changed = changes.filter(c => c.prev !== c.now);
const prevComplete = new Set(); // ids complete on 11-01, from the previous live read embedded beside the snapshot
try { const lc = JSON.parse(fs.readFileSync(path.join(HERE, 'previous-live-counts-2026-11-01-1431.json'), 'utf8')); for (const e of Object.values(lc.epics)) for (const i of e.completeIds) prevComplete.add(i); } catch {}
const newlyComplete = prevComplete.size ? Object.keys(T).filter(i => T[i].status === 'complete' && !prevComplete.has(i)).sort() : null;
const viewsChanged = new Set(changed.map(c => c.view));

// ---- figures -----------------------------------------------------------------------------------------------
const stations = [['on main', S.onMain, 'st-main'], ['complete on the line only', S.complete - S.onMain, 'st-line'], ['in a pull request', S.inPr, 'st-pr'], ['not started', S.todo, 'st-todo'], ['blocked', S.blocked, 'st-block']];
function partitionWide() {
  const W = 1000, x0 = 20, bw = 960, y = 46, h = 34; let x = x0, o = '';
  for (const [, v, c] of stations) { const w = v / S.total * bw; if (v > 0) o += `<rect class="${c}" x="${(x + 1).toFixed(1)}" y="${y}" width="${Math.max(w - 2, 2).toFixed(1)}" height="${h}" rx="3"/>`; x += w; }
  x = x0; const labels = []; for (const [l, v] of stations) { const w = v / S.total * bw; if (v > 0) labels.push({ l, v, cx: x + w / 2, tw: String(v).length * 9 + 6 + l.length * 7.2 }); x += w; }
  let lx = x0; const ty = y + h + 44, mid = y + h + 14;
  for (const a of labels) { const tx = Math.max(lx, Math.min(a.cx - a.tw / 2, W - 20 - a.tw)); const lc = tx + a.tw / 2; lx = tx + a.tw + 18; o += `<path class="ink3" d="M${a.cx.toFixed(1)} ${y + h + 2}V${mid}H${lc.toFixed(1)}V${ty - 15}"/><text x="${tx.toFixed(1)}" y="${ty}" class="b">${a.v}</text><text x="${(tx + String(a.v).length * 9 + 6).toFixed(1)}" y="${ty}" class="s">${a.l}</text>`; }
  o += `<text x="${x0}" y="26" class="b">${S.total} tasks in the three epics</text><text x="${W - 20}" y="26" text-anchor="end" class="s">${S.complete} complete, ${overall}%</text>`;
  return `<svg class="wide" viewBox="0 0 ${W} ${ty + 10}" role="img" aria-labelledby="p1t"><title id="p1t">${S.total} tasks: ${S.onMain} on main, ${S.complete - S.onMain} complete on the line only, ${S.inPr} in a pull request, ${S.todo} not started, ${S.blocked} blocked</title>${o}</svg>`;
}
function partitionNarrow() {
  const W = 330, x0 = 10, bw = 310, y = 30, h = 28; let x = x0, o = '';
  for (const [, v, c] of stations) { const w = v / S.total * bw; if (v > 0) o += `<rect class="${c}" x="${(x + 1).toFixed(1)}" y="${y}" width="${Math.max(w - 2, 2).toFixed(1)}" height="${h}" rx="3"/>`; x += w; }
  o += stations.map(([l, v, c], i) => `<rect class="${c}" x="${x0}" y="${80 + i * 26}" width="14" height="14" rx="3"/><text x="${x0 + 22}" y="${92 + i * 26}" class="b">${v}</text><text x="${x0 + 22 + String(v).length * 9 + 6}" y="${92 + i * 26}" class="s">${l}</text>`).join('');
  o += `<text x="${x0}" y="18" class="b">${S.total} tasks, ${overall}% complete</text>`;
  return `<svg class="narrow" viewBox="0 0 ${W} 222" role="img" aria-labelledby="p1tn"><title id="p1tn">The same partition, read as a list</title>${o}</svg>`;
}
const bar = (c, w = 400) => { const tot = c.total || 1; return `<svg class="marks" viewBox="0 0 ${w} 22" style="width:100%;height:auto;display:block" role="img" aria-label="${c.complete} of ${c.total} complete, ${c.onMain} on main, ${c.inPr} in a pull request"><rect class="st-todo" x="0" y="4" width="${w}" height="14" rx="4"/><rect class="st-line" x="0" y="4" width="${(w * c.complete / tot).toFixed(1)}" height="14" rx="4"/><rect class="st-main" x="0" y="4" width="${(w * c.onMain / tot).toFixed(1)}" height="14" rx="4"/>${c.inPr ? `<rect class="st-pr" x="${(w * c.complete / tot + 2).toFixed(1)}" y="4" width="${Math.max(w * c.inPr / tot - 2, 2).toFixed(1)}" height="14" rx="3"/>` : ''}${c.blocked ? `<rect class="st-block" x="${(w - 2 - w * c.blocked / tot).toFixed(1)}" y="4" width="${Math.max(w * c.blocked / tot, 3).toFixed(1)}" height="14" rx="2"/>` : ''}</svg>`; };
const stepDots = (n, of) => `<span class="steps" role="img" aria-label="step ${n} of ${of}">${Array.from({ length: of }, (_, i) => `<i class="${i + 1 < n ? 'done' : i + 1 === n ? 'now' : ''}"></i>`).join('')}</span>`;
const MARK = { ok: '<circle class="ok-fill" cx="12" cy="12" r="9"/><path style="stroke:var(--ground);fill:none;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round" d="M7.5 12l3 3 6-6"/>', conf: '<rect class="warn-fill" x="5" y="5" width="14" height="14" rx="3" transform="rotate(45 12 12)"/><path style="stroke:var(--ground);fill:none;stroke-width:1.6;stroke-linecap:round" d="M12 7.5v4m0 0l-3 3.5m3-3.5l3 3.5"/>', unk: '<circle class="unk-ring" cx="12" cy="12" r="8.5"/><path style="stroke:var(--fig-ink-2);fill:none;stroke-width:1.6;stroke-linecap:round" d="M12 15.5v.5M10 10a2 2 0 1 1 2.6 1.9c-.4.2-.6.5-.6.9v.4"/>' };
const mark = k => `<svg class="marks" viewBox="0 0 24 24" aria-hidden="true">${MARK[k]}</svg>`;
const stMark = t => t.status === 'complete' ? (t.onMain ? '<i class="sm st-main" title="on main"></i>' : '<i class="sm st-line" title="complete on the line"></i>') : t.status === 'blocked' ? '<i class="sm st-block" title="blocked"></i>' : t.location === 'pull request' ? '<i class="sm st-pr" title="in a pull request"></i>' : '<i class="sm st-todo" title="not started"></i>';
const locChip = t => `<span class="chip ${t.onMain ? 'plain' : t.status === 'complete' ? 'accent' : t.location === 'pull request' ? 'warn' : t.status === 'blocked' ? 'danger' : 'plain'}">${esc(t.location)}${t.prs.length ? ' ' + t.prs.map(n => '#' + n).join(' ') : ''}</span>`;
const prState = p => p.mergeable === 'MERGEABLE' ? 'ok' : p.mergeable === 'CONFLICTING' ? 'conf' : 'unk';

// ---- shared pieces -----------------------------------------------------------------------------------------
const crumbs = parts => `<nav class="crumbs" aria-label="Breadcrumb">${parts.map((p, i) => i < parts.length - 1 ? `<a href="#${p[0]}" data-view-link="${p[0]}">${esc(p[1])}</a><span class="sep" aria-hidden="true">/</span>` : `<span aria-current="page">${esc(p[1])}</span>`).join('')}</nav>`;
const strip = facts => `<dl class="strip" aria-label="Context">${facts.map(([k, v, cls]) => `<div><dt>${esc(k)}</dt><dd class="${cls || ''}">${v}</dd></div>`).join('')}</dl>`;
const tabs = (id, list) => `<div class="tabs" role="tablist" aria-label="Aspects">${list.map(([k, l, n], i) => `<button class="tab" role="tab" id="${id}-tab-${k}" aria-controls="${id}-${k}" aria-selected="${i === 0}" type="button">${esc(l)}${n != null ? ` <span class="count">${n}</span>` : ''}</button>`).join('')}</div>${list.map(([k, , , body], i) => `<div class="tabpanel" role="tabpanel" id="${id}-${k}" aria-labelledby="${id}-tab-${k}"${i ? ' hidden' : ''}>${body}</div>`).join('')}`;
const taskRow = t => `<div><span class="tc">${stMark(t)}<a href="#k-${t.id}" data-view-link="k-${t.id}" class="mono">${t.id}</a></span><span class="tt">${esc(t.title)}</span>${locChip(t)}</div>`;
const taskTable = (ids, cols = '12ch 1fr auto') => `<div class="vt tasks" style="--vt-cols:${cols}"><div class="vt-head"><span>Task</span><span>Title</span><span>Location</span></div>${ids.map(i => taskRow(T[i])).join('')}</div>`;
const epicState = e => e.status === 'in_progress' ? 'in progress' : e.status;
const readyIn = e => e.tasks.filter(i => T[i].next && T[i].next.state === 'ready').length;
const waitingIn = e => e.tasks.filter(i => T[i].next && T[i].next.state === 'waiting').length;
const nextStep = e => e.counts.complete === e.counts.total ? 'close the epic' : readyIn(e) ? `${readyIn(e)} tasks ready to start, ${waitingIn(e)} waiting on a predecessor` : waitingIn(e) ? `${waitingIn(e)} tasks waiting on a predecessor; none ready` : 'no ready task listed by work next';
const plainLine = s => String(s || '').replace(/\x60integration\/EPC-\d+-[^\x60]*\x60/g, 'the line').replace(/\x60main\x60/g, 'main');
const firstSentence = s => { const m = String(s || '').match(/^[\s\S]*?[.!?](?=\s|$)/); return m ? m[0] : String(s || ''); };

// ---- views -------------------------------------------------------------------------------------------------
const overview = `<section class="view is-active" id="overview" data-title="Overview">
  <div class="card section" data-block="Deliverable" style="margin-top:22px">
    <div class="hero"><div class="num">${overall}<small>%</small>${delta(+overall, +pct1(P.scope.complete, P.scope.total), 'pts')}</div><div class="what"><b style="color:var(--text)">Deliverable.</b> ${esc(R.deliverable)}</div></div>
    <div class="chips" style="margin-top:14px"><span class="chip accent">${S.complete} of ${S.total} tasks complete</span>${delta(S.complete, P.scope.complete)}${delta(S.total, P.scope.total, 'in scope')}<span class="chip plain">${S.onMain} of them on main</span><span class="chip warn">${S.inPr} in pull requests</span><span class="chip ${S.blocked ? 'danger' : 'plain'}">${S.blocked} blocked</span><span class="chip plain">${R.inputs.length} decisions and ${R.actionsOwed.length} actions owed by you</span></div>
  </div>
  <h2 class="section" style="margin-top:30px">The ${EP.length} outcomes</h2>
  <p class="lede" style="margin-bottom:12px">Each opens to its themes, tasks and evidence. Percent and state are from the records on the line.</p>
  <div class="grid outcomes">${EP.map(e => `<a class="card outcome" href="#o-${e.id}" data-view-link="o-${e.id}" data-block="Outcome ${e.id}">
      <div class="head"><span class="p">${pct1(e.counts.complete, e.counts.total)}%</span><span class="t">${esc(e.title)}</span><span class="chip ${e.status === 'in_progress' ? 'accent' : 'plain'}">${esc(epicState(e))}</span><span class="id">${e.id}</span></div>
      ${bar(e.counts)}
      <div class="dl">${esc(clip(firstSentence(plainLine(e.deliverable)), 170))}</div>
      <div class="nums"><span><b>${e.counts.complete}</b> of <b>${e.counts.total}</b> done${delta(e.counts.complete, prevEpic(e.id).done)}</span><span><b>${e.counts.onMain}</b> on main</span><span><b>${e.counts.inPr}</b> in pull requests</span><span>next: ${esc(nextStep(e))}</span></div>
    </a>`).join('')}</div>
  <figure class="fig" data-block="Figure 1, the partition">
    <div class="fig-head"><span class="fig-t">Where the ${S.total} tasks are</span><span class="fig-n">Figure 1</span></div>
    ${partitionWide()}${partitionNarrow()}
    <p style="margin-top:10px;color:var(--text-2)">The hop to main: no pull request from the line into main is open; the line is ${F.gh.ahead} commits ahead of main${delta(F.gh.ahead, P.aheadOfMain)} and ${F.gh.behind} behind.</p>
    <ul class="key"><li><svg viewBox="0 0 22 14"><rect class="st-main" x="1" y="2" width="20" height="10" rx="3"/></svg>on main</li><li><svg viewBox="0 0 22 14"><rect class="st-line" x="1" y="2" width="20" height="10" rx="3"/></svg>complete on the line only</li><li><svg viewBox="0 0 22 14"><rect class="st-pr" x="1" y="2" width="20" height="10" rx="3"/></svg>in a pull request</li><li><svg viewBox="0 0 22 14"><rect class="st-todo" x="1" y="2" width="20" height="10" rx="3"/></svg>not started</li><li><svg viewBox="0 0 22 14"><rect class="st-block" x="1" y="2" width="20" height="10" rx="3"/></svg>blocked</li></ul>
    <figcaption>${esc(R.basis)}</figcaption>
  </figure>
  <p class="meta section" style="margin-bottom:8px"><span>The four streams, from the Coordinator trackers at <b>${esc(R.readAt)}</b></span></p>
  <div class="grid grid-2">${R.streams.map(st => `<div class="card" data-block="${esc(st.title)}"><div style="display:flex;justify-content:space-between;gap:8px;align-items:center"><b>${esc(st.title)}</b>${stepDots(st.step, st.steps)}</div><p class="meta" style="margin:6px 0 0">Step ${st.step} of ${st.steps}</p>${[['done', st.done], ['now', st.now], ['next', st.next]].map(([k, xs]) => `<p class="meta kicker-l">${k}</p><ul class="points" style="margin:0;color:var(--text-2)">${xs.map(x => `<li>${esc(x)}</li>`).join('')}</ul>`).join('')}<div style="margin-top:10px"><span class="chip plain">next move: ${esc(st.owner)}</span></div></div>`).join('')}</div>
  <div class="card section" data-block="Pull requests against the line">
    <div style="display:flex;justify-content:space-between;gap:10px;flex-wrap:wrap;align-items:baseline"><h3>${F.gh.open.length} pull requests open against the line${delta(F.gh.open.length, P.prs.openAgainstLine)}</h3><span style="color:var(--text-2)">${F.gh.mergedToday.filter(p => p.baseRefName.startsWith('integration/EPC-022')).length} landed on the line today, ${F.gh.mergedToday.filter(p => p.baseRefName === 'main').length} on main</span></div>
    <ul class="prs" style="margin-top:12px" aria-label="open pull requests">${F.gh.open.map(p => `<li class="pr"><span class="sr">${p.mergeable === 'MERGEABLE' ? 'mergeable' : p.mergeable === 'CONFLICTING' ? 'conflicts with the line' : 'mergeability unknown on GitHub'}: </span>${mark(prState(p))}<span>${p.number}${p.isDraft ? '<small> draft</small>' : ''}</span></li>`).join('')}</ul>
    <ul class="key"><li>${mark('ok')}mergeable</li><li>${mark('conf')}conflicts with the line</li><li>${mark('unk')}not yet computed by GitHub at ${esc(F.gh.at)}</li></ul>
    <div class="chips" style="margin-top:10px"><span class="chip ok">${F.gh.open.filter(p => p.mergeable === 'MERGEABLE').length} mergeable</span><span class="chip warn">${F.gh.open.filter(p => p.mergeable === 'CONFLICTING').length} conflicting</span><span class="chip plain">${F.gh.open.filter(p => p.mergeable === 'UNKNOWN').length} unknown</span><span class="chip plain">${F.gh.open.filter(p => !p.isDraft).length} marked ready</span></div>
  </div>
</section>`;

function topicView(e) {
  const id = 'o-' + e.id, c = e.counts;
  const themes = Object.entries(e.themes).map(([name, ids]) => ({ name, ids, done: ids.filter(i => T[i].status === 'complete').length, key: `t-${e.id}-${Object.keys(e.themes).indexOf(name)}` }));
  const order = ['ready', 'waiting', 'blocked'].map(s => [s, e.tasks.filter(i => T[i].next && T[i].next.state === s)]);
  const open = e.tasks.filter(i => T[i].status !== 'complete' && T[i].status !== 'cancelled' && !T[i].next);
  const done = e.tasks.filter(i => T[i].status === 'complete');
  const blocked = e.tasks.filter(i => T[i].status === 'blocked' || (T[i].next && T[i].next.state === 'blocked'));
  const prs = F.gh.open.filter(p => e.tasks.some(i => p.headRefName.includes(i)));
  const progress = `<div class="card" data-block="${e.id} what it is" style="margin-bottom:14px"><b>What it is</b><p style="margin-top:6px;color:var(--text-2)">${esc(clip(firstSentence(plainLine(e.deliverable)), 118))}</p><p class="meta" style="margin-top:6px">Source: <code>epics/${e.id}.md</code>, ${esc(e.deliverableSource)}.</p></div>
    <div class="card" data-block="${e.id} progress"><div class="epic"><div class="head"><span class="p">${pct1(c.complete, c.total)}%</span><span class="t">${c.complete} of ${c.total} tasks complete${delta(c.complete, prevEpic(e.id).done)}</span></div>${bar(c)}<div class="nums"><span><b>${c.onMain}</b> on main</span><span><b>${c.complete - c.onMain}</b> on the line only</span><span><b>${c.inPr}</b> in pull requests</span><span><b>${c.todo}</b> not started</span><span><b>${c.blocked}</b> blocked</span><span><b>${c.cancelled}</b> cancelled, not counted</span></div></div></div>
    <h3 class="section" style="margin-top:26px">${themes.length} themes</h3><p class="lede" style="margin-bottom:10px">A theme is the epic record section that introduced its tasks (TSK-1095 AC-15). Each opens to its tasks.</p>
    <div class="vt themes" style="--vt-cols:minmax(0,1.4fr) minmax(120px,1fr) 9ch"><div class="vt-head"><span>Theme</span><span>Done of total</span><span></span></div>${themes.map(t => `<div><a href="#${t.key}" data-view-link="${t.key}">${esc(t.name)}</a><span class="cell-bar"><span class="track"><span class="fill" style="width:${(100 * t.done / t.ids.length).toFixed(0)}%"></span></span><span class="n">${t.done}/${t.ids.length}</span></span><span class="mute">${t.ids.length === t.done ? 'done' : t.ids.filter(i => T[i].next && T[i].next.state === 'ready').length + ' ready'}</span></div>`).join('')}</div>`;
  const orderBody = `<p class="lede" style="margin-bottom:10px">From <code>codeflow work next</code> at ${esc(F.next.at)}: ready first, then waiting with the predecessor each waits on.</p>${order.map(([s, ids]) => ids.length ? `<h3 class="section" style="margin-top:18px">${ids.length} ${s}</h3>${s === 'ready' ? taskTable(ids) : `<div class="vt tasks" style="--vt-cols:12ch 1fr"><div class="vt-head"><span>Task</span><span>Why it waits</span></div>${ids.map(i => `<div><span class="tc">${stMark(T[i])}<a href="#k-${i}" data-view-link="k-${i}" class="mono">${i}</a></span><span class="mute">${esc(clip((T[i].next.reason || '').replace(/^task TSK-\d+ /, ''), 120))}</span></div>`).join('')}</div>`}` : '').join('')}${open.length ? `<h3 class="section" style="margin-top:18px">${open.length} open and not listed by work next</h3>${taskTable(open)}` : ''}<details class="section" style="margin-top:18px"><summary>${done.length} complete tasks</summary>${taskTable(done)}</details>`;
  const issuesBody = `${blocked.length ? `<h3>${blocked.length} blocked</h3>${taskTable(blocked)}` : '<p class="lede">No task of this epic is blocked on the line.</p>'}<h3 class="section" style="margin-top:22px">${prs.length} open pull requests</h3>${prs.length ? `<div class="vt" style="--vt-cols:7ch 1fr auto"><div class="vt-head"><span>PR</span><span>Title</span><span>State</span></div>${prs.map(p => `<div><span class="mono">${p.number}</span><span>${esc(clip(p.title, 90))}</span><span class="chip ${p.mergeable === 'MERGEABLE' ? 'ok' : p.mergeable === 'CONFLICTING' ? 'warn' : 'plain'}">${p.isDraft ? 'draft, ' : ''}${p.mergeable.toLowerCase()}</span></div>`).join('')}</div>` : '<p class="lede">None against the line names a task of this epic.</p>'}`;
  return `<section class="view" id="${id}" data-title="${esc(e.title)}">
  ${crumbs([['overview', 'Release'], [id, e.id]])}
  <div class="vhead"><h2>${esc(e.title)}</h2><span class="chip ${e.status === 'in_progress' ? 'accent' : 'plain'}">${esc(epicState(e))}</span><span class="id">${e.id}</span></div>
  ${strip([['what it is', esc(clip(firstSentence(plainLine(e.deliverable)), 110)), 'w'], ['percent', `<b>${pct1(c.complete, c.total)}%</b>`], ['tasks', `${c.total}`], ['criteria', `${e.criteria}`], ['capabilities', e.capabilities.join(', ') || 'not recorded'], ['next step', esc(nextStep(e))]])}
  ${tabs(id, [['progress', 'Progress', null, progress], ['order', 'Order', readyIn(e), orderBody], ['issues', 'Issues', blocked.length + prs.length, issuesBody]])}
</section>`;
}

const epicsView = `<section class="view" id="epics" data-title="Epics">
  <h2 class="section" style="margin-top:22px">The three epics of 4.0.0</h2>
  <p class="lede" style="margin-bottom:14px">Done, not done and the percent, from each task's record. An epic counts as verified only when its record says so.</p>
  <div class="grid">${EP.map(e => `<div class="card epic" data-block="${e.id}"><div class="head"><span class="p">${pct1(e.counts.complete, e.counts.total)}%</span><a class="t" href="#o-${e.id}" data-view-link="o-${e.id}">${esc(e.title)}</a><span class="id">${e.id}, ${esc(epicState(e))}, created ${esc(e.created)}</span></div>${bar(e.counts)}<div class="nums"><span><b>${e.counts.complete}</b> done</span><span><b>${e.counts.onMain}</b> on main</span><span><b>${e.counts.total - e.counts.complete - e.counts.blocked}</b> not done</span><span><b>${e.counts.inPr}</b> in pull requests</span><span><b>${e.counts.blocked}</b> blocked</span><span><b>${e.counts.total}</b> in all</span></div><div class="chips"><span class="chip warn">epic verification: not recorded on the line</span><span class="chip plain">${e.criteria} epic criteria</span><span class="chip plain">${Object.keys(e.themes).length} themes</span></div><ul class="points"><li class="ok">${esc(clip(firstSentence(plainLine(e.deliverable)), 100))}</li><li class="todo">${esc(nextStep(e))}</li></ul></div>`).join('')}</div>
</section>`;

const blockersView = `<section class="view" id="blockers" data-title="Blockers">
  <h2 class="section" style="margin-top:22px">${R.blockers.length} blockers</h2>
  <p class="lede" style="margin-bottom:14px">Each with who clears it, the recommendation, why, and the alternative. Read from the trackers at ${esc(R.readAt)}.</p>
  <div class="grid">${R.blockers.map((b, i) => `<div class="card blk" data-block="Blocker ${i + 1}"><h3><span class="chip danger">${i + 1}</span>${esc(b.title)}<span class="chip accent plain">clears it: ${esc(b.who)}</span><span class="chip plain">${esc(b.state)}</span></h3><div class="rw rec"><b>Recommend</b><span>${md(b.rec)}</span></div><div class="rw"><b>Why</b><span>${md(b.why)}</span></div><div class="rw"><b>Alternative</b><span>${md(b.alt)}</span></div><div class="rw"><b>Source</b><span class="src">${esc(b.source)}</span></div></div>`).join('')}</div>
</section>`;

const sinceView = `<section class="view" id="since" data-title="What changed">
  <h2 class="section" style="margin-top:22px">What changed since the previous report</h2>
  <p class="lede" style="margin-bottom:14px">Previous report read at <b>${esc(PREV_AT)}</b> (line tip ${esc(P.lineTip)}); this one at <b>${esc(F.read.finished)}</b> (line tip ${esc(F.read.lineTip)}). The same marks sit beside the numbers in every view.</p>
  <div class="vt" style="--vt-cols:1fr 7ch 7ch 9ch"><div class="vt-head"><span>Number</span><span>Was</span><span>Now</span><span>Change</span></div>${changes.map(c => `<div class="${c.prev === c.now ? 'same' : ''}"><a href="#${c.view}" data-view-link="${c.view}">${esc(c.what)}</a><span class="mute">${c.prev ?? 'n/a'}</span><span class="mono">${c.now}</span><span>${c.prev === c.now ? '<span class="mute">same</span>' : delta(c.now, c.prev)}</span></div>`).join('')}</div>
  <h3 class="section" style="margin-top:26px">${F.gh.mergedToday.length} pull requests merged today</h3>
  <div class="vt" style="--vt-cols:7ch 1fr auto"><div class="vt-head"><span>PR</span><span>Title</span><span>Into</span></div>${F.gh.mergedToday.map(p => `<div><span class="mono">${p.number}</span><span>${esc(clip(p.title, 90))}</span><span class="chip ${p.baseRefName === 'main' ? 'ok' : 'accent'}">${esc(p.baseRefName.replace('integration/', '').replace(/-the-.*$/, ''))}</span></div>`).join('')}</div>
  ${newlyComplete ? `<h3 class="section" style="margin-top:26px">${newlyComplete.length} tasks complete now that were not on 11-01</h3>${newlyComplete.length ? taskTable(newlyComplete) : '<p class="lede">None.</p>'}` : ''}
  <h3 class="section" style="margin-top:26px">Decided since</h3>
  <div class="card" data-block="Decided since"><ul class="points">${R.decided.map(x => `<li class="ok"><span><span style="color:var(--text-3)">${esc(x.when)}</span> ${esc(x.what)}</span></li>`).join('')}</ul></div>
  <h3 class="section" style="margin-top:26px">The previous report's blockers today</h3>
  <div class="vt" style="--vt-cols:1fr 1fr"><div class="vt-head"><span>Then (11-01 14:31)</span><span>Now</span></div>${P.blockers.map(b => { const now = b.id === 'b2' ? `${F.gh.open.filter(p => p.mergeable === 'CONFLICTING').length} conflicting; GitHub has not computed ${F.gh.open.filter(p => p.mergeable === 'UNKNOWN').length} of ${F.gh.open.length}` : b.id === 'b3' ? 'unchanged: the EPC-024 tip is still e283948c6 (blocker 3 here)' : b.id === 'b4' ? 'unchanged: the hook binary is still 3.1.0 (blocker 4 here)' : 'not rechecked today; the fetch at 12:22 printed no error to this session'; return `<div><span>${esc(b.title)}</span><span class="mute">${esc(now)}</span></div>`; }).join('')}</div>
</section>`;

const lanesView = `<section class="view" id="lanes" data-title="Lanes">
  <h2 class="section" style="margin-top:22px">${R.lanes.length} lanes in flight</h2>
  <p class="lede" style="margin-bottom:14px">By Coordinator, from the trackers at ${esc(R.readAt)}. A lane is one builder, seat or subagent on one unit.</p>
  ${['Parallel', 'Visual and docs', 'Planning', 'Capabilities'].map(co => { const ls = R.lanes.filter(l => l.coordinator === co); return ls.length ? `<h3 class="section" style="margin-top:18px">${esc(co)} Coordinator</h3><div class="vt lanes" style="--vt-cols:minmax(120px,.8fr) minmax(0,1.6fr) minmax(0,1fr)"><div class="vt-head"><span>Lane</span><span>What</span><span>State and expected</span></div>${ls.map(l => `<div><span><b>${esc(l.lane)}</b><br><span class="mute">${esc(l.who)}</span></span><span class="mute">${esc(clip(l.what, 160))}</span><span><span class="chip ${/wait|idle/.test(l.state) ? 'warn' : /fix/.test(l.state) ? 'danger' : 'accent'}">${esc(clip(l.state, 60))}</span><br><span class="mute">${esc(l.expected)}</span></span></div>`).join('')}</div>` : ''; }).join('')}
</section>`;

const sources = [['Task records on the line', 'git show ' + F.read.lineRef, F.read.finished], ['Task records on origin/main', 'git show origin/main', F.read.finished], ['Record history on the line', 'git log --name-only ' + F.read.lineRef, F.read.finished], ['GitHub, read only', 'gh pr list, gh api compare', F.gh.at], ['codeflow work next', 'in the task worktree', F.next.at], ['Coordinator trackers', 'coordination/coordinators/*/TRACKER.md', R.readAt], ['Previous report', '_src/previous-2026-11-01-1431.json', PREV_AT]];
const measures = [['task counts and percent', [1, 0, 0, 0, 0, 0, 0]], ['on main', [1, 1, 0, 0, 0, 0, 0]], ['in a pull request', [1, 0, 0, 1, 0, 0, 0]], ['last moved', [0, 0, 1, 0, 0, 0, 0]], ['pull request state', [0, 0, 0, 1, 0, 0, 0]], ['ready, waiting, blocked', [0, 0, 0, 0, 1, 0, 0]], ['themes', [1, 0, 0, 0, 0, 0, 0]], ['streams, lanes, blockers', [0, 0, 0, 0, 0, 1, 0]], ['decisions and actions owed', [0, 0, 0, 0, 0, 1, 0]], ['change marks', [1, 0, 0, 1, 0, 0, 1]], ['lane and window per task', [0, 0, 0, 0, 0, 0, 0]]];
function coverageFig() {
  const cw = 92, rh = 30, lx = 230, ty = 70, W = lx + sources.length * cw + 10, H = ty + measures.length * rh + 10; let o = '';
  sources.forEach((s, j) => { const x = lx + j * cw + cw / 2; o += `<text x="${x}" y="${ty - 28}" text-anchor="middle" class="s">${esc(s[0].split(' ')[0])}</text><text x="${x}" y="${ty - 12}" text-anchor="middle" class="s">${esc(s[0].split(' ').slice(1, 3).join(' '))}</text>`; });
  measures.forEach(([m, cells], i) => { const y = ty + i * rh; o += `<text x="${lx - 12}" y="${y + rh / 2 + 4}" text-anchor="end" class="b">${esc(m)}</text>`; const none = cells.every(c => !c); cells.forEach((c, j) => { const x = lx + j * cw + cw / 2; o += c ? `<circle class="ok-fill" cx="${x}" cy="${y + rh / 2}" r="7"/>` : none ? `<path class="ink3" d="M${x - 6} ${y + rh / 2}h12"/>` : `<circle class="unk-ring" cx="${x}" cy="${y + rh / 2}" r="5" style="stroke:var(--fig-ink-3)"/>`; }); if (none) o += `<text x="${lx + sources.length * cw + 4}" y="${y + rh / 2 + 4}" class="s">not read today</text>`; });
  return `<svg viewBox="0 0 ${W + 110} ${H}" role="img" aria-label="Which source each measure was read from"><title>Coverage: measures by sources</title>${o}</svg>`;
}
const evidenceView = `<section class="view" id="evidence" data-title="Evidence">
  <h2 class="section" style="margin-top:22px">Which numbers you can trust, and what was not read</h2>
  <p class="lede" style="margin-bottom:14px">One read from ${esc(F.read.started)} to ${esc(F.read.finished)}; the line at ${esc(F.read.lineTip)}, main at ${esc(F.read.mainTip)}, the installed binary ${esc(F.read.codeflow)}.</p>
  <figure class="fig" data-block="Figure 2, coverage"><div class="fig-head"><span class="fig-t">Measures by sources</span><span class="fig-n">Figure 2</span></div><div class="scroll-x" tabindex="0" role="region" aria-label="Figure 2, coverage matrix">${coverageFig()}</div><ul class="key"><li><svg viewBox="0 0 24 24" class="marks"><circle class="ok-fill" cx="12" cy="12" r="7"/></svg>read from this source</li><li><svg viewBox="0 0 24 24" class="marks"><circle class="unk-ring" cx="12" cy="12" r="5" style="stroke:var(--fig-ink-3)"/></svg>not from this source</li><li><svg viewBox="0 0 24 24" class="marks"><path class="ink3" d="M5 12h14"/></svg>not read today (shown as not recorded)</li></ul><figcaption>Lane and window per task are TSK-1095's fields; until it lands they read <code>not recorded</code> on every task view.</figcaption></figure>
  <div class="vt section" style="--vt-cols:minmax(0,1fr) minmax(0,1.4fr) auto"><div class="vt-head"><span>Source</span><span>How</span><span>Read at</span></div>${sources.map(s => `<div><span><b>${esc(s[0])}</b></span><span class="mute"><code>${esc(s[1])}</code></span><span class="mute">${esc(s[2])}</span></div>`).join('')}</div>
  <div class="note section" data-block="Not verified"><span class="chip warn">not verified</span><span>GitHub had not computed mergeability for ${F.gh.open.filter(p => p.mergeable === 'UNKNOWN').length} of ${F.gh.open.length} open pull requests at ${esc(F.gh.at)}; the Herdr seats and host load were not probed by this page.</span></div>
</section>`;

const inputsView = `<section class="view" id="inputs" data-title="Your decisions">
  <h2 class="section" style="margin-top:22px">${R.inputs.length} decisions for the walk-through, ${R.actionsOwed.length} actions owed</h2>
  <div class="card" data-block="Decided today" style="margin-bottom:14px"><b>Decided today, not asked again</b><ul class="points" style="margin-top:6px">${R.decided.map(x => `<li class="ok"><span><span style="color:var(--text-3)">${esc(x.when)}</span> ${esc(x.what)}</span></li>`).join('')}</ul></div>
  <div class="card" data-block="Actions owed" style="margin-bottom:14px"><b>Actions only you can take</b><ul class="points" style="margin-top:6px">${R.actionsOwed.map(x => `<li class="todo"><span>${esc(x.what)} <span style="color:var(--text-3)">(since ${esc(x.since)})</span></span></li>`).join('')}</ul></div>
  <p class="lede" style="margin-bottom:14px">Each asks one plain question. The recommended answer is marked, never preselected. An answer records your choice and authorises nothing else; the Script view writes exactly what you chose.</p>
  <form class="form" data-answers="tsk-1122-walkthrough" id="decisions">${R.inputs.map((q, i) => `<div class="card" data-block="Decision ${i + 1}"><div class="q">${i + 1}. ${esc(q.title)}</div><div class="hint">${esc(q.context)}</div><div class="opts">${q.options.map(o => `<label class="opt"><input type="radio" name="${q.id}" value="${o.value}" data-label="${esc(o.label)}"><span>${esc(o.label)}</span>${o.recommended ? '<span class="rec">recommended</span>' : '<span></span>'}<span class="cost">cost: ${esc(o.cost)}</span></label>`).join('')}</div><div class="hint" style="margin-top:8px"><b>Why:</b> ${esc(q.reasoning)}</div></div>`).join('')}
    <div class="card" data-block="Named changes"><div class="q">Changes you name</div><div class="hint">Anything the prototype should change before the build tasks start; this goes into the record as the operator's words.</div><textarea name="changes" rows="3" style="margin-top:8px" placeholder="none, or your changes"></textarea></div>
    <div style="display:flex;gap:8px;flex-wrap:wrap;align-items:center"><button class="btn primary" type="submit">Keep answers for this session</button><button class="btn" type="reset">Clear</button><span style="color:var(--text-2)">then copy the script from the Script view</span></div>
  </form>
</section>`;

const scriptView = `<section class="view" id="script" data-title="Script">
  <h2 class="section" style="margin-top:22px">One script: recheck, then record only what you chose</h2>
  <p class="lede" style="margin-bottom:14px">Part 1 rechecks the numbers here, read only, from any folder. Part 2 appears only for decisions you answered, checks the workspace is on <code>integration/workspace</code>, writes your answers to one file and commits that file alone.</p>
  <div class="copy-wrap" data-block="Script"><button class="btn" type="button" data-copy="#script-text">Copy</button><pre tabindex="0" role="region" aria-label="Recheck script"><code id="script-text"></code></pre></div>
</section>`;

const KINDS = ['overview', 'topic', 'item', 'list', 'timeline', 'issues', 'decisions', 'evidence', 'commands'];
const kindsView = `<section class="view" id="kinds" data-title="Report kinds">
  <h2 class="section" style="margin-top:22px">Ten report kinds on one shell</h2>
  <p class="lede" style="margin-bottom:14px">Each kind is a view tree from TSK-1120's registry; the shell, menu, tabs and marks are the same. TSK-1127 ships these as specimens; this page shows only which views each kind declares.</p>
  <div class="vt kinds" style="--vt-cols:minmax(120px,.8fr) minmax(0,2fr) minmax(0,1fr)"><div class="vt-head"><span>Kind</span><span>Views, in menu order</span><span>Leads with</span></div>${R.kinds.map(k => `<div><span><b>${esc(k.kind)}</b>${k.note ? `<br><span class="mute">${esc(k.note)}</span>` : ''}</span><span class="chips">${KINDS.map(v => `<span class="chip ${k.views.includes(v) ? (v === 'overview' ? 'accent' : 'plain') : 'off'}">${v}</span>`).join('')}</span><span class="mute">${esc(k.lead)}</span></div>`).join('')}</div>
  <ul class="key" style="margin-top:12px"><li><span class="chip accent">overview</span>the root view, always first</li><li><span class="chip plain">kind</span>declared</li><li><span class="chip off">kind</span>not in this report kind</li></ul>
</section>`;

function portalFig() {
  const L = R.portalLayers; let o = ''; const W = 640, x0 = 40, w = 560;
  L.forEach((l, i) => { const y = 24 + i * 78; o += `<rect class="${i === 0 ? 'acc-wash' : 'node-soft'}" x="${x0 + i * 24}" y="${y}" width="${w - i * 48}" height="56" rx="10"/><text x="${x0 + i * 24 + 16}" y="${y + 24}" class="b">${esc(l.layer)}</text><text x="${x0 + i * 24 + 16}" y="${y + 44}" class="s">${esc(clip(l.what, 70))}</text>`; if (i) o += `<path class="ink2" d="M${W / 2} ${y - 22}v16" marker-end="url(#arr)"/>`; });
  return `<svg viewBox="0 0 ${W} 262" role="img" aria-label="Three portal layers, overview map over area pages over reference pages"><defs><marker id="arr" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L10 5 0 10z" fill="var(--fig-ink-2)"/></marker></defs>${o}</svg>`;
}
const portalView = `<section class="view" id="portal" data-title="Docs portal layers">
  <h2 class="section" style="margin-top:22px">The docs portal on the same tokens, three layers</h2>
  <p class="lede" style="margin-bottom:14px">Concept, then architecture, then technical (the operator's 12:18 decision of 11-01). Each layer maps onto a shell kind, so a reader moves between the portal and a report without relearning the page.</p>
  <figure class="fig" data-block="Figure 3, portal layers"><div class="fig-head"><span class="fig-t">Overview map, area page, reference page</span><span class="fig-n">Figure 3</span></div>${portalFig()}<figcaption>Every layer is a page kind with the same top bar, menu and marks as this report; TSK-1127 ships one sample of each.</figcaption></figure>
  <div class="vt section" style="--vt-cols:minmax(100px,.6fr) minmax(0,1.4fr) minmax(0,1fr)"><div class="vt-head"><span>Layer</span><span>What it holds</span><span>Same as in a report</span></div>${R.portalLayers.map(l => `<div><span><b>${esc(l.layer)}</b></span><span class="mute">${esc(l.what)}</span><span class="mute">${esc(l.same)}</span></div>`).join('')}</div>
  <div class="note section" data-block="What changed for the portal"><span class="chip plain">What changed</span><span>The portal gets the same What changed page as a report: TSK-1121's per-view marks, against the previous build of the site.</span></div>
</section>`;

// ---- menu -------------------------------------------------------------------------------------------------
const dot = id => viewsChanged.has(id) ? '<span class="chg" title="changed since the previous report"></span>' : '';
const row = (id, label, count, icon) => `<a class="row" data-icon="${icon || id}" href="#${id}" data-view-link="${id}"><span class="label">${esc(label)}</span>${dot(id)}${count !== '' && count != null ? `<span class="count">${count}</span>` : ''}</a>`;
const menu = `<section class="area" data-area="release" data-title="Where the release stands" data-icon="status">
        ${row('overview', 'Overview', '', 'overview')}
        <span class="grp">Outcomes</span>
        ${EP.map(e => row('o-' + e.id, e.title, pct1(e.counts.complete, e.counts.total) + '%', 'outcome')).join('\n        ')}
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
        ${row('portal', 'Docs portal layers', R.portalLayers.length, 'docs')}</section>
      <section class="area" data-area="earlier" data-title="Earlier reports" data-icon="history" data-util>
        <span class="row" data-icon="history" style="cursor:default" title="The earlier page is no longer published"><span class="label">${esc(PREV_AT)}, earlier form (the earlier page is no longer published)</span></span></section>`;

// ---- page --------------------------------------------------------------------------------------------------
const DATA = { tasks: T, epics: Object.fromEntries(EP.map(e => [e.id, { id: e.id, title: e.title, themes: e.themes, deliverable: e.deliverable }])), lineRef: F.read.lineRef, prevAt: PREV_AT, readAt: F.read.finished, next: F.next };
const localStyle = `
.notice{display:flex;flex-wrap:wrap;gap:8px 14px;align-items:center;margin:12px 0 0;padding:10px 14px;border-radius:var(--r-2);background:var(--accent-wash);color:var(--accent-ink);font-size:var(--fs-1)}
.delta{display:inline-flex;align-items:center;height:20px;padding:0 7px;margin-left:6px;border-radius:var(--r-pill);font-size:var(--fs-0);font-weight:600;letter-spacing:0;font-variant-numeric:tabular-nums;vertical-align:middle;background:var(--accent-wash);color:var(--accent-ink)}
.delta.down{background:var(--sunken);color:var(--text-2)} .hero .num .delta{font-size:var(--fs-0);margin-left:10px}
.chg{display:inline-block;width:7px;height:7px;border-radius:50%;background:var(--accent);margin-right:6px;flex:none}
.outcomes{gap:12px} a.outcome{display:grid;gap:10px;color:inherit;text-decoration:none} a.outcome:hover{text-decoration:none;background:var(--raised);box-shadow:0 0 0 1px var(--accent),var(--shadow-1)}
.outcome .head,.epic .head,.vhead{display:flex;flex-wrap:wrap;align-items:baseline;gap:6px 14px} .vhead{margin:14px 0 12px}
.outcome .head .p,.epic .head .p{font-size:var(--fs-4);font-weight:600;font-variant-numeric:tabular-nums;letter-spacing:-.02em} .outcome .head .t,.epic .head .t{font-weight:600;color:var(--text)} .outcome .head .id,.epic .head .id,.vhead .id{color:var(--text-3);font-size:var(--fs-1)}
.outcome .dl{color:var(--text-2);font-size:var(--fs-1)} .outcome .nums,.epic .nums{display:flex;flex-wrap:wrap;gap:6px 14px;color:var(--text-2);font-size:var(--fs-1)} .outcome .nums b,.epic .nums b{color:var(--text);font-weight:600;font-variant-numeric:tabular-nums}
.epic{display:grid;gap:10px}
.crumbs{display:flex;flex-wrap:wrap;gap:4px 8px;align-items:center;margin-top:18px;font-size:var(--fs-1);color:var(--text-2)} .crumbs .sep{color:var(--text-3)} .crumbs [aria-current]{color:var(--text);font-weight:500}
.strip{display:flex;flex-wrap:wrap;gap:10px 22px;margin:0 0 6px;padding:12px 16px;border-radius:var(--r-2);background:var(--surface);box-shadow:var(--shadow-1)} .strip>div{display:grid;gap:2px;min-width:0;max-width:100%} .strip dt{font-size:var(--fs-0);font-weight:600;letter-spacing:.04em;text-transform:uppercase;color:var(--text-3)} .strip dd{margin:0;font-size:var(--fs-1);color:var(--text-2);overflow-wrap:anywhere} .strip dd.nr{color:var(--text-3);font-style:italic} .strip dd.w{max-width:48ch}
.tabpanel[hidden]{display:none} .tab .count{color:var(--text-3);font-size:var(--fs-0);font-weight:600}
.vt.tasks>div{grid-template-columns:var(--vt-cols)} .tc{display:inline-flex;align-items:center;gap:8px} .tt{color:var(--text-2);min-width:0;overflow-wrap:anywhere}
.sm{display:inline-block;width:12px;height:12px;border-radius:3px;flex:none} .sm.st-main{background:var(--text)} .sm.st-line{background:var(--accent)} .sm.st-pr{background:var(--fig-pr);box-shadow:inset 0 0 0 1.2px var(--accent)} .sm.st-todo{background:var(--fig-fill);box-shadow:inset 0 0 0 1.2px var(--fig-outline)} .sm.st-block{background:var(--danger)}
.vt .same{color:var(--text-3)} .vt .src{color:var(--text-3);font-size:var(--fs-0)}
.blk{display:grid;gap:10px} .blk h3{display:flex;gap:10px;align-items:center;flex-wrap:wrap} .blk .rw{display:grid;grid-template-columns:12ch 1fr;gap:10px;font-size:var(--fs-2)} .blk .rw b{color:var(--text-2);font-weight:600;font-size:var(--fs-1);padding-top:2px} .blk .rw span{color:var(--text-2)} .blk .rw.rec span{color:var(--text)}
.opt .cost{grid-column:2/4;color:var(--text-2);font-size:var(--fs-1)} .opt{grid-template-columns:18px 1fr auto}
.prs{list-style:none;margin:0;padding:0;display:flex;flex-wrap:wrap;gap:8px 16px} .pr{display:inline-flex;align-items:center;gap:5px;font-size:var(--fs-1);color:var(--text-2);font-variant-numeric:tabular-nums} .pr small{color:var(--text-3)} .pr svg,.key .marks{width:20px;height:20px}
.sr{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap}
.kicker-l{margin:10px 0 2px;text-transform:uppercase;letter-spacing:.04em;font-size:12px}
.chip.off{background:transparent;color:var(--text-3);box-shadow:inset 0 0 0 1px var(--line)} .chip.off::before{display:none}
.scroll-x{overflow-x:auto} .scroll-x svg{min-width:560px}
.vt .chip{white-space:normal;height:auto;min-height:24px;padding:3px 9px;line-height:1.25;text-align:left}
details>summary{cursor:pointer;color:var(--text-2);font-weight:500;padding:6px 0} details[open]>summary{margin-bottom:10px}
.fig .narrow text{font-size:13.5px}.fig .narrow text.s{font-size:13px}
.about{margin-top:14px} .about p{color:var(--text-2)}
@media (max-width:640px){.blk .rw{grid-template-columns:1fr;gap:2px} .vt.tasks>div{grid-template-columns:1fr!important} .strip{gap:8px 16px}}`;

const render = `
(() => {
  const D = ${JSON.stringify(DATA)}; const T = D.tasks, E = D.epics;
  const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const clip = (s, n = 150) => { s = String(s || '').replace(/\\s+/g, ' ').trim(); if (s.length <= n) return s; const c = s.slice(0, n); return c.slice(0, Math.max(c.lastIndexOf(' '), 40)) + ' …'; };
  const page = document.querySelector('.page');
  const crumbs = parts => '<nav class="crumbs" aria-label="Breadcrumb">' + parts.map((p, i) => i < parts.length - 1 ? '<a href="#' + p[0] + '" data-view-link="' + p[0] + '">' + esc(p[1]) + '</a><span class="sep" aria-hidden="true">/</span>' : '<span aria-current="page">' + esc(p[1]) + '</span>').join('') + '</nav>';
  const strip = facts => '<dl class="strip" aria-label="Context">' + facts.map(([k, v, cls]) => '<div><dt>' + esc(k) + '</dt><dd class="' + (cls || '') + '">' + v + '</dd></div>').join('') + '</dl>';
  const sm = t => t.status === 'complete' ? (t.onMain ? '<i class="sm st-main"></i>' : '<i class="sm st-line"></i>') : t.status === 'blocked' ? '<i class="sm st-block"></i>' : t.location === 'pull request' ? '<i class="sm st-pr"></i>' : '<i class="sm st-todo"></i>';
  const loc = t => '<span class="chip ' + (t.onMain ? 'plain' : t.status === 'complete' ? 'accent' : t.location === 'pull request' ? 'warn' : t.status === 'blocked' ? 'danger' : 'plain') + '">' + esc(t.location) + (t.prs.length ? ' ' + t.prs.map(n => '#' + n).join(' ') : '') + '</span>';
  const link = i => T[i] ? '<a href="#k-' + i + '" data-view-link="k-' + i + '" class="mono">' + i + '</a>' : '<span class="mono">' + esc(i) + '</span> <span class="mute">(not on the line)</span>';
  const table = ids => '<div class="vt tasks" style="--vt-cols:12ch 1fr auto"><div class="vt-head"><span>Task</span><span>Title</span><span>Location</span></div>' + ids.map(i => '<div><span class="tc">' + sm(T[i]) + link(i) + '</span><span class="tt">' + esc(T[i].title) + '</span>' + loc(T[i]) + '</div>').join('') + '</div>';
  const themeKey = (e, name) => 't-' + e + '-' + Object.keys(E[e].themes).indexOf(name);
  function make(id) {
    if (document.getElementById(id)) return;
    let html = null, title = '';
    const tm = id.match(/^t-(EPC-\\d+)-(\\d+)$/), km = id.match(/^k-(TSK-\\d+)$/);
    if (tm && E[tm[1]]) { const e = E[tm[1]], name = Object.keys(e.themes)[+tm[2]], ids = e.themes[name]; if (!name) return; title = name;
      const done = ids.filter(i => T[i].status === 'complete').length, ready = ids.filter(i => T[i].next && T[i].next.state === 'ready');
      html = crumbs([['overview', 'Release'], ['o-' + e.id, e.id], [id, name]]) + '<div class="vhead"><h2>' + esc(name) + '</h2><span class="id">theme of ' + e.id + '</span></div>'
        + strip([['outcome', '<a href="#o-' + e.id + '" data-view-link="o-' + e.id + '">' + esc(e.title) + '</a>'], ['tasks', String(ids.length)], ['done', done + ' of ' + ids.length], ['ready to start', String(ready.length)], ['introduced by', 'the epic record section of this name']])
        + '<h3 class="section" style="margin-top:22px">' + ids.length + ' tasks</h3>' + table(ids); }
    else if (km && T[km[1]]) { const t = T[km[1]], e = E[t.epic]; title = t.id;
      const parts = [['overview', 'Release']]; if (e) { parts.push(['o-' + e.id, e.id]); parts.push([themeKey(e.id, t.theme), clip(t.theme, 48)]); } parts.push([id, t.id]);
      const nr = '<span class="nr">not recorded</span>';
      html = crumbs(parts) + '<div class="vhead"><h2>' + esc(t.title) + '</h2><span class="chip ' + (t.status === 'complete' ? 'ok' : t.status === 'blocked' ? 'danger' : 'plain') + '">' + esc(t.status) + '</span><span class="id">' + t.id + ', ' + esc(t.work_type) + ', created ' + esc(t.created) + '</span></div>'
        + strip([['outcome', e ? '<a href="#o-' + e.id + '" data-view-link="o-' + e.id + '">' + esc(e.title) + '</a>' : nr], ['theme', e ? '<a href="#' + themeKey(e.id, t.theme) + '" data-view-link="' + themeKey(e.id, t.theme) + '">' + esc(clip(t.theme, 60)) + '</a>' : nr], ['location', loc(t)], ['state', t.next ? esc(t.next.state) + (t.next.state === 'ready' ? ', unclaimed' : '') : t.status === 'complete' ? 'landed' : esc(t.status)], ['lane', nr, 'nr'], ['window', nr, 'nr'], ['last moved', esc(t.lastMoved)], ['pull request', t.prs.length ? t.prs.map(n => '<a href="https://github.com/sathyassn/codeflow/pull/' + n + '">#' + n + '</a>').join(' ') : 'none open']])
        + '<div class="card about" data-block="' + t.id + ' about"><b>What it is about</b><p style="margin-top:6px">' + esc(clip(t.about, 118)) + '</p>' + (t.about.length > 118 ? '<p class="meta" style="margin-top:6px">The rest is in the record\\'s Outcome section (Evidence below).</p>' : '') + '</div>'
        + '<div class="grid grid-2 section" style="margin-top:14px">'
        + '<div class="card" data-block="' + t.id + ' criteria"><b>Criteria and status</b><div class="chips" style="margin-top:8px"><span class="chip plain">' + t.criteria + ' acceptance criteria</span>' + t.serves.map(s => '<span class="chip plain">serves ' + esc(s) + '</span>').join('') + '</div>' + (t.next && t.next.reason ? '<p class="meta" style="margin-top:10px">' + esc(clip(t.next.reason.replace(/^task TSK-\\d+ /, 'This task '), 140)) + '</p>' : '') + '</div>'
        + '<div class="card" data-block="' + t.id + ' depends"><b>Depends on</b><div class="chips" style="margin-top:8px">' + (t.depends_on.length ? t.depends_on.map(d => '<span class="chip ' + (T[d] && T[d].status === 'complete' ? 'ok' : 'plain') + '">' + link(d) + '</span>').join('') : '<span class="mute">a root: no predecessor</span>') + '</div></div>'
        + '</div>'
        + '<div class="card section" data-block="' + t.id + ' evidence" style="margin-top:14px"><b>Evidence</b><div class="vt" style="margin-top:8px;--vt-cols:11ch 1fr"><div><span class="mute">record</span><span><code>project-management/tasks/' + t.id + '.md</code> on the line</span></div><div><span class="mute">read at</span><span class="mute">' + esc(D.readAt) + '</span></div><div><span class="mute">location</span><span class="mute">from origin/main and the open pull requests' + (t.prs.length ? ': ' + t.prs.map(n => '<a href="https://github.com/sathyassn/codeflow/pull/' + n + '">#' + n + '</a>').join(', ') : '') + '</span></div></div><pre style="margin-top:10px" tabindex="0" role="region" aria-label="Evidence command"><code>git -C codeflow show ' + esc(D.lineRef) + ':project-management/tasks/' + t.id + '.md</code></pre></div>'; }
    if (html == null) return;
    const s = document.createElement('section'); s.className = 'view'; s.id = id; s.dataset.title = title; s.innerHTML = html; page.appendChild(s);
  }
  window.cfMake = make;
  if (location.hash.length > 1) make(location.hash.slice(1));
  document.addEventListener('click', e => { const a = e.target.closest('[data-view-link]'); if (a) make(a.dataset.viewLink); }, true);
  // tabs inside a view
  document.addEventListener('click', e => { const b = e.target.closest('.tab[role=tab]'); if (!b) return; const list = b.closest('[role=tablist]'); list.querySelectorAll('[role=tab]').forEach(x => { const on = x === b; x.setAttribute('aria-selected', String(on)); const p = document.getElementById(x.getAttribute('aria-controls')); if (p) p.hidden = !on; }); });
  document.addEventListener('keydown', e => { const b = e.target.closest('.tab[role=tab]'); if (!b || (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft')) return; const tabs = [...b.closest('[role=tablist]').querySelectorAll('[role=tab]')]; const n = tabs[(tabs.indexOf(b) + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length]; n.focus(); n.click(); e.preventDefault(); });
})();`;

const scriptJs = `
(() => {
  const RECHECK = ${JSON.stringify(R.recheck)}, FILE = ${JSON.stringify(R.answersFile)}, AS_OF = ${JSON.stringify(F.read.finished)};
  const form = document.getElementById('decisions'), out = document.getElementById('script-text');
  const q = s => "'" + String(s).replace(/'/g, "'\\\\''") + "'";
  function build() {
    const answers = [...form.querySelectorAll('input[type=radio]:checked')].map(i => ({ id: i.name, value: i.value, label: i.dataset.label }));
    const changes = (form.querySelector('textarea[name=changes]').value || '').trim();
    let t = '#!/bin/sh\\n# CodeFlow 4.0.0 status prototype (TSK-1122), ' + AS_OF + '. Part 1 rechecks (read only). Part 2 records only the decisions answered on the page.\\nset -eu\\nW=/Volumes/DATA/Local/software-workspace/projects/agentic-systems\\n\\n# Part 1 of 2: recheck\\n' + RECHECK + '\\n\\n# Part 2 of 2: your answers\\n';
    if (!answers.length && !changes) t += "echo 'No decision was answered on the page: nothing is written, nothing is committed.'\\n";
    else {
      t += 'F=' + FILE + '\\n[ "$(git -C "$W" rev-parse --abbrev-ref HEAD)" = "integration/workspace" ] || { echo "the workspace is not on integration/workspace; nothing written"; exit 1; }\\nmkdir -p "$W/$(dirname "$F")"\\n[ -f "$W/$F" ] || printf \\'%s\\\\n\\' \\'# Operator answers to the TSK-1122 prototype walk-through\\' \\'\\' > "$W/$F"\\n';
      for (const a of answers) t += 'printf \\'%s\\\\n\\' ' + q('- ' + AS_OF + ', ' + a.id + ': ' + a.value + ' (' + a.label + ')') + ' >> "$W/$F"\\n';
      if (changes) t += 'printf \\'%s\\\\n\\' ' + q('- ' + AS_OF + ', changes named: ' + changes) + ' >> "$W/$F"\\n';
      t += 'git -C "$W" add -- "$F"\\ngit -C "$W" commit -m "docs(coordination): record the operator\\'s TSK-1122 walk-through answers" -- "$F"\\necho "Recorded ' + (answers.length + (changes ? 1 : 0)) + ' line(s) in $F on integration/workspace; nothing else changed."\\n';
    }
    out.textContent = t;
  }
  form.addEventListener('change', build); form.addEventListener('input', build); form.addEventListener('reset', () => setTimeout(build)); build();
})();`;

const html = `<!doctype html>
<html lang="en" data-theme="graphite">
<head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>CodeFlow 4.0.0 status prototype</title>
<meta name="description" content="TSK-1122: a clickable prototype of the status dashboard on real 4.0.0 facts read at ${esc(F.read.finished)}: outcomes first, a two-level side menu, tabs, drill-down to epic, theme, task and evidence, decisions as form controls, one script, change marks.">
<script>(()=>{try{const m=localStorage.getItem('cf4.mode'),t=localStorage.getItem('cf4.theme'),a=localStorage.getItem('cf4.accent');const r=document.documentElement;if(m&&m!=='system')r.setAttribute('data-mode',m);if(t)r.setAttribute('data-theme',t);if(a)r.setAttribute('data-accent',a);}catch(e){}})();</script>
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
    <div class="notice" data-block="Prototype notice"><span class="chip accent">prototype</span><span>A prototype of the design for TSK-1122, built outside the runtime. Real facts, read once at <b>${esc(F.read.finished)}</b>; not a report to act on.</span></div>
    <div class="meta"><span>line tip <b>${esc(F.read.lineTip)}</b>, main <b>${esc(F.read.mainTip)}</b></span><span>records on both refs, GitHub (read only), <code>codeflow work next</code> and the four Coordinator trackers</span><span>previous report <b>${esc(PREV_AT)}</b></span></div>
    ${overview}
    ${EP.map(topicView).join('\n')}
    ${epicsView}
    ${blockersView}
    ${sinceView}
    ${lanesView}
    ${evidenceView}
    ${inputsView}
    ${scriptView}
    ${kindsView}
    ${portalView}
  </div></main>
</div>
<aside class="rail" aria-label="Comments"><b>Comments</b> <span style="color:var(--text-2)">click a block to mark it</span><ol></ol></aside>
<script>${kit('cf-icons.js')}</script>
<script>${render}</script>
<script>${kit('cf.js')}</script>
<script>${scriptJs}</script>
</body></html>
`;
fs.writeFileSync(OUT, html);
// the same page with the kit files linked, beside them, for the workspace nav test (it looks for an external cf.js)
const split = html.replace('<style>' + kit('cf.css') + '</style>', '<link rel="stylesheet" href="cf.css">').replace('<script>' + kit('cf-icons.js') + '</script>', '<script src="cf-icons.js"></script>').replace('<script>' + kit('cf.js') + '</script>', '<script src="cf.js"></script>');
fs.writeFileSync(path.join(HERE, 'kit', 'index.html'), split);
console.log('wrote', OUT, html.length, 'bytes;', Object.keys(T).length, 'tasks,', EP.map(e => `${e.id}: ${Object.keys(e.themes).length} themes`).join(', '), ';', changed.length, 'changed numbers');
