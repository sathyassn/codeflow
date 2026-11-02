// TSK-1122 prototype: the docs topic drawings and the docs change figure, kept from round 2 as drawings (not layout).
// Usage: require('./figures.cjs')({ esc, figA11y, dataTable, F, DC, dcMax })
module.exports = ({ esc, figA11y, dataTable, F, DC, dcMax }) => {
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

return { PLANES, HOOKS, planesConcept, planesArchitecture, planesTechnical, docsSinceFig };
};
