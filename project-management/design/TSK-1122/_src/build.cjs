#!/usr/bin/env node
// TSK-1122 prototype builder: one self-contained page (kit CSS and JS inlined, facts embedded) from _src/facts.json
// (every number, the record texts), _src/report.json (prose facts with sources, short names, the kinds) and
// _src/previous-2026-11-01-1431.json (the change marks' baseline). Usage: node build.cjs   -> ../prototype.html
// Rules this page follows (the operator, 2026-11-02 about 16:10 EST): every epic, task, capability and issue shows its
// name in words first and its id after; an epic opens with what it delivers, for whom and why now, never cut; every time
// is wall clock with its zone; the banner says what the page is in words.
const fs = require('fs'); const path = require('path');
const HERE = __dirname, OUT = path.resolve(HERE, '../prototype.html');
const F = JSON.parse(fs.readFileSync(path.join(HERE, 'facts.json'), 'utf8'));
const R = JSON.parse(fs.readFileSync(path.join(HERE, 'report.json'), 'utf8'));
const P = JSON.parse(fs.readFileSync(path.join(HERE, 'previous-2026-11-01-1431.json'), 'utf8'));
const kit = n => fs.readFileSync(path.join(HERE, 'kit', n), 'utf8');

// ---- helpers shared with the page runtime (injected by source, so the page builds a task view the same way) -------
function esc(s) { return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }
function when(s) { // "2026-11-02 13:41 EST" -> "13:41 EST, 2 Nov"; anything else is returned as it is
  const m = String(s || '').match(/^(\d{4})-(\d\d)-(\d\d)[ T](\d\d:\d\d)(?::\d\d)?\s*([A-Z]{2,4})?$/); if (!m) return String(s || '');
  const MON = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  return `${m[4]} ${m[5] || 'EST'}, ${+m[3]} ${MON[+m[2] - 1]}${m[1] === '2026' ? '' : ' ' + m[1]}`;
}
function day(s) { const m = String(s || '').match(/^(\d{4})-(\d\d)-(\d\d)/); if (!m) return String(s || ''); const MON = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']; return `${+m[3]} ${MON[+m[2] - 1]} ${m[1]}`; }
function sentences(s, n) { const t = String(s || '').replace(/\s+/g, ' ').trim(); const m = t.match(/.*?[.!?](?=\s+[A-Z(`"]|$)/g); return m && m.length > n ? m.slice(0, n).map(x => x.trim()).join(' ') : t; }
function crumbs(parts) { return `<nav class="crumbs" aria-label="Breadcrumb">${parts.map((p, i) => i < parts.length - 1 ? `<a href="#${p[0]}" data-view-link="${p[0]}">${esc(p[1])}</a><span class="sep" aria-hidden="true">/</span>` : `<span aria-current="page">${esc(p[1])}</span>`).join('')}</nav>`; }
function strip(facts) { return `<dl class="strip" aria-label="Context">${facts.map(([k, v, cls]) => `<div><dt>${esc(k)}</dt><dd class="${cls || ''}">${v}</dd></div>`).join('')}</dl>`; }
// a visual table: a grid with table semantics; each cell names its column, which the phone layout shows as a label
function vt(o) { const h = o.head || []; return `<div class="vt ${o.cls || ''}" role="table"${o.label ? ` aria-label="${esc(o.label)}"` : ''} style="--vt-cols:${o.cols}">${h.length ? `<div class="vt-head" role="row">${h.map(x => `<span role="columnheader">${esc(x)}</span>`).join('')}</div>` : ''}${o.rows.map(r => `<div role="row"${r.cls ? ` class="${r.cls}"` : ''}>${r.cells.map((c, i) => `<span role="cell" data-l="${esc(h[i] || '')}">${c}</span>`).join('')}</div>`).join('')}</div>`; }
function idTag(id) { return `<span class="id">${esc(id)}</span>`; }

const md = s => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>');
const pct1 = (a, b) => b ? (Math.round(1000 * a / b) / 10).toFixed(1) : '0.0';
const S = F.scope, overall = pct1(S.complete, S.total), PREV_AT = P.snapshotAt, EP = Object.values(F.epics), T = F.tasks, CAPS = F.capabilities || {};
const NB = Object.values(T).filter(t => t.next && t.next.state === 'blocked').length;
const READ_AT = when(F.read.finished), PREV_W = when(PREV_AT), TRACK_AT = when(R.readAt);

// names in words before ids: the short phrase from report.json, else the record title on the line
const nameOf = id => R.names[id] || (T[id] && T[id].title) || (F.epics[id] && F.epics[id].title) || (CAPS[id]) || null;
const named = text => { const t = String(text || ''); return t.replace(/\b((?:TSK|EPC|CAP|SPC)-\d+)\b( AC-\d+)?/g, (m, id, ac, at) => { const before = t.slice(0, at); const depth = (before.match(/\(/g) || []).length - (before.match(/\)/g) || []).length; if (depth > 0 || t[at + m.length] === ')') return m; let n = nameOf(id); if (!n) return m; if (/\bthe $/i.test(before) && /^the /i.test(n)) n = n.slice(4); return `${n} (${id}${ac || ''})`; }); };
const mdn = s => md(named(s));
const capChips = ids => ids.length ? ids.map(c => `<span class="chip plain">${esc(CAPS[c] || 'capability')} ${idTag(c)}</span>`).join('') : '<span class="mute">not recorded</span>';
const taskName = (id, link = true) => T[id] ? `${link ? `<a href="#k-${id}" data-view-link="k-${id}">${esc(T[id].title)}</a>` : esc(T[id].title)} ${idTag(id)}` : `${esc(nameOf(id) || 'a task not on the line')} ${idTag(id)}`;
const epicName = e => `${esc(e.title)} ${idTag(e.id)}`;

// ---- change marks since the previous report -----------------------------------------------------------------
const delta = (now, prev, label = '') => { if (prev == null || now === prev) return ''; const d = Math.round((now - prev) * 10) / 10; return `<span class="delta ${d > 0 ? 'up' : 'down'}" title="was ${prev} at ${esc(PREV_W)}">${d > 0 ? '+' : ''}${d}${label ? ' ' + esc(label) : ''}</span>`; };
const prevEpic = id => P.epics.find(e => e.id === id) || {};
const changes = [];
changes.push({ what: 'tasks in scope', prev: P.scope.total, now: S.total, view: 'overview' });
changes.push({ what: 'tasks complete', prev: P.scope.complete, now: S.complete, view: 'overview' });
changes.push({ what: 'complete on main', prev: P.scope.onMain, now: S.onMain, view: 'overview' });
changes.push({ what: 'tasks in a pull request', prev: P.scope.inPr, now: S.inPr, view: 'overview' });
changes.push({ what: 'pull requests open against the line', prev: P.prs.openAgainstLine, now: F.gh.open.length, view: 'overview' });
changes.push({ what: 'commits the line is ahead of main', prev: P.aheadOfMain, now: F.gh.ahead, view: 'overview' });
for (const e of EP) { const p = prevEpic(e.id); changes.push({ what: `tasks of ${e.title} (${e.id})`, prev: p.total, now: e.counts.total, view: 'o-' + e.id }); changes.push({ what: `complete in ${e.title} (${e.id})`, prev: p.done, now: e.counts.complete, view: 'o-' + e.id }); }
const changed = changes.filter(c => c.prev !== c.now);
const prevComplete = new Set();
try { const lc = JSON.parse(fs.readFileSync(path.join(HERE, 'previous-live-counts-2026-11-01-1431.json'), 'utf8')); for (const e of Object.values(lc.epics)) for (const i of e.completeIds) prevComplete.add(i); } catch {}
const newlyComplete = prevComplete.size ? Object.keys(T).filter(i => T[i].status === 'complete' && !prevComplete.has(i)).sort() : null;
const viewsChanged = new Set(changed.map(c => c.view));

// ---- figures -----------------------------------------------------------------------------------------------
// Every information-bearing figure has a name (title), a description that states each drawn fact, and the same facts
// as a table under "The figure as a table".
const stations = [['on main', S.onMain, 'st-main'], ['complete on the line only', S.complete - S.onMain, 'st-line'], ['in a pull request', S.inPr, 'st-pr'], ['todo, no open PR', S.todo, 'st-todo'], ['blocked in the record', S.blocked, 'st-block']];
const figA11y = (id, title, desc) => `role="img" aria-labelledby="${id}-t" aria-describedby="${id}-d"><title id="${id}-t">${esc(title)}</title><desc id="${id}-d">${esc(desc)}</desc>`;
const dataTable = (head, rows, label) => `<details class="fig-data"><summary>The figure as a table</summary><table aria-label="${esc(label)}"><thead><tr>${head.map(h => `<th scope="col">${esc(h)}</th>`).join('')}</tr></thead><tbody>${rows.map(r => `<tr>${r.map((c, i) => i ? `<td>${c}</td>` : `<th scope="row">${c}</th>`).join('')}</tr>`).join('')}</tbody></table></details>`;
const partDesc = `${S.total} tasks in the three epics: ${stations.map(([l, v]) => `${v} ${l}`).join(', ')}; ${S.complete} complete, ${overall} percent.`;
function partitionWide() {
  const W = 1000, x0 = 20, bw = 960, y = 46, h = 34; let x = x0, o = '';
  for (const [, v, c] of stations) { const w = v / S.total * bw; if (v > 0) o += `<rect class="${c}" x="${(x + 1).toFixed(1)}" y="${y}" width="${Math.max(w - 2, 2).toFixed(1)}" height="${h}" rx="3"/>`; x += w; }
  x = x0; const labels = []; for (const [l, v] of stations) { const w = v / S.total * bw; if (v > 0) labels.push({ l, v, cx: x + w / 2, tw: String(v).length * 9 + 6 + l.length * 7.2 }); x += w; }
  let lx = x0; const ty = y + h + 44, mid = y + h + 14;
  for (const a of labels) { const tx = Math.max(lx, Math.min(a.cx - a.tw / 2, W - 20 - a.tw)); const lc = tx + a.tw / 2; lx = tx + a.tw + 18; o += `<path class="ink2" d="M${a.cx.toFixed(1)} ${y + h + 2}V${mid}H${lc.toFixed(1)}V${ty - 15}"/><text x="${tx.toFixed(1)}" y="${ty}" class="b">${a.v}</text><text x="${(tx + String(a.v).length * 9 + 6).toFixed(1)}" y="${ty}" class="s">${a.l}</text>`; }
  o += `<text x="${x0}" y="26" class="b">${S.total} tasks in the three epics</text><text x="${W - 20}" y="26" text-anchor="end" class="s">${S.complete} complete, ${overall}%</text>`;
  return `<svg class="wide" viewBox="0 0 ${W} ${ty + 10}" ${figA11y('p1', `Where the ${S.total} tasks are, as one bar split by stage`, partDesc)}${o}</svg>`;
}
function partitionNarrow() {
  const W = 330, x0 = 10, bw = 310, y = 30, h = 28; let x = x0, o = '';
  for (const [, v, c] of stations) { const w = v / S.total * bw; if (v > 0) o += `<rect class="${c}" x="${(x + 1).toFixed(1)}" y="${y}" width="${Math.max(w - 2, 2).toFixed(1)}" height="${h}" rx="3"/>`; x += w; }
  o += stations.map(([l, v, c], i) => `<rect class="${c}" x="${x0}" y="${80 + i * 26}" width="14" height="14" rx="3"/><text x="${x0 + 22}" y="${92 + i * 26}" class="b">${v}</text><text x="${x0 + 22 + String(v).length * 9 + 6}" y="${92 + i * 26}" class="s">${l}</text>`).join('');
  o += `<text x="${x0}" y="18" class="b">${S.total} tasks, ${overall}% complete</text>`;
  return `<svg class="narrow" viewBox="0 0 ${W} 222" ${figA11y('p1n', `Where the ${S.total} tasks are, as one bar with its key below`, partDesc)}${o}</svg>`;
}
const bar = (c, w = 400) => { const tot = c.total || 1; return `<svg class="marks" viewBox="0 0 ${w} 22" style="width:100%;height:auto;display:block" role="img" aria-label="${c.complete} of ${c.total} complete, ${c.onMain} on main, ${c.inPr} in a pull request, ${c.blocked} blocked"><rect class="st-todo" x="0" y="4" width="${w}" height="14" rx="4"/><rect class="st-line" x="0" y="4" width="${(w * c.complete / tot).toFixed(1)}" height="14" rx="4"/><rect class="st-main" x="0" y="4" width="${(w * c.onMain / tot).toFixed(1)}" height="14" rx="4"/>${c.inPr ? `<rect class="st-pr" x="${(w * c.complete / tot + 2).toFixed(1)}" y="4" width="${Math.max(w * c.inPr / tot - 2, 2).toFixed(1)}" height="14" rx="3"/>` : ''}${c.blocked ? `<rect class="st-block" x="${(w - 2 - w * c.blocked / tot).toFixed(1)}" y="4" width="${Math.max(w * c.blocked / tot, 3).toFixed(1)}" height="14" rx="2"/>` : ''}</svg>`; };
const stepDots = (n, of) => `<span class="steps" role="img" aria-label="step ${n} of ${of}">${Array.from({ length: of }, (_, i) => `<i class="${i + 1 < n ? 'done' : i + 1 === n ? 'now' : ''}"></i>`).join('')}</span>`;
const MARK = { ok: '<circle class="ok-fill" cx="12" cy="12" r="9"/><path style="stroke:var(--ground);fill:none;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round" d="M7.5 12l3 3 6-6"/>', conf: '<rect class="warn-fill" x="5" y="5" width="14" height="14" rx="3" transform="rotate(45 12 12)"/><path style="stroke:var(--ground);fill:none;stroke-width:1.6;stroke-linecap:round" d="M12 7.5v4m0 0l-3 3.5m3-3.5l3 3.5"/>', unk: '<circle class="unk-ring" cx="12" cy="12" r="8.5"/><path style="stroke:var(--fig-ink-2);fill:none;stroke-width:1.6;stroke-linecap:round" d="M12 15.5v.5M10 10a2 2 0 1 1 2.6 1.9c-.4.2-.6.5-.6.9v.4"/>' };
const mark = k => `<svg class="marks" viewBox="0 0 24 24" aria-hidden="true">${MARK[k]}</svg>`;
function stMark(t) { return t.status === 'complete' ? (t.onMain ? '<i class="sm st-main" title="on main"></i>' : '<i class="sm st-line" title="complete on the line"></i>') : t.status === 'blocked' ? '<i class="sm st-block" title="blocked"></i>' : t.location === 'pull request' ? '<i class="sm st-pr" title="in a pull request"></i>' : '<i class="sm st-todo" title="todo, no open PR"></i>'; }
function locChip(t) { return `<span class="chip ${t.onMain ? 'plain' : t.status === 'complete' ? 'accent' : t.location === 'pull request' ? 'warn' : t.status === 'blocked' ? 'danger' : 'plain'}">${esc(t.location)}${t.prs.length ? ' ' + t.prs.map(n => '#' + n).join(' ') : ''}</span>`; }
const prState = p => p.mergeable === 'MERGEABLE' ? 'ok' : p.mergeable === 'CONFLICTING' ? 'conf' : 'unk';
const prWord = p => p.mergeable === 'MERGEABLE' ? 'mergeable' : p.mergeable === 'CONFLICTING' ? 'conflicts with the line' : 'mergeability not yet computed by GitHub';

// ---- shared pieces -----------------------------------------------------------------------------------------
const tabs = (id, list) => `<div class="tabs" role="tablist" aria-label="Aspects">${list.map(([k, l, n], i) => `<button class="tab" role="tab" id="${id}-tab-${k}" aria-controls="${id}-${k}" aria-selected="${i === 0}" tabindex="${i === 0 ? 0 : -1}" type="button">${esc(l)}${n != null ? ` <span class="count">${n}</span>` : ''}</button>`).join('')}</div>${list.map(([k, , , body], i) => `<div class="tabpanel" role="tabpanel" id="${id}-${k}" aria-labelledby="${id}-tab-${k}"${i ? ' hidden' : ''}>${body}</div>`).join('')}`;
const taskRow = t => ({ cells: [`<span class="tc">${stMark(t)}<span class="tt">${taskName(t.id)}</span></span>`, locChip(t)] });
const taskTable = (ids, label = 'Tasks') => vt({ cls: 'tasks', cols: 'minmax(0,1fr) auto', head: ['Task', 'Location'], label, rows: ids.map(i => taskRow(T[i])) });
const epicState = e => e.status === 'in_progress' ? 'in progress' : e.status;
const readyIn = e => e.tasks.filter(i => T[i].next && T[i].next.state === 'ready').length;
const waitingIn = e => e.tasks.filter(i => T[i].next && T[i].next.state === 'waiting').length;
const nextStep = e => e.counts.complete === e.counts.total ? 'close the epic' : readyIn(e) ? `${readyIn(e)} tasks ready to start, ${waitingIn(e)} waiting on a predecessor` : waitingIn(e) ? `${waitingIn(e)} tasks waiting on a predecessor; none ready` : 'no ready task listed by work next';
const plainLine = s => String(s || '').replace(/\x60integration\/EPC-\d+-[^\x60]*\x60/g, 'the line').replace(/\x60main\x60/g, 'main');
const ctx = e => R.epicContext[e.id] || {};
// an epic's context in its own words: what it delivers, for whom, why now. Full sentences, never cut.
const epicAbout = (e, short) => `<div class="about-rows">
  <div class="rw"><b>What it delivers</b><span>${md(plainLine(short ? sentences(e.doneMeans, 1) : e.doneMeans))}</span></div>
  <div class="rw"><b>For whom</b><span>${e.serves === 'not recorded' ? '<span class="nr">no "it serves" sentence in the record; the Outcome names its users</span>' : md(e.serves)}</span></div>
  <div class="rw"><b>Why now</b><span>${md(ctx(e).whyNow || 'not recorded')}</span></div></div>`;

// ---- views -------------------------------------------------------------------------------------------------
const e022 = F.epics['EPC-022'];
const overview = `<section class="view is-active" id="overview" data-title="Overview">
  <div class="card section" data-block="Deliverable" style="margin-top:22px">
    <div class="hero"><div class="num">${overall}<small>%</small>${delta(+overall, +pct1(P.scope.complete, P.scope.total), 'pts')}</div><div class="what" style="max-width:none;flex:1;min-width:260px"><div class="about-rows"><div class="rw"><b>What it is</b><span>${md(sentences(e022.whatItIs, 2))}</span></div><div class="rw"><b>Done means</b><span>${esc(R.deliverable)}</span></div></div></div></div>
    <div class="chips" style="margin-top:14px"><span class="chip accent">${S.complete} of ${S.total} tasks complete</span>${delta(S.complete, P.scope.complete)}${delta(S.total, P.scope.total, 'in scope')}<span class="chip plain">${S.onMain} of them on main</span><span class="chip warn">${S.inPr} in pull requests</span><span class="chip plain">${S.blocked} blocked in task records</span><span class="chip ${NB ? 'danger' : 'plain'}">${NB} blocked by work next</span><span class="chip ${R.blockers.length ? 'danger' : 'plain'}"><a href="#blockers" data-view-link="blockers">${R.blockers.length} operational blockers</a></span><span class="chip plain">${R.inputs.length} decisions and ${R.actionsOwed.length} actions owed by you</span></div>
  </div>
  <h2 class="section" style="margin-top:30px">The ${EP.length} outcomes</h2>
  <p class="lede" style="margin-bottom:12px">Each opens to what it delivers, its themes, tasks and evidence. Percent and state are from the records on the line.</p>
  <div class="grid outcomes">${EP.map(e => `<a class="card outcome" href="#o-${e.id}" data-view-link="o-${e.id}" data-block="Outcome: ${esc(e.title)}">
      <div class="head"><span class="p">${pct1(e.counts.complete, e.counts.total)}%</span><span class="t">${esc(e.title)}</span><span class="chip ${e.status === 'in_progress' ? 'accent' : 'plain'}">${esc(epicState(e))}</span>${idTag(e.id)}</div>
      ${bar(e.counts)}
      ${epicAbout(e, true)}
      <div class="nums"><span><b>${e.counts.complete}</b> of <b>${e.counts.total}</b> done${delta(e.counts.complete, prevEpic(e.id).done)}</span><span><b>${e.counts.onMain}</b> on main</span><span><b>${e.counts.inPr}</b> in pull requests</span><span>next: ${esc(nextStep(e))}</span></div>
    </a>`).join('')}</div>
  <figure class="fig" data-block="Figure 1, the partition">
    <div class="fig-head"><span class="fig-t">Where the ${S.total} tasks are</span><span class="fig-n">Figure 1</span></div>
    ${partitionWide()}${partitionNarrow()}
    <p style="margin-top:10px;color:var(--text-2)">The hop to main: the line is ${F.gh.ahead} commits ahead of main${delta(F.gh.ahead, P.aheadOfMain)} and ${F.gh.behind} behind.</p>
    <p style="color:var(--text-2)">${F.gh.openToMain ? (F.gh.openToMain.length ? `Open into main (GitHub, ${when(F.gh.at)}): ` + F.gh.openToMain.map(p => esc(p.title) + ' (#' + p.number + ')').join('; ') + '.' : 'No pull request into main is open (GitHub, ' + when(F.gh.at) + ').') : 'Open pull requests into main were not read.'}</p>
    <ul class="key"><li><svg viewBox="0 0 22 14"><rect class="st-main" x="1" y="2" width="20" height="10" rx="3"/></svg>on main</li><li><svg viewBox="0 0 22 14"><rect class="st-line" x="1" y="2" width="20" height="10" rx="3"/></svg>complete on the line only</li><li><svg viewBox="0 0 22 14"><rect class="st-pr" x="1" y="2" width="20" height="10" rx="3"/></svg>in a pull request</li><li><svg viewBox="0 0 22 14"><rect class="st-todo" x="1" y="2" width="20" height="10" rx="3"/></svg>todo, no open PR</li><li><svg viewBox="0 0 22 14"><rect class="st-block" x="1" y="2" width="20" height="10" rx="3"/></svg>blocked</li></ul>
    ${dataTable(['Stage', 'Tasks', 'Share'], stations.map(([l, v]) => [esc(l), String(v), pct1(v, S.total) + '%']), 'Figure 1 as a table')}
    <figcaption>${esc(R.basis)}</figcaption>
  </figure>
  <p class="meta section" style="margin-bottom:8px"><span>The four streams, from the Coordinator trackers at <b>${esc(TRACK_AT)}</b></span></p>
  <div class="grid streams">${R.streams.map(st => `<div class="card" data-block="${esc(st.title)}"><div style="display:flex;justify-content:space-between;gap:8px;align-items:center"><b>${esc(st.title)}</b>${stepDots(st.step, st.steps)}</div><p class="meta" style="margin:6px 0 0">Step ${st.step} of ${st.steps}</p>${[['done', st.done], ['now', st.now], ['next', st.next]].map(([k, xs]) => `<p class="meta kicker-l">${k}</p><ul class="points" style="margin:0;color:var(--text-2)">${xs.map(x => `<li>${mdn(x)}</li>`).join('')}</ul>`).join('')}<div style="margin-top:10px"><span class="chip plain">next move: ${esc(st.owner)}</span></div></div>`).join('')}</div>
  <div class="card section" data-block="Pull requests against the line">
    <div style="display:flex;justify-content:space-between;gap:10px;flex-wrap:wrap;align-items:baseline"><h3>${F.gh.open.length} pull requests open against the line${delta(F.gh.open.length, P.prs.openAgainstLine)}</h3><span style="color:var(--text-2)">${F.gh.mergedToday.filter(p => p.baseRefName.startsWith('integration/EPC-022')).length} landed on the line today, ${F.gh.mergedToday.filter(p => p.baseRefName === 'main').length} on main (GitHub at ${esc(when(F.gh.at))})</span></div>
    <div class="chips" style="margin:12px 0"><span class="chip ok">${F.gh.open.filter(p => p.mergeable === 'MERGEABLE').length} mergeable</span><span class="chip warn">${F.gh.open.filter(p => p.mergeable === 'CONFLICTING').length} conflicting</span><span class="chip plain">${F.gh.open.filter(p => p.mergeable === 'UNKNOWN').length} not yet computed</span><span class="chip plain">${F.gh.open.filter(p => !p.isDraft).length} out of draft</span></div>
    ${vt({ cols: '7ch minmax(0,1fr) 7ch auto', head: ['State', 'Pull request', 'Number', 'Draft'], label: 'Open pull requests against the line', rows: F.gh.open.map(p => ({ cells: [`<span class="sr">${prWord(p)}</span>${mark(prState(p))}`, esc(p.title), `<span class="mono">#${p.number}</span>`, `<span class="mute">${p.isDraft ? 'draft' : 'out of draft'}</span>`] })) })}
    <ul class="key"><li>${mark('ok')}mergeable</li><li>${mark('conf')}conflicts with the line</li><li>${mark('unk')}not yet computed by GitHub at ${esc(when(F.gh.at))}</li></ul>
  </div>
  <details class="section terms" data-block="Terms"><summary>Terms used on this page</summary>${vt({ cols: 'minmax(100px,.5fr) minmax(0,1.5fr)', head: ['Term', 'Meaning'], label: 'Terms', rows: R.terms.map(([k, v]) => ({ cells: [`<b>${esc(k)}</b>`, md(v)] })) })}</details>
</section>`;

function topicView(e) {
  const id = 'o-' + e.id, c = e.counts;
  const themes = Object.entries(e.themes).map(([name, ids]) => ({ name, ids, done: ids.filter(i => T[i].status === 'complete').length, key: `t-${e.id}-${Object.keys(e.themes).indexOf(name)}` }));
  const order = ['ready', 'waiting', 'blocked'].map(s => [s, e.tasks.filter(i => T[i].next && T[i].next.state === s)]);
  const open = e.tasks.filter(i => T[i].status !== 'complete' && T[i].status !== 'cancelled' && !T[i].next);
  const done = e.tasks.filter(i => T[i].status === 'complete');
  const blocked = e.tasks.filter(i => T[i].status === 'blocked' || (T[i].next && T[i].next.state === 'blocked'));
  const prs = F.gh.open.filter(p => e.tasks.some(i => p.headRefName.includes(i)));
  const progress = `<div class="card" data-block="${esc(e.title)}: progress"><div class="epic"><div class="head"><span class="p">${pct1(c.complete, c.total)}%</span><span class="t">${c.complete} of ${c.total} tasks complete${delta(c.complete, prevEpic(e.id).done)}</span></div>${bar(c)}<div class="nums"><span><b>${c.onMain}</b> on main</span><span><b>${c.complete - c.onMain}</b> on the line only</span><span><b>${c.inPr}</b> in pull requests</span><span><b>${c.todo}</b> todo, no open PR</span><span><b>${c.blocked}</b> blocked</span><span><b>${c.cancelled}</b> cancelled, not counted</span></div></div></div>
    <h3 class="section" style="margin-top:26px">${themes.length} themes</h3><p class="lede" style="margin-bottom:10px">A theme is the epic record section that introduced its tasks (the status facts command, TSK-1095 AC-15). Each opens to its tasks.</p>
    ${vt({ cls: 'themes', cols: 'minmax(0,1.4fr) minmax(120px,1fr) 9ch', head: ['Theme', 'Done of total', 'State'], label: 'Themes', rows: themes.map(t => ({ cells: [`<a href="#${t.key}" data-view-link="${t.key}">${esc(t.name)}</a>`, `<span class="cell-bar"><span class="track"><span class="fill" style="width:${(100 * t.done / t.ids.length).toFixed(0)}%"></span></span><span class="n">${t.done}/${t.ids.length}</span></span>`, `<span class="mute">${t.ids.length === t.done ? 'done' : t.ids.filter(i => T[i].next && T[i].next.state === 'ready').length + ' ready'}</span>`] })) })}`;
  const orderBody = `<p class="lede" style="margin-bottom:10px">From <code>codeflow work next</code> at ${esc(when(F.next.at))}: ready first, then waiting with the predecessor each waits on.</p>${order.map(([s, ids]) => ids.length ? `<h3 class="section" style="margin-top:18px">${ids.length} ${s}</h3>${s === 'ready' ? taskTable(ids, 'Ready tasks') : vt({ cls: 'tasks', cols: 'minmax(0,1fr) minmax(0,1fr)', head: ['Task', 'Why it waits'], label: s + ' tasks', rows: ids.map(i => ({ cells: [`<span class="tc">${stMark(T[i])}<span class="tt">${taskName(i)}</span></span>`, `<span class="mute">${mdn((T[i].next.reason || '').replace(/^task TSK-\d+ /, ''))}</span>`] })) })}` : '').join('')}${open.length ? `<h3 class="section" style="margin-top:18px">${open.length} open and not listed by work next</h3>${taskTable(open, 'Open tasks not listed by work next')}` : ''}<details class="section" style="margin-top:18px"><summary>${done.length} complete tasks</summary>${taskTable(done, 'Complete tasks')}</details>`;
  const issuesBody = `${blocked.length ? `<h3>${blocked.length} blocked</h3>${taskTable(blocked, 'Blocked tasks')}` : '<p class="lede">No task of this epic is blocked on the line.</p>'}<h3 class="section" style="margin-top:22px">${prs.length} open pull requests</h3>${prs.length ? vt({ cols: 'minmax(0,1fr) 7ch auto', head: ['Pull request', 'Number', 'State'], label: 'Open pull requests of this epic', rows: prs.map(p => ({ cells: [esc(p.title), `<span class="mono">#${p.number}</span>`, `<span class="chip ${p.mergeable === 'MERGEABLE' ? 'ok' : p.mergeable === 'CONFLICTING' ? 'warn' : 'plain'}">${p.isDraft ? 'draft, ' : ''}${p.mergeable === 'UNKNOWN' ? 'not computed' : p.mergeable.toLowerCase()}</span>`] })) }) : '<p class="lede">None against the line names a task of this epic.</p>'}`;
  return `<section class="view" id="${id}" data-title="${esc(e.title)}">
  ${crumbs([['overview', 'Release'], [id, e.title]])}
  <div class="vhead"><h2>${esc(e.title)}</h2><span class="chip ${e.status === 'in_progress' ? 'accent' : 'plain'}">${esc(epicState(e))}</span>${idTag(e.id)}</div>
  <div class="card about" data-block="${esc(e.title)}: what it is" style="margin:0 0 14px"><b>What it is</b><p style="margin-top:6px;max-width:none">${md(plainLine(e.whatItIs))}</p>${epicAbout(e, false)}<p class="meta" style="margin-top:8px">Source: <code>epics/${e.id}.md</code> on the line, Summary; why now from ${esc(ctx(e).source || 'the record')}.</p></div>
  ${strip([['percent', `<b>${pct1(c.complete, c.total)}%</b>`], ['tasks', `${c.total}`], ['epic criteria', `${e.criteria}`], ['created', esc(day(e.created))], ['next step', esc(nextStep(e))]])}
  <div class="chips" style="margin:0 0 6px"><span class="mute" style="font-size:var(--fs-1)">Capabilities it creates or changes:</span>${capChips(e.capabilities)}</div>
  ${tabs(id, [['progress', 'Progress', null, progress], ['order', 'Order', readyIn(e), orderBody], ['issues', 'Issues', blocked.length + prs.length, issuesBody]])}
</section>`;
}

const epicsView = `<section class="view" id="epics" data-title="Epics">
  <h2 class="section" style="margin-top:22px">The three epics of 4.0.0</h2>
  <p class="lede" style="margin-bottom:14px">What each delivers, for whom and why now, in its record's words; then done, not done and the percent from each task's record. An epic counts as verified only when its record says so.</p>
  <div class="grid">${EP.map(e => `<div class="card epic" data-block="${esc(e.title)}"><div class="head"><span class="p">${pct1(e.counts.complete, e.counts.total)}%</span><a class="t" href="#o-${e.id}" data-view-link="o-${e.id}">${esc(e.title)}</a><span class="id">${e.id}, ${esc(epicState(e))}, created ${esc(day(e.created))}</span></div>${epicAbout(e, true)}${bar(e.counts)}<div class="nums"><span><b>${e.counts.complete}</b> done</span><span><b>${e.counts.onMain}</b> on main</span><span><b>${e.counts.total - e.counts.complete - e.counts.blocked}</b> not done</span><span><b>${e.counts.inPr}</b> in pull requests</span><span><b>${e.counts.blocked}</b> blocked</span><span><b>${e.counts.total}</b> in all</span></div><div class="chips"><span class="chip warn">verification not recorded on the line</span><span class="chip plain">${e.criteria} epic criteria</span><span class="chip plain">${Object.keys(e.themes).length} themes</span></div><p class="meta">Next: ${esc(nextStep(e))}.</p></div>`).join('')}</div>
</section>`;

const decidedTable = label => vt({ cls: 'decided', cols: 'minmax(110px,auto) minmax(0,1fr)', head: ['When', 'Decided'], label, rows: R.decided.map(x => ({ cells: [`<span class="mute">${esc(x.when)}</span>`, `<span class="tc"><i class="sm st-main" title="decided"></i><span class="tt">${mdn(x.what)}</span></span>`] })) });
const blockersView = `<section class="view" id="blockers" data-title="Blockers">
  <h2 class="section" style="margin-top:22px">${R.blockers.length} blockers</h2>
  <p class="lede" style="margin-bottom:14px">Each with who clears it, the recommendation, why, and the alternative. Read from the trackers at ${esc(TRACK_AT)}.</p>
  <div class="grid">${R.blockers.map((b, i) => `<div class="card blk" data-block="Blocker ${i + 1}"><h3><span class="chip danger">${i + 1}</span>${esc(b.title)}</h3><div class="chips"><span class="chip accent plain">clears it: ${esc(b.who)}</span><span class="chip plain">${esc(b.state)}</span></div><div class="rw rec"><b>Recommend</b><span>${mdn(b.rec)}</span></div><div class="rw"><b>Why</b><span>${mdn(b.why)}</span></div><div class="rw"><b>Alternative</b><span>${mdn(b.alt)}</span></div><div class="rw"><b>Source</b><span class="src">${esc(b.source)}</span></div></div>`).join('')}</div>
</section>`;

const sinceView = `<section class="view" id="since" data-title="What changed">
  <h2 class="section" style="margin-top:22px">What changed since the previous report</h2>
  <p class="lede" style="margin-bottom:14px">Previous report read at <b>${esc(PREV_W)}</b> (line tip ${esc(P.lineTip)}); this one at <b>${esc(READ_AT)}</b> (line tip ${esc(F.read.lineTip)}). The same marks sit beside the numbers in every view.</p>
  ${vt({ cols: 'minmax(0,1fr) 7ch 7ch 9ch', head: ['Number', 'Was', 'Now', 'Change'], label: 'Numbers then and now', rows: changes.map(c => ({ cls: c.prev === c.now ? 'same' : '', cells: [`<a href="#${c.view}" data-view-link="${c.view}">${esc(c.what)}</a>`, `<span class="mute">${c.prev ?? 'n/a'}</span>`, `<span class="mono">${c.now}</span>`, c.prev === c.now ? '<span class="mute">same</span>' : delta(c.now, c.prev)] })) })}
  <h3 class="section" style="margin-top:26px">${F.gh.mergedToday.length} pull requests merged today</h3>
  ${vt({ cols: 'minmax(0,1fr) 7ch auto', head: ['Pull request', 'Number', 'Into'], label: 'Pull requests merged today', rows: F.gh.mergedToday.map(p => ({ cells: [esc(p.title), `<span class="mono">#${p.number}</span>`, `<span class="chip ${p.baseRefName === 'main' ? 'ok' : 'accent'}">${esc(p.baseRefName === 'main' ? 'main' : 'the ' + p.baseRefName.replace('integration/', '').replace(/-the-.*$/, '').replace(/-.*$/, '') + ' line')}</span>`] })) })}
  ${newlyComplete ? `<h3 class="section" style="margin-top:26px">${newlyComplete.length} tasks complete now that were not at ${esc(PREV_W)}</h3>${newlyComplete.length ? taskTable(newlyComplete, 'Newly complete tasks') : '<p class="lede">None.</p>'}` : ''}
  <h3 class="section" style="margin-top:26px">Decided since</h3>
  ${decidedTable('Decided since the previous report')}
  <h3 class="section" style="margin-top:26px">The previous report's blockers today</h3>
  ${vt({ cols: 'minmax(0,1fr) minmax(0,1fr)', head: [`Then (${PREV_W})`, 'Now'], label: 'Previous blockers today', rows: P.blockers.map(b => { const now = b.id === 'b2' ? `${F.gh.open.filter(p => p.mergeable === 'CONFLICTING').length} conflicting; GitHub has not computed ${F.gh.open.filter(p => p.mergeable === 'UNKNOWN').length} of ${F.gh.open.length} (${when(F.gh.at)})` : b.id === 'b3' ? 'unchanged: the quality defaults line (EPC-024) is still at e283948c6 (blocker 3 here)' : b.id === 'b4' ? 'unchanged: the installed hook binary differs from the line\u2019s sources (blocker 4 here)' : 'not rechecked today; the fetch at 12:22 EST, 2 Nov printed no error to this session'; return { cells: [esc(b.title), `<span class="mute">${esc(now)}</span>`] }; }) })}
</section>`;

const lanesView = `<section class="view" id="lanes" data-title="Lanes">
  <h2 class="section" style="margin-top:22px">${R.lanes.length} lane entries in the trackers</h2>
  <p class="lede" style="margin-bottom:14px">By Coordinator, from the trackers at ${esc(TRACK_AT)}. ${esc(R.lanesNote)}</p>
  ${['Parallel', 'Visual and docs', 'Planning', 'Capabilities'].map(co => { const ls = R.lanes.filter(l => l.coordinator === co); return ls.length ? `<h3 class="section" style="margin-top:18px">${esc(co)} Coordinator</h3>${vt({ cls: 'lanes', cols: 'minmax(140px,.9fr) minmax(0,1.6fr) minmax(0,1fr)', head: ['Lane', 'What', 'State and expected'], label: co + ' lanes', rows: ls.map(l => ({ cells: [`<b>${esc(l.lane)}</b><br><span class="mute">${esc(l.who)}${l.seatIds ? ` <span class="src">(${esc(l.seatIds)})</span>` : ''}</span>`, `<span class="mute">${mdn(l.what)}</span>`, `<span class="chip ${/wait|idle/.test(l.stateShort) ? 'warn' : /fix/.test(l.stateShort) ? 'danger' : 'accent'}">${esc(l.stateShort)}</span><br><span class="mute">${mdn(l.state)}; expected ${mdn(l.expected)}</span>`] })) })}` : ''; }).join('')}
</section>`;

const sources = [['Task records on the line', 'Line records', 'git show ' + F.read.lineRef, F.read.finished], ['Task records on origin/main', 'Main records', 'git show origin/main', F.read.finished], ['Record history on the line', 'Record history', 'git log --name-only ' + F.read.lineRef, F.read.finished], ['GitHub, read only', 'GitHub', 'gh pr list, gh api compare', F.gh.at], ['codeflow work next', 'work next', 'in the task worktree', F.next.at], ['Coordinator trackers', 'Trackers', 'coordination/coordinators/*/TRACKER.md', R.readAt], ['Previous report', 'Previous report', '_src/previous-2026-11-01-1431.json', PREV_AT]];
const measures = [['task counts and percent', [1, 0, 0, 0, 0, 0, 0]], ['on main', [1, 1, 0, 0, 0, 0, 0]], ['in a pull request', [1, 0, 0, 1, 0, 0, 0]], ['record last changed', [0, 0, 1, 0, 0, 0, 0]], ['pull request state', [0, 0, 0, 1, 0, 0, 0]], ['ready, waiting, blocked', [0, 0, 0, 0, 1, 0, 0]], ['themes', [1, 0, 0, 0, 0, 0, 0]], ['streams, lanes, blockers', [0, 0, 0, 0, 0, 1, 0]], ['decisions and actions owed', [0, 0, 0, 0, 0, 1, 0]], ['change marks', [1, 0, 0, 1, 0, 0, 1]], ['lane and window per task', [0, 0, 0, 0, 0, 0, 0]]];
const covDesc = measures.map(([m, cells]) => `${m}: ${cells.some(c => c) ? sources.filter((s, j) => cells[j]).map(s => s[1]).join(' and ') : 'not read today'}`).join('; ') + '.';
function coverageFig() {
  const cw = 104, rh = 30, lx = 230, ty = 70, W = lx + sources.length * cw + 10, H = ty + measures.length * rh + 10; let o = '';
  sources.forEach((s, j) => { const x = lx + j * cw + cw / 2; const ws = s[1].split(' '); o += `<text x="${x}" y="${ty - (ws.length > 1 ? 28 : 20)}" text-anchor="middle" class="s">${esc(ws[0])}</text>${ws.length > 1 ? `<text x="${x}" y="${ty - 12}" text-anchor="middle" class="s">${esc(ws.slice(1).join(' '))}</text>` : ''}`; });
  measures.forEach(([m, cells], i) => { const y = ty + i * rh; o += `<text x="${lx - 12}" y="${y + rh / 2 + 4}" text-anchor="end" class="b">${esc(m)}</text>`; const none = cells.every(c => !c); cells.forEach((c, j) => { const x = lx + j * cw + cw / 2; o += c ? `<circle class="ok-fill" cx="${x}" cy="${y + rh / 2}" r="7"/>` : none ? `<path class="ink2" d="M${x - 6} ${y + rh / 2}h12"/>` : `<circle class="unk-ring" cx="${x}" cy="${y + rh / 2}" r="5"/>`; }); if (none) o += `<text x="${lx + sources.length * cw + 4}" y="${y + rh / 2 + 4}" class="s">not read today</text>`; });
  return `<svg viewBox="0 0 ${W + 110} ${H}" ${figA11y('cov', 'Which source each measure was read from', covDesc)}${o}</svg>`;
}
const evidenceView = `<section class="view" id="evidence" data-title="Evidence">
  <h2 class="section" style="margin-top:22px">Which numbers you can trust, and what was not read</h2>
  <p class="lede" style="margin-bottom:14px">One read from ${esc(when(F.read.started))} to ${esc(READ_AT)}; the line at ${esc(F.read.lineTip)}, main at ${esc(F.read.mainTip)}, the installed binary ${esc(F.read.codeflow)}.${F.read.recordsRefreshedAt ? ` The record texts (task outcomes, epic summaries, capability names, the docs diff) were re-read at the same two tips at ${esc(when(F.read.recordsRefreshedAt))}; no count changed.` : ''}</p>
  <figure class="fig" data-block="Figure 2, coverage"><div class="fig-head"><span class="fig-t">Measures by sources</span><span class="fig-n">Figure 2</span></div><div class="scroll-x" tabindex="0" role="region" aria-label="Figure 2, coverage matrix; scrolls sideways on a narrow screen">${coverageFig()}</div><ul class="key"><li><svg viewBox="0 0 24 24" class="marks"><circle class="ok-fill" cx="12" cy="12" r="7"/></svg>read from this source</li><li><svg viewBox="0 0 24 24" class="marks"><circle class="unk-ring" cx="12" cy="12" r="5"/></svg>not from this source</li><li><svg viewBox="0 0 24 24" class="marks"><path class="ink2" d="M5 12h14"/></svg>not read today (shown as not recorded)</li></ul>
  ${dataTable(['Measure', 'Read from'], measures.map(([m, cells]) => [esc(m), cells.some(c => c) ? sources.filter((s, j) => cells[j]).map(s => esc(s[1])).join(', ') : '<span class="mute">not read today</span>']), 'Figure 2 as a table')}
  <figcaption>Lane and window per task are fields of the status facts command (TSK-1095); until it lands they read <code>not recorded</code> on every task view.</figcaption></figure>
  ${vt({ cls: 'section', cols: 'minmax(0,1fr) minmax(0,1.4fr) auto', head: ['Source', 'How', 'Read at'], label: 'Sources', rows: sources.map(s => ({ cells: [`<b>${esc(s[0])}</b>`, `<span class="mute"><code>${esc(s[2])}</code></span>`, `<span class="mute">${esc(when(s[3]))}</span>`] })) })}
  <div class="note section" data-block="Not verified"><span class="chip warn">not verified</span><span>GitHub had not computed mergeability for ${F.gh.open.filter(p => p.mergeable === 'UNKNOWN').length} of ${F.gh.open.length} open pull requests at ${esc(when(F.gh.at))}; the Herdr seats and host load were not probed by this page; the hand-entered tracker facts carry their own times and were not re-verified by this page.</span></div>
</section>`;

const inputsView = `<section class="view" id="inputs" data-title="Your decisions">
  <h2 class="section" style="margin-top:22px">${R.inputs.length} decisions for the walk-through, ${R.actionsOwed.length} actions owed</h2>
  <h3 class="section" style="margin-top:18px">Decided today, not asked again</h3>
  ${decidedTable('Decided today')}
  <h3 class="section" style="margin-top:22px">Actions only you can take</h3>
  ${vt({ cls: 'owed', cols: 'minmax(0,1fr) minmax(110px,auto)', head: ['Action', 'Since'], label: 'Actions owed', rows: R.actionsOwed.map(x => ({ cells: [`<span class="tc"><i class="sm st-todo" title="owed"></i><span class="tt">${mdn(x.what)}</span></span>`, `<span class="mute">${esc(x.since)}</span>`] })) })}
  <div style="height:14px"></div>
  <p class="lede" style="margin-bottom:14px">Each asks one plain question. The recommended answer is marked, never preselected. An answer records your choice and authorises nothing else; the Script view writes exactly what you chose.</p>
  <form class="form" data-answers="tsk-1122-walkthrough" id="decisions">${R.inputs.map((q, i) => `<div class="card" data-block="Decision ${i + 1}"><div class="q">${i + 1}. ${esc(q.title)}</div><div class="hint">${mdn(q.context)}</div><div class="opts">${q.options.map(o => `<label class="opt"><input type="radio" name="${q.id}" value="${o.value}" data-label="${esc(o.label)}"><span>${esc(o.label)}</span>${o.recommended ? '<span class="rec">recommended</span>' : '<span></span>'}<span class="cost">cost: ${mdn(o.cost)}</span></label>`).join('')}</div><div class="hint" style="margin-top:8px"><b>Why:</b> ${mdn(q.reasoning)}</div></div>`).join('')}
    <div class="card" data-block="Named changes"><div class="q">Changes you name</div><div class="hint">Anything the prototype should change before the build tasks start; this goes into the record as the operator's words.</div><textarea name="changes" rows="3" style="margin-top:8px" placeholder="none, or your changes"></textarea></div>
    <div style="display:flex;gap:8px;flex-wrap:wrap;align-items:center"><button class="btn primary" type="submit">Keep answers for this session</button><button class="btn" type="reset">Clear</button><span style="color:var(--text-2)">then copy the script from the Script view</span></div>
  </form>
</section>`;

const scriptView = `<section class="view" id="script" data-title="Script">
  <h2 class="section" style="margin-top:22px">One script: recheck, then record only what you chose</h2>
  <p class="lede" style="margin-bottom:14px">Part 1 rechecks the numbers here, read only, from any folder. Part 2 appears only for decisions you answered, checks the workspace is on <code>integration/workspace</code>, writes your answers to one file and commits that file alone.</p>
  <div class="copy-wrap" data-block="Script"><button class="btn" type="button" data-copy="#script-text">Copy</button><pre tabindex="0" role="region" aria-label="Recheck script"><code id="script-text"></code></pre></div>
</section>`;

// ---- the ten report kinds, each shown as a layout specimen on the same shell -----------------------------------
const KIND_WHAT = { overview: 'the root view: the one number, the outcomes, the lead figure', topic: 'one subject with its context strip and aspect tabs', item: 'one record: about first, then status, decisions and blockers', list: 'a visual table with marks', timeline: 'rows in time order with what changed', issues: 'each issue with who clears it, the recommendation, why and the alternative', decisions: 'decided items, owed actions and the operator\u2019s form controls', evidence: 'the coverage figure, the sources and what was not verified', commands: 'one labelled code block with copy' };
const slug = s => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
const kinds = R.kinds.map(k => ({ ...k, view: k.note === 'this prototype' ? 'overview' : 's-' + slug(k.kind) }));
// the lead figure of each kind, drawn as a schematic with its vocabulary as labels (no numbers; this is a specimen)
function leadGlyph(kind, x, y, w) {
  const seg = (xs, cls) => xs.map(([a, b]) => `<rect class="${cls}" x="${(x + a * w).toFixed(1)}" y="${y}" width="${((b - a) * w - 2).toFixed(1)}" height="26" rx="3"/>`).join('');
  const lbl = (tx, ty, t, c = 's') => `<text x="${tx.toFixed(1)}" y="${ty}" class="${c}">${esc(t)}</text>`;
  switch (kind) {
    case 'partition': return seg([[0, .25]], 'st-main') + seg([[.25, .32]], 'st-line') + seg([[.32, .38]], 'st-pr') + seg([[.38, 1]], 'st-todo') + lbl(x, y + 46, 'on main, on the line, in a pull request, todo: one bar, in stage order');
    case 'criteria': return ['criterion proved on the candidate', 'criterion proved on the candidate', 'criterion not yet proved'].map((t, i) => `<circle class="${i < 2 ? 'ok-fill' : 'unk-ring'}" cx="${x + 8}" cy="${y + 10 + i * 24}" r="7"/>` + lbl(x + 24, y + 15 + i * 24, t)).join('');
    case 'severity': return [['blocking', .9, 'st-block'], ['material', .6, 'warn-fill'], ['nit', .3, 'st-todo']].map(([t, f, c], i) => `<rect class="${c}" x="${x + 70}" y="${y + i * 24}" width="${(w - 70) * f}" height="16" rx="3"/>` + lbl(x, y + 13 + i * 24, t, 'b')).join('');
    case 'timeline': { const n = 5; return `<path class="ink2" d="M${x} ${y + 12}H${x + w}"/>` + Array.from({ length: n }, (_, i) => `<circle class="${i < n - 1 ? 'ok-fill' : 'unk-ring'}" cx="${x + 10 + i * (w - 20) / (n - 1)}" cy="${y + 12}" r="6"/>`).join('') + lbl(x, y + 40, 'first event') + `<text x="${x + w}" y="${y + 40}" text-anchor="end" class="s">latest</text>`; }
    case 'pair': return `<rect class="node-soft" x="${x}" y="${y}" width="${w / 2 - 8}" height="60" rx="6"/><rect class="acc-wash" x="${x + w / 2 + 8}" y="${y}" width="${w / 2 - 8}" height="60" rx="6"/>` + lbl(x + 10, y + 34, 'before', 'b') + lbl(x + w / 2 + 18, y + 34, 'after', 'b');
    case 'structure': return `<rect class="acc-wash" x="${x + w / 2 - 60}" y="${y}" width="120" height="26" rx="6"/>` + lbl(x + w / 2 - 50, y + 17, 'capability', 'b') + [0, 1, 2].map(i => `<path class="ink2" d="M${x + w / 2} ${y + 26}V${y + 40}H${x + w / 6 + i * w / 3}V${y + 50}"/><rect class="node-soft" x="${x + w / 6 + i * w / 3 - 40}" y="${y + 50}" width="80" height="22" rx="5"/>` + lbl(x + w / 6 + i * w / 3 - 30, y + 65, 'task')).join('');
    case 'owners': return [0, 1].map(i => `<rect class="node-soft" x="${x + i * (w / 2 + 6)}" y="${y}" width="${w / 2 - 6}" height="72" rx="6"/>` + lbl(x + 10 + i * (w / 2 + 6), y + 18, i ? 'owed by the primary' : 'owed by you', 'b') + [0, 1].map(j => `<circle class="unk-ring" cx="${x + 16 + i * (w / 2 + 6)}" cy="${y + 38 + j * 18}" r="5"/>` + lbl(x + 28 + i * (w / 2 + 6), y + 42 + j * 18, 'item, since, resume command')).join('')).join('');
    case 'options': return [['option, recommended', 'acc-wash'], ['option', 'node-soft'], ['option', 'node-soft']].map(([t, c], i) => `<rect class="${c}" x="${x}" y="${y + i * 26}" width="${w}" height="20" rx="5"/><circle class="unk-ring" cx="${x + 12}" cy="${y + 10 + i * 26}" r="5"/>` + lbl(x + 26, y + 14 + i * 26, t)).join('');
    case 'bar': return seg([[0, .4]], 'st-line') + seg([[.4, 1]], 'st-todo') + lbl(x, y + 46, 'one outcome: done, then the rest');
    default: return '';
  }
}
function specimenFig(k) {
  const W = 1000, mx = 20, mw = 250, px = 300, pw = 680; let o = '';
  o += `<rect class="node" x="${mx}" y="20" width="${mw}" height="${40 + k.views.length * 28}" rx="8"/><text x="${mx + 14}" y="44" class="b">menu</text>`;
  k.views.forEach((v, i) => { o += `<rect class="${i === 0 ? 'acc-wash' : 'fill'}" x="${mx + 10}" y="${56 + i * 28}" width="${mw - 20}" height="22" rx="4"/><text x="${mx + 20}" y="${71 + i * 28}" class="${i === 0 ? 'b' : 's'}">${esc(v)}</text>`; });
  const ph = Math.max(40 + k.views.length * 28, 200);
  o += `<rect class="node" x="${px}" y="20" width="${pw}" height="${ph}" rx="8"/><text x="${px + 16}" y="44" class="b">${esc(k.views[0])} view: ${esc(k.lead)}</text>`;
  o += leadGlyph(k.figure, px + 16, 66, pw - 32);
  o += `<text x="${px + 16}" y="${ph + 8}" class="s">below the figure: ${esc(k.opening)}</text>`;
  const H = ph + 30;
  const desc = `A layout specimen of the ${k.kind} kind: a menu of ${k.views.length} views in declared order (${k.views.join(', ')}), and the ${k.views[0]} view, which leads with ${k.lead}; below the figure: ${k.opening}.`;
  const wide = `<svg class="wide" viewBox="0 0 ${W} ${H}" ${figA11y('sp-' + slug(k.kind), `${k.kind}: the menu and the opening view`, desc)}${o}</svg>`;
  // narrow: the menu as a row of chips, then the opening view
  let n = ''; let cx = 10, cy = 14; k.views.forEach((v, i) => { const tw = v.length * 7.5 + 16; if (cx + tw > 320) { cx = 10; cy += 26; } n += `<rect class="${i === 0 ? 'acc-wash' : 'fill'}" x="${cx}" y="${cy}" width="${tw.toFixed(0)}" height="20" rx="10"/><text x="${cx + 8}" y="${cy + 14}" class="s">${esc(v)}</text>`; cx += tw + 6; });
  const oy = cy + 36; n += `<rect class="node" x="10" y="${oy}" width="310" height="150" rx="8"/><text x="22" y="${oy + 22}" class="b">${esc(k.views[0])}: ${esc(k.lead)}</text>` + leadGlyph(k.figure, 22, oy + 40, 286);
  const narrow = `<svg class="narrow" viewBox="0 0 330 ${oy + 170}" ${figA11y('spn-' + slug(k.kind), `${k.kind}: the menu as chips and the opening view`, desc)}${n}</svg>`;
  return `<figure class="fig" data-block="${esc(k.kind)} specimen"><div class="fig-head"><span class="fig-t">${esc(k.kind)}: the menu and the opening view</span><span class="fig-n">Specimen</span></div>${wide}${narrow}${dataTable(['Position', 'View', 'What it holds'], k.views.map((v, i) => [String(i + 1), esc(v), esc(KIND_WHAT[v] || '')]), k.kind + ' views as a table')}<figcaption>A specimen of layout, not of data: the shapes carry the kind's vocabulary as labels. The report and portal samples task (TSK-1127) builds each kind on real sources: ${esc(k.reads)}.</figcaption></figure>`;
}
const kindsView = `<section class="view" id="kinds" data-title="Report kinds">
  <h2 class="section" style="margin-top:22px">Ten report kinds on one shell</h2>
  <p class="lede" style="margin-bottom:14px">Each kind is a view tree from the dashboard shell's registry (TSK-1120); the shell, menu, tabs and marks are the same. The status report is this page; the other nine open as layout specimens.</p>
  ${vt({ cls: 'kinds', cols: 'minmax(140px,.9fr) minmax(0,2fr) minmax(0,1fr)', head: ['Kind', 'Views, in declared order', 'Leads with'], label: 'Report kinds', rows: kinds.map(k => ({ cells: [`<a href="#${k.view}" data-view-link="${k.view}"><b>${esc(k.kind)}</b></a>${k.note ? `<br><span class="mute">${esc(k.note)}</span>` : ''}`, `<span class="chips">${k.views.map((v, i) => `<span class="chip ${i === 0 ? 'accent' : 'plain'}">${i + 1} ${v}</span>`).join('')}</span>`, `<span class="mute">${esc(k.lead)}</span>`] })) })}
  <ul class="key" style="margin-top:12px"><li><span class="chip accent">1 overview</span>the root view, always first</li><li><span class="chip plain">n kind</span>the nth view in the kind's menu</li></ul>
</section>`;
const specimenViews = kinds.filter(k => k.view !== 'overview').map(k => `<section class="view" id="${k.view}" data-title="${esc(k.kind)}">
  ${crumbs([['kinds', 'Report kinds'], [k.view, k.kind]])}
  <div class="vhead"><h2>${esc(k.kind)}</h2><span class="chip plain">layout specimen</span></div>
  ${strip([['leads with', esc(k.lead)], ['views', String(k.views.length)], ['reads', esc(k.reads)], ['note', esc(k.note)]])}
  ${specimenFig(k)}
  ${vt({ cls: 'section', cols: '4ch minmax(120px,.6fr) minmax(0,1.6fr)', head: ['#', 'View', 'What it holds in this kind'], label: k.kind + ' views', rows: k.views.map((v, i) => ({ cells: [String(i + 1), `<b>${esc(v)}</b>`, `<span class="mute">${esc(KIND_WHAT[v] || '')}</span>`] })) })}
</section>`).join('\n');

// ---- the docs portal on the same shell: layers, one topic with its three tabs, and its What changed page ---------
function portalFig() {
  const L = R.portalLayers; const desc = L.map((l, i) => `${i + 1}. ${l.layer}: ${l.what}; in a report, ${l.same}`).join('. ') + '.';
  let o = ''; const W = 1000, x0 = 40, w = 920;
  L.forEach((l, i) => { const y = 24 + i * 84; o += `<rect class="${i === 0 ? 'acc-wash' : 'node-soft'}" x="${x0 + i * 40}" y="${y}" width="${w - i * 80}" height="62" rx="10"/><text x="${x0 + i * 40 + 16}" y="${y + 26}" class="b">${esc(l.layer)}</text><text x="${x0 + i * 40 + 16}" y="${y + 48}" class="s">${esc(l.what)}</text>`; if (i) o += `<path class="ink2" d="M${W / 2} ${y - 22}v16" marker-end="url(#arr)"/>`; });
  const wide = `<svg class="wide" viewBox="0 0 ${W} 280" ${figA11y('pl', 'Three portal layers: overview map, area page, reference page', desc)}<defs><marker id="arr" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L10 5 0 10z" fill="var(--fig-ink-2)"/></marker></defs>${o}</svg>`;
  let n = ''; L.forEach((l, i) => { const y = 10 + i * 70; n += `<rect class="${i === 0 ? 'acc-wash' : 'node-soft'}" x="${10 + i * 12}" y="${y}" width="${310 - i * 24}" height="54" rx="8"/><text x="${22 + i * 12}" y="${y + 22}" class="b">${esc(l.layer)}</text><text x="${22 + i * 12}" y="${y + 42}" class="s">${esc(l.same)}</text>`; });
  const narrow = `<svg class="narrow" viewBox="0 0 330 ${10 + L.length * 70}" ${figA11y('pln', 'Three portal layers, stacked, each with its report kind', desc)}${n}</svg>`;
  return wide + narrow;
}
const portalView = `<section class="view" id="portal" data-title="Docs portal layers">
  <h2 class="section" style="margin-top:22px">The docs portal on the same tokens, three layers</h2>
  <p class="lede" style="margin-bottom:14px">Each layer maps onto a shell kind, so a reader moves between the portal and a report without relearning the page. One topic with its three tabs is at <a href="#docs" data-view-link="docs">A docs topic</a>; the portal's own change page is at <a href="#docs-since" data-view-link="docs-since">Docs: what changed</a>.</p>
  <figure class="fig" data-block="Figure 3, portal layers"><div class="fig-head"><span class="fig-t">Overview map, area page, reference page</span><span class="fig-n">Figure 3</span></div>${portalFig()}${dataTable(['Layer', 'What it holds', 'Same as in a report'], R.portalLayers.map(l => [esc(l.layer), esc(l.what), esc(l.same)]), 'Figure 3 as a table')}<figcaption>Every layer is a page kind with the same top bar, menu and marks as this report; the report and portal samples task (TSK-1127) ships one sample of each.</figcaption></figure>
</section>`;

// one real topic: the four enforcement planes, from .codeflow/rules/git-rules.md "Enforcement" (lines 6 to 36 in this worktree)
const PLANES = [['Git hooks', 'five shims on the clone', 'local'], ['In-session guards', 'git-guard and exec-guard before every command', 'local'], ['Scaffolded CI', 'codeflow ci on the hosted run', 'server'], ['Remote branch protection', 'the rules on the forge', 'server']];
const HOOKS = [['pre-commit', 'refuses protected-branch commits, commits at the root checkout off its root branch, staged secrets and unresolved conflict markers'], ['commit-msg', 'checks the commit format, AI attribution and emoji'], ['pre-push', 'checks branch naming and refuses protected-branch pushes, force-pushes and deletes'], ['pre-merge-commit', 'the protected-branch merge backstop'], ['reference-transaction', 'the protected-branch ref backstop']];
function planesConcept() {
  const desc = 'Four planes around a change, inner to outer: git hooks and in-session guards are local fast feedback an agent on the host can edit or skip; scaffolded CI and remote branch protection are the server-side boundary. All four read one policy file.';
  const W = 1000; let o = '';
  PLANES.forEach((p, i) => { const inset = (3 - i) * 44; o += `<rect class="${i < 2 ? 'node-soft' : 'acc-wash'}" x="${20 + inset}" y="${16 + inset * .6}" width="${960 - 2 * inset}" height="${250 - inset * 1.2}" rx="12"/><text x="${34 + inset}" y="${38 + inset * .6}" class="b">${esc(p[0])}</text><text x="${34 + inset}" y="${56 + inset * .6}" class="s">${esc(p[1])}, ${p[2] === 'local' ? 'local fast feedback' : 'the server-side boundary'}</text>`; });
  o += `<rect class="node" x="420" y="150" width="160" height="40" rx="8"/><text x="500" y="175" text-anchor="middle" class="b">the change</text>`;
  const wide = `<svg class="wide" viewBox="0 0 ${W} 280" ${figA11y('pc', 'Four planes around a change: two local, two server side', desc)}${o}</svg>`;
  let n = ''; PLANES.forEach((p, i) => { const y = 8 + i * 64; n += `<rect class="${i < 2 ? 'node-soft' : 'acc-wash'}" x="10" y="${y}" width="310" height="54" rx="8"/><text x="22" y="${y + 22}" class="b">${esc(p[0])}</text><text x="22" y="${y + 42}" class="s">${p[2] === 'local' ? 'local fast feedback' : 'server-side boundary'}</text>`; });
  const narrow = `<svg class="narrow" viewBox="0 0 330 270" ${figA11y('pcn', 'Four planes, listed inner to outer', desc)}${n}</svg>`;
  return wide + narrow + dataTable(['Plane', 'Where it runs', 'What it is'], PLANES.map(p => [esc(p[0]), esc(p[1]), p[2] === 'local' ? 'local fast feedback' : 'server-side boundary']), 'Concept figure as a table');
}
function planesArchitecture() {
  const desc = 'One policy file, .codeflow/policy.json, in the middle; the hooks and CI read it directly; the in-session guards run from each harness\u2019s settings (.claude/settings.json, the .codex starter, .grok/hooks); remote protection is armed from the policy by codeflow remote protect and read back by codeflow doctor.';
  const W = 1000; let o = `<rect class="acc-wash" x="400" y="110" width="200" height="50" rx="10"/><text x="500" y="140" text-anchor="middle" class="b">.codeflow/policy.json</text>`;
  const boxes = [[20, 20, 'Git hooks', '.git/hooks shims, five events'], [20, 200, 'In-session guards', '.claude/settings.json, .codex, .grok/hooks'], [700, 20, 'Scaffolded CI', 'codeflow ci on the hosted run'], [700, 200, 'Remote branch protection', 'codeflow remote protect; read back by codeflow doctor']];
  boxes.forEach(([x, y, t, s]) => { o += `<rect class="node-soft" x="${x}" y="${y}" width="280" height="50" rx="10"/><text x="${x + 14}" y="${y + 21}" class="b">${esc(t)}</text><text x="${x + 14}" y="${y + 40}" class="s">${esc(s)}</text>`; });
  o += `<path class="ink2" d="M300 45H380V110"/><path class="ink2" d="M300 225H380V160"/><path class="ink2" d="M700 45H620V110"/><path class="acc-dash" d="M700 225H620V160"/><text x="500" y="100" text-anchor="middle" class="s">read by hooks and CI; the guards and the remote rules follow it</text>`;
  const wide = `<svg class="wide" viewBox="0 0 ${W} 270" ${figA11y('pa', 'Where each plane runs, around one policy file', desc)}${o}</svg>`;
  let n = `<rect class="acc-wash" x="10" y="8" width="310" height="40" rx="8"/><text x="165" y="33" text-anchor="middle" class="b">.codeflow/policy.json</text>`;
  boxes.forEach(([, , t, s], i) => { const y = 62 + i * 60; n += `<path class="ink2" d="M165 ${y - 14}v14"/><rect class="node-soft" x="10" y="${y}" width="310" height="48" rx="8"/><text x="22" y="${y + 20}" class="b">${esc(t)}</text><text x="22" y="${y + 38}" class="s">${esc(s)}</text>`; });
  const narrow = `<svg class="narrow" viewBox="0 0 330 310" ${figA11y('pan', 'The policy file, then each plane below it', desc)}${n}</svg>`;
  return wide + narrow + dataTable(['Plane', 'Runs from', 'Reads the policy'], boxes.map(([, , t, s], i) => [esc(t), esc(s), i === 3 ? 'armed from it, then read live' : 'yes']), 'Architecture figure as a table');
}
function planesTechnical() {
  const desc = 'The five git-hook shims in event order, each with what it refuses: ' + HOOKS.map(h => `${h[0]} ${h[1]}`).join('; ') + '.';
  const W = 1000; let o = `<path class="ink2" d="M20 40H980"/>`;
  HOOKS.forEach((h, i) => { const x = 20 + i * 192; o += `<circle class="ok-fill" cx="${x + 10}" cy="40" r="7"/><text x="${x}" y="72" class="b">${esc(h[0])}</text>`; const words = h[1].split(' '); let line = '', ly = 92; for (const wd of words) { if ((line + ' ' + wd).length > 26) { o += `<text x="${x}" y="${ly}" class="s">${esc(line)}</text>`; line = wd; ly += 17; } else line = line ? line + ' ' + wd : wd; } o += `<text x="${x}" y="${ly}" class="s">${esc(line)}</text>`; });
  const wide = `<svg class="wide" viewBox="0 0 ${W} 215" ${figA11y('pt', 'The five hook shims in event order', desc)}${o}</svg>`;
  let n = `<path class="ink2" d="M20 10V${10 + HOOKS.length * 44}"/>`; HOOKS.forEach((h, i) => { const y = 20 + i * 44; n += `<circle class="ok-fill" cx="20" cy="${y}" r="6"/><text x="36" y="${y + 4}" class="b">${esc(h[0])}</text>`; });
  const narrow = `<svg class="narrow" viewBox="0 0 330 ${20 + HOOKS.length * 44}" ${figA11y('ptn', 'The five hook shims, listed in event order', desc)}${n}</svg>`;
  return wide + narrow + dataTable(['Hook', 'What it refuses or checks'], HOOKS.map(h => [esc(h[0]), esc(h[1])]), 'Technical figure as a table');
}
const docsView = `<section class="view" id="docs" data-title="A docs topic">
  ${crumbs([['portal', 'Docs portal'], ['docs', 'Git enforcement: the four planes']])}
  <div class="vhead"><h2>Git enforcement: the four planes</h2><span class="chip plain">docs topic</span></div>
  <p class="lede" style="margin-bottom:6px">One portal topic on the same shell: three tabs, each opening with a figure, as decided on 2 Nov (d2). The content is the "Enforcement" section of <code>.codeflow/rules/git-rules.md</code> (lines 6 to 36 in this worktree); nothing here is invented for the specimen.</p>
  ${tabs('docs', [
    ['concept', 'Concept', null, `<p class="lede" style="margin-bottom:10px">Four planes provide defence in depth around every change; two are local fast feedback and two are the server-side boundary.</p><figure class="fig" data-block="Docs concept figure"><div class="fig-head"><span class="fig-t">Four planes around a change</span><span class="fig-n">Figure</span></div>${planesConcept()}<figcaption>The local planes are fast feedback that an agent on the host can edit or skip; required CI and remote rules are the server-side boundary, and only where they are configured for the actor's permissions (git-rules.md:24 to 27).</figcaption></figure><p style="margin-top:14px;color:var(--text-2)">Installed files do not prove active enforcement: verify hook execution, harness trust and event support, CI results and the actual remote rules, and report a missing plane without relaxing task safety or review (git-rules.md:21 to 24). An override environment variable is not authentication and creates no boundary (git-rules.md:31 to 32).</p>`],
    ['architecture', 'Architecture', null, `<p class="lede" style="margin-bottom:10px">Hooks and CI share one policy file and the <code>codeflow ci</code> checks; the guards and the remote rules follow the same policy.</p><figure class="fig" data-block="Docs architecture figure"><div class="fig-head"><span class="fig-t">Where each plane runs</span><span class="fig-n">Figure</span></div>${planesArchitecture()}<figcaption>Hooks and CI share <code>.codeflow/policy.json</code> and the <code>codeflow ci</code> checks (ADR-0017, git-rules.md:11 to 12); the in-session guards are wired in each harness's settings (git-rules.md:8 to 10); the remote plane is armed with <code>codeflow remote protect</code> and read back with <code>codeflow doctor --check remote-perimeter</code> (git-rules.md:27 to 31).</figcaption></figure>`],
    ['technical', 'Technical', null, `<p class="lede" style="margin-bottom:10px">Every tier ships the same armed policy and all five git-hook shims.</p><figure class="fig" data-block="Docs technical figure"><div class="fig-head"><span class="fig-t">The five hook shims in event order</span><span class="fig-n">Figure</span></div>${planesTechnical()}<figcaption>From git-rules.md:12 to 19. The remote plane requires the checks in <code>git.required_checks</code> on a branch that is up to date with its base (git-rules.md:28 to 30).</figcaption></figure>
      <h3 class="section" style="margin-top:22px">Commands</h3>${vt({ cols: 'minmax(0,1fr) minmax(0,1.3fr)', head: ['Command', 'What it does'], label: 'Commands', rows: [['codeflow remote protect', 'arms the remote plane where the host supports it'], ['codeflow doctor --check remote-perimeter', 'reads the live remote rules before a boundary is claimed'], ['codeflow ci', 'the checks the hooks and CI share']].map(r => ({ cells: [`<code>${esc(r[0])}</code>`, `<span class="mute">${esc(r[1])}</span>`] })) })}`],
  ])}
</section>`;

// the portal's What changed page: the docs files that differ between main and the line tip, one numstat
const DC = (F.docsChanged || []).slice().sort((a, b) => (b.added + b.removed) - (a.added + a.removed));
const dcMax = Math.max(1, ...DC.map(d => d.added + d.removed));
function docsSinceFig() {
  const short = f => f.replace(/^docs\//, '').replace(/^decisions\/ADR-(\d+)-.*$/, 'decisions/ADR-$1').replace(/^evidence\/permission-presets\//, 'presets/');
  const desc = `${DC.length} docs files differ between main (${F.read.mainTip}) and the line (${F.read.lineTip}): ` + DC.map(d => `${short(d.file)} ${d.kind}, ${d.added} lines added and ${d.removed} removed`).join('; ') + '.';
  const rh = 24, lx = 300, bw = 560; let o = '';
  DC.forEach((d, i) => { const y = 10 + i * rh; const a = bw * d.added / dcMax, r = bw * d.removed / dcMax; o += `<text x="${lx - 10}" y="${y + 16}" text-anchor="end" class="${d.kind === 'new' ? 'b' : 's'}">${esc(short(d.file))}${d.kind === 'new' ? ' (new)' : ''}</text><rect class="st-line" x="${lx}" y="${y + 4}" width="${Math.max(a, 1).toFixed(1)}" height="14" rx="2"/><rect class="st-block" x="${(lx + a + 2).toFixed(1)}" y="${y + 4}" width="${Math.max(r, d.removed ? 1 : 0).toFixed(1)}" height="14" rx="2"/><text x="${(lx + a + r + 8).toFixed(1)}" y="${y + 16}" class="s">+${d.added} \u2212${d.removed}</text>`; });
  const wide = `<svg class="wide" viewBox="0 0 1000 ${20 + DC.length * rh}" ${figA11y('ds', 'Lines added and removed per docs file, main to the line', desc)}${o}</svg>`;
  let n = ''; DC.forEach((d, i) => { const y = 8 + i * 40; const a = 300 * d.added / dcMax, r = 300 * d.removed / dcMax; n += `<text x="10" y="${y + 12}" class="s">${esc(short(d.file))} +${d.added} \u2212${d.removed}</text><rect class="st-line" x="10" y="${y + 18}" width="${Math.max(a, 1).toFixed(1)}" height="10" rx="2"/><rect class="st-block" x="${(12 + a).toFixed(1)}" y="${y + 18}" width="${Math.max(r, d.removed ? 1 : 0).toFixed(1)}" height="10" rx="2"/>`; });
  const narrow = `<svg class="narrow" viewBox="0 0 330 ${10 + DC.length * 40}" ${figA11y('dsn', 'Lines added and removed per docs file, one row each', desc)}${n}</svg>`;
  return wide + narrow;
}
const docsSinceView = `<section class="view" id="docs-since" data-title="Docs: what changed">
  ${crumbs([['portal', 'Docs portal'], ['docs-since', 'What changed']])}
  <h2 class="section" style="margin-top:14px">What changed in the docs between main and the line</h2>
  <p class="lede" style="margin-bottom:14px">The portal's own change page, on real data: <code>git diff --numstat ${esc(F.read.mainTip)} ${esc(F.read.lineTip)} -- docs/</code>, read at ${esc(when(F.read.recordsRefreshedAt || F.read.finished))}. ${DC.length} files differ: ${DC.filter(d => d.kind === 'new').length} new, ${DC.filter(d => d.kind === 'changed').length} changed, ${DC.filter(d => d.kind === 'removed').length} removed. The marks per view (revision in place, TSK-1121) would sit beside each page the way the deltas do in this report.</p>
  <figure class="fig" data-block="Figure 4, docs changed"><div class="fig-head"><span class="fig-t">Lines added and removed per docs file</span><span class="fig-n">Figure 4</span></div>${docsSinceFig()}<ul class="key"><li><svg viewBox="0 0 22 14"><rect class="st-line" x="1" y="2" width="20" height="10" rx="3"/></svg>lines added</li><li><svg viewBox="0 0 22 14"><rect class="st-block" x="1" y="2" width="20" height="10" rx="3"/></svg>lines removed</li></ul>${dataTable(['File', 'Kind', 'Added', 'Removed'], DC.map(d => [`<code>${esc(d.file)}</code>`, esc(d.kind), String(d.added), String(d.removed)]), 'Figure 4 as a table')}<figcaption>Counts are lines of Markdown or JSON per file; a new ADR counts all its lines as added.</figcaption></figure>
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
        ${row('portal', 'Docs portal layers', R.portalLayers.length, 'docs')}
        ${row('docs', 'A docs topic, three tabs', '', 'docs')}
        ${row('docs-since', 'Docs: what changed', DC.length, 'next')}</section>
      <section class="area" data-area="earlier" data-title="Earlier reports" data-icon="history" data-util>
        <span class="row" data-icon="history" style="cursor:default" title="The earlier page is no longer published"><span class="label">${esc(PREV_W)}, earlier form (no longer published)</span></span></section>`;

// ---- page --------------------------------------------------------------------------------------------------
const DATA = { tasks: T, epics: Object.fromEntries(EP.map(e => [e.id, { id: e.id, title: e.title, themes: e.themes }])), names: R.names, lineRef: F.read.lineRef, prevAt: PREV_W, readAt: READ_AT };
const localStyle = `
.menu .row{height:auto;min-height:30px;white-space:normal;padding:4px 10px;align-items:flex-start} .menu .row svg{margin-top:2px} .menu .row .label{overflow:visible;text-overflow:clip;line-height:1.3;overflow-wrap:anywhere} .menu .row .count{margin-top:2px}
.notice{display:flex;flex-wrap:wrap;gap:8px 14px;align-items:center;margin:12px 0 0;padding:10px 14px;border-radius:var(--r-2);background:var(--accent-wash);color:var(--accent-ink);font-size:var(--fs-1)}
.delta{display:inline-flex;align-items:center;height:20px;padding:0 7px;margin-left:6px;border-radius:var(--r-pill);font-size:var(--fs-0);font-weight:600;letter-spacing:0;font-variant-numeric:tabular-nums;vertical-align:middle;background:var(--accent-wash);color:var(--accent-ink)}
.delta.down{background:var(--sunken);color:var(--text-2)} .hero .num .delta{font-size:var(--fs-0);margin-left:10px}
.chg{display:inline-block;width:7px;height:7px;border-radius:50%;background:var(--accent);margin-right:6px;flex:none}
.outcomes{gap:12px} a.outcome{display:grid;gap:10px;color:inherit;text-decoration:none} a.outcome:hover{text-decoration:none;background:var(--raised);box-shadow:0 0 0 1px var(--accent),var(--shadow-1)}
.outcome .head,.epic .head,.vhead{display:flex;flex-wrap:wrap;align-items:baseline;gap:6px 14px} .vhead{margin:14px 0 12px}
.outcome .head .p,.epic .head .p{font-size:var(--fs-4);font-weight:600;font-variant-numeric:tabular-nums;letter-spacing:-.02em} .outcome .head .t,.epic .head .t{font-weight:600;color:var(--text)} .id{color:var(--text-2);font-size:var(--fs-0);font-weight:500;letter-spacing:.02em;white-space:nowrap} .chip>.id{color:inherit} .lede a,.about a,p a{text-decoration:underline;text-underline-offset:.15em;text-decoration-color:color-mix(in srgb,currentColor 55%,transparent)}
.outcome .nums,.epic .nums{display:flex;flex-wrap:wrap;gap:6px 14px;color:var(--text-2);font-size:var(--fs-1)} .outcome .nums b,.epic .nums b{color:var(--text);font-weight:600;font-variant-numeric:tabular-nums}
.about-rows{display:grid;gap:6px;margin-top:4px} .about-rows .rw{display:grid;grid-template-columns:11ch 1fr;gap:10px;font-size:var(--fs-1);color:var(--text-2)} .about-rows .rw b{color:var(--text-3);font-weight:600;font-size:var(--fs-0);text-transform:uppercase;letter-spacing:.04em;padding-top:2px} .about-rows .rw span{max-width:none} .nr{color:var(--text-3);font-style:italic}
.epic{display:grid;gap:10px}
.crumbs{display:flex;flex-wrap:wrap;gap:4px 8px;align-items:center;margin-top:18px;font-size:var(--fs-1);color:var(--text-2)} .crumbs .sep{color:var(--text-3)} .crumbs [aria-current]{color:var(--text);font-weight:500}
.strip{display:flex;flex-wrap:wrap;gap:10px 22px;margin:0 0 10px;padding:12px 16px;border-radius:var(--r-2);background:var(--surface);box-shadow:var(--shadow-1)} .strip>div{display:grid;gap:2px;min-width:0;max-width:100%} .strip dt{font-size:var(--fs-0);font-weight:600;letter-spacing:.04em;text-transform:uppercase;color:var(--text-3)} .strip dd{margin:0;font-size:var(--fs-1);color:var(--text-2);overflow-wrap:anywhere} .strip dd.nr{color:var(--text-3);font-style:italic} .strip dd.w{max-width:48ch}
.tabpanel[hidden]{display:none} .tab .count{color:var(--text-3);font-size:var(--fs-0);font-weight:600}
.tc{display:inline-flex;align-items:flex-start;gap:8px;min-width:0} .tc .sm{margin-top:5px} .tt{color:var(--text-2);min-width:0;overflow-wrap:anywhere} .tt a{color:var(--text);font-weight:500}
.sm{display:inline-block;width:12px;height:12px;border-radius:3px;flex:none} .sm.st-main{background:var(--text)} .sm.st-line{background:var(--accent)} .sm.st-pr{background:var(--fig-pr);box-shadow:inset 0 0 0 1.2px var(--accent)} .sm.st-todo{background:var(--stage-todo)} .sm.st-block{background:var(--danger)}
.vt .same{color:var(--text-3)} .vt .src{color:var(--text-3);font-size:var(--fs-0)} .vt [role=cell]{min-width:0;overflow-wrap:anywhere}
.blk{display:grid;gap:10px} .blk h3{display:flex;gap:10px;align-items:center;flex-wrap:wrap} .blk .rw{display:grid;grid-template-columns:12ch 1fr;gap:10px;font-size:var(--fs-2)} .blk .rw b{color:var(--text-2);font-weight:600;font-size:var(--fs-1);padding-top:2px} .blk .rw span{color:var(--text-2)} .blk .rw.rec span{color:var(--text)}
.opt .cost{grid-column:2/4;color:var(--text-2);font-size:var(--fs-1)} .opt{grid-template-columns:18px 1fr auto}
.sr{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap}
.streams{grid-template-columns:repeat(auto-fit,minmax(min(100%,420px),1fr))} .when{color:var(--text-3)} .card .points{font-size:var(--fs-1)}
@media (max-width:640px){.when{display:block;font-size:var(--fs-0)}}
.kicker-l{margin:10px 0 2px;text-transform:uppercase;letter-spacing:.04em;font-size:12px}
.scroll-x{overflow-x:auto} .scroll-x svg{min-width:720px}
details>summary{cursor:pointer;color:var(--text-2);font-weight:500;padding:6px 0} details[open]>summary{margin-bottom:10px}
.fig-data{margin-top:10px} .fig-data table{border-collapse:collapse;width:100%;font-size:var(--fs-1)} .fig-data th,.fig-data td{text-align:left;padding:6px 10px;border-bottom:1px solid var(--line);vertical-align:top} .fig-data th{color:var(--text-3);font-size:var(--fs-0);font-weight:600;letter-spacing:.04em;text-transform:uppercase} .fig-data tbody th{color:var(--text);font-size:var(--fs-1);text-transform:none;letter-spacing:0;font-weight:500}
.fig .narrow text{font-size:13.5px}.fig .narrow text.s{font-size:13px}
.about{margin-top:14px} .about p{color:var(--text-2)} .terms summary{font-size:var(--fs-1)}
.list-plain{list-style:none;padding:0;margin:8px 0 0;display:grid;gap:4px;color:var(--text-2);font-size:var(--fs-1)}
@media (max-width:640px){.blk .rw,.about-rows .rw{grid-template-columns:1fr;gap:2px} .strip{gap:8px 16px} .fig-data table,.fig-data thead,.fig-data tbody,.fig-data tr{display:block} .fig-data thead{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0)} .fig-data td,.fig-data tbody th{display:block;border:0;padding:2px 0} .fig-data tr{padding:6px 0;border-bottom:1px solid var(--line)}}`;

const render = `
(() => {
  const D = ${JSON.stringify(DATA)}; const T = D.tasks, E = D.epics, NAMES = D.names;
  ${esc.toString()}
  ${crumbs.toString()}
  ${strip.toString()}
  ${vt.toString()}
  ${idTag.toString()}
  ${stMark.toString()}
  ${locChip.toString()}
  const nameOf = id => NAMES[id] || (T[id] && T[id].title) || (E[id] && E[id].title) || null;
  const named = text => { const t = String(text || ''); return t.replace(/\\b((?:TSK|EPC|CAP|SPC)-\\d+)\\b( AC-\\d+)?/g, (m, id, ac, at) => { const before = t.slice(0, at); const depth = (before.match(/\\(/g) || []).length - (before.match(/\\)/g) || []).length; if (depth > 0 || t[at + m.length] === ')') return m; let n = nameOf(id); if (!n) return m; if (/\\bthe $/i.test(before) && /^the /i.test(n)) n = n.slice(4); return n + ' (' + id + (ac || '') + ')'; }); };
  const page = document.querySelector('.page');
  const taskName = i => T[i] ? '<a href="#k-' + i + '" data-view-link="k-' + i + '">' + esc(T[i].title) + '</a> ' + idTag(i) : esc(nameOf(i) || 'a task not on the line') + ' ' + idTag(i);
  const table = (ids, label) => vt({ cls: 'tasks', cols: 'minmax(0,1fr) auto', head: ['Task', 'Location'], label, rows: ids.map(i => ({ cells: ['<span class="tc">' + stMark(T[i]) + '<span class="tt">' + taskName(i) + '</span></span>', locChip(T[i])] })) });
  const themeKey = (e, name) => 't-' + e + '-' + Object.keys(E[e].themes).indexOf(name);
  function make(id) {
    if (document.getElementById(id)) return;
    let html = null, title = '';
    const tm = id.match(/^t-(EPC-\\d+)-(\\d+)$/), km = id.match(/^k-(TSK-\\d+)$/);
    if (tm && E[tm[1]]) { const e = E[tm[1]], name = Object.keys(e.themes)[+tm[2]], ids = e.themes[name]; if (!name) return; title = name;
      const done = ids.filter(i => T[i].status === 'complete').length, ready = ids.filter(i => T[i].next && T[i].next.state === 'ready');
      html = crumbs([['overview', 'Release'], ['o-' + e.id, e.title], [id, name]]) + '<div class="vhead"><h2>' + esc(name) + '</h2><span class="chip plain">theme</span></div>'
        + '<div class="card about" data-block="Theme: ' + esc(name) + '" style="margin:0 0 14px"><b>What it is</b><p style="margin-top:6px;max-width:none">The tasks the section "' + esc(name) + '" of the epic record of ' + esc(e.title) + ' (' + e.id + ') introduced: ' + ids.length + ' tasks, ' + done + ' complete, ' + ready.length + ' ready to start.</p></div>'
        + strip([['outcome', '<a href="#o-' + e.id + '" data-view-link="o-' + e.id + '">' + esc(e.title) + '</a> ' + idTag(e.id)], ['tasks', String(ids.length)], ['done', done + ' of ' + ids.length], ['ready to start', String(ready.length)]])
        + '<h3 class="section" style="margin-top:22px">' + ids.length + ' tasks</h3>' + table(ids, 'Tasks of the theme'); }
    else if (km && T[km[1]]) { const t = T[km[1]], e = E[t.epic]; title = t.title;
      const parts = [['overview', 'Release']]; if (e) { parts.push(['o-' + e.id, e.title]); parts.push([themeKey(e.id, t.theme), t.theme]); } parts.push([id, t.title]);
      const nr = '<span class="nr">not recorded</span>';
      html = crumbs(parts) + '<div class="vhead"><h2>' + esc(t.title) + '</h2><span class="chip ' + (t.status === 'complete' ? 'ok' : t.status === 'blocked' ? 'danger' : 'plain') + '">' + esc(t.status) + '</span><span class="id">' + t.id + ', ' + esc(t.work_type) + ', created ' + esc(t.created) + '</span></div>'
        // about first, whole: the record's Outcome section
        + '<div class="card about" data-block="' + esc(t.title) + ': about" style="margin:0 0 14px"><b>What it is about</b><p style="margin-top:6px;max-width:none">' + esc(t.about) + '</p><p class="meta" style="margin-top:6px">The record\\'s Outcome section, whole; source in Evidence below.</p></div>'
        + strip([['outcome', e ? '<a href="#o-' + e.id + '" data-view-link="o-' + e.id + '">' + esc(e.title) + '</a> ' + idTag(e.id) : nr], ['theme', e ? '<a href="#' + themeKey(e.id, t.theme) + '" data-view-link="' + themeKey(e.id, t.theme) + '">' + esc(t.theme) + '</a>' : nr], ['location', locChip(t)], ['state', t.next ? esc(t.next.state) + (t.next.state === 'ready' ? ', unclaimed' : '') : t.status === 'complete' ? 'landed' : esc(t.status)], ['lane', nr, 'nr'], ['window', nr, 'nr'], ['record last changed', esc(D.when ? D.when(t.lastMoved) : t.lastMoved)], ['pull request', t.prs.length ? t.prs.map(n => '<a href="https://github.com/sathyassn/codeflow/pull/' + n + '">#' + n + '</a>').join(' ') : 'none open']])
        + '<div class="grid grid-2 section" style="margin-top:14px">'
        + '<div class="card" data-block="' + esc(t.title) + ': criteria"><b>Criteria and status</b><div class="chips" style="margin-top:8px"><span class="chip plain">' + t.criteria + ' acceptance criteria</span></div>' + (t.serves.length ? '<ul class="list-plain">' + t.serves.map(s => '<li>serves ' + esc(named(s)) + '</li>').join('') + '</ul>' : '') + (t.next && t.next.reason ? '<p class="meta" style="margin-top:10px">' + esc(named(t.next.reason.replace(/^task TSK-\\d+ /, 'This task '))) + '</p>' : '') + '</div>'
        + '<div class="card" data-block="' + esc(t.title) + ': depends"><b>Depends on</b>' + (t.depends_on.length ? '<ul class="list-plain">' + t.depends_on.map(d => '<li>' + (T[d] && T[d].status === 'complete' ? '<i class="sm st-main" title="complete"></i> ' : '<i class="sm st-todo" title="not complete"></i> ') + taskName(d) + '</li>').join('') + '</ul>' : '<p class="meta" style="margin-top:8px">a root: no predecessor</p>') + '</div>'
        + '</div>'
        + '<div class="card section" data-block="' + esc(t.title) + ': evidence" style="margin-top:14px"><b>Evidence</b>' + vt({ cols: '11ch minmax(0,1fr)', head: ['Fact', 'Source'], label: 'Evidence', rows: [{ cells: ['<span class="mute">record</span>', '<code>project-management/tasks/' + t.id + '.md</code> on the line'] }, { cells: ['<span class="mute">read at</span>', '<span class="mute">' + esc(D.readAt) + '</span>'] }, { cells: ['<span class="mute">location</span>', '<span class="mute">from origin/main and the open pull requests' + (t.prs.length ? ': ' + t.prs.map(n => '<a href="https://github.com/sathyassn/codeflow/pull/' + n + '">#' + n + '</a>').join(', ') : '') + '</span>'] }] }) + '<pre style="margin-top:10px" tabindex="0" role="region" aria-label="Evidence command"><code>git -C codeflow show ' + esc(D.lineRef) + ':project-management/tasks/' + t.id + '.md</code></pre></div>'; }
    if (html == null) return;
    const s = document.createElement('section'); s.className = 'view'; s.id = id; s.dataset.title = title; s.innerHTML = html; page.appendChild(s);
  }
  ${when.toString()} D.when = when;
  window.cfMake = make;
  if (location.hash.length > 1) make(location.hash.slice(1));
  document.addEventListener('click', e => { const a = e.target.closest('[data-view-link]'); if (a) make(a.dataset.viewLink); }, true);
  // tabs inside a view: one tab stop (roving tabindex), arrows move, the selected panel shows
  const selectTab = b => { const list = b.closest('[role=tablist]'); list.querySelectorAll('[role=tab]').forEach(x => { const on = x === b; x.setAttribute('aria-selected', String(on)); x.tabIndex = on ? 0 : -1; const p = document.getElementById(x.getAttribute('aria-controls')); if (p) p.hidden = !on; }); };
  document.addEventListener('click', e => { const b = e.target.closest('.tab[role=tab]'); if (b) selectTab(b); });
  document.addEventListener('keydown', e => { const b = e.target.closest('.tab[role=tab]'); if (!b) return; const tabs = [...b.closest('[role=tablist]').querySelectorAll('[role=tab]')]; let n = null; if (e.key === 'ArrowRight') n = tabs[(tabs.indexOf(b) + 1) % tabs.length]; else if (e.key === 'ArrowLeft') n = tabs[(tabs.indexOf(b) + tabs.length - 1) % tabs.length]; else if (e.key === 'Home') n = tabs[0]; else if (e.key === 'End') n = tabs[tabs.length - 1]; if (!n) return; n.focus(); selectTab(n); e.preventDefault(); });
})();`;

const scriptJs = `
(() => {
  const RECHECK = ${JSON.stringify(R.recheck)}, FILE = ${JSON.stringify(R.answersFile)}, AS_OF = ${JSON.stringify(F.read.finished)};
  const form = document.getElementById('decisions'), out = document.getElementById('script-text');
  const q = s => "'" + String(s).replace(/'/g, "'\\\\''") + "'";
  function build() {
    const answers = [...form.querySelectorAll('input[type=radio]:checked')].map(i => ({ id: i.name, value: i.value, label: i.dataset.label }));
    const changes = (form.querySelector('textarea[name=changes]').value || '').trim();
    let t = '#!/bin/sh\\n# CodeFlow 4.0.0 status prototype, facts as of ' + AS_OF + '. Part 1 rechecks (read only). Part 2 records only the decisions answered on the page.\\nset -eu\\nW=/Volumes/DATA/Local/software-workspace/projects/agentic-systems\\n\\n# Part 1 of 2: recheck\\n' + RECHECK + '\\n\\n# Part 2 of 2: your answers\\n';
    if (!answers.length && !changes) t += "echo 'No decision was answered on the page: nothing is written, nothing is committed.'\\n";
    else {
      t += 'F=' + FILE + '\\n[ "$(git -C "$W" rev-parse --abbrev-ref HEAD)" = "integration/workspace" ] || { echo "the workspace is not on integration/workspace; nothing written"; exit 1; }\\nmkdir -p "$W/$(dirname "$F")"\\n[ -f "$W/$F" ] || printf \\'%s\\\\n\\' \\'# Operator answers to the status prototype walk-through (TSK-1122)\\' \\'\\' > "$W/$F"\\n';
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
<meta name="description" content="A design prototype for the status report, on real 4.0.0 facts read at ${esc(READ_AT)}: outcomes first, a two-level side menu, tabs, drill-down to epic, theme, task and evidence, decisions as form controls, one script, change marks, ten report kinds and a docs topic on the same shell.">
<script>(()=>{try{const m=localStorage.getItem('cf4.mode'),t=localStorage.getItem('cf4.theme'),a=localStorage.getItem('cf4.accent'),s=localStorage.getItem('cf4.size');const r=document.documentElement;if(m&&m!=='system')r.setAttribute('data-mode',m);if(t)r.setAttribute('data-theme',t);if(a)r.setAttribute('data-accent',a);if(s)r.setAttribute('data-size',s);}catch(e){}})();</script>
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
    <div class="notice" data-block="Prototype notice"><span class="chip accent">prototype</span><span>A design prototype for the status report, built outside the runtime on real facts read once at <b>${esc(READ_AT)}</b>. Not a report to act on.</span></div>
    <div class="meta"><span>line tip <b>${esc(F.read.lineTip)}</b>, main <b>${esc(F.read.mainTip)}</b></span><span>records on both refs, GitHub (read only), <code>codeflow work next</code> and the four Coordinator trackers</span><span>previous report <b>${esc(PREV_W)}</b></span></div>
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
    ${specimenViews}
    ${portalView}
    ${docsView}
    ${docsSinceView}
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
console.log('wrote', OUT, html.length, 'bytes;', Object.keys(T).length, 'tasks,', EP.map(e => `${e.id}: ${Object.keys(e.themes).length} themes`).join(', '), ';', changed.length, 'changed numbers;', kinds.length, 'kinds,', DC.length, 'docs files changed');
