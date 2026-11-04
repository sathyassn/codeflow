// Record prose as blocks, so a Markdown list, table or subheading in a record stays one on the page.
// A block is a string (a paragraph), { heading }, { list: 'ul'|'ol', start, items: [{ text, sub }] } or { table: { head, rows } }.
const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
const RULE = /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/;
const cells = line => line.trim().replace(/^\||\|$/g, '').split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, '|'));
const indentOf = line => line.match(/^\s*/)[0].length;
const clean = s => s.replace(/\s+/g, ' ').trim();

function parseList(lines, at, base) {
  const first = lines[at].match(ITEM), ordered = /\d/.test(first[2]);
  const list = { list: ordered ? 'ol' : 'ul', start: ordered ? parseInt(first[2], 10) : 1, items: [] };
  let i = at, item = null, afterBlank = false;
  while (i < lines.length) {
    const line = lines[i], m = line.match(ITEM);
    if (!line.trim()) { afterBlank = true; i++; continue; }
    const indent = indentOf(line);
    if (m && indent < base) break;
    if (m && indent <= base + 1) {
      if (/\d/.test(m[2]) !== ordered) break;
      item = { text: m[3], sub: [] }; list.items.push(item); afterBlank = false; i++; continue;
    }
    if (m && item) { const [sub, next] = parseList(lines, i, indent); item.sub.push(sub); afterBlank = false; i = next; continue; }
    if (indent <= base && (afterBlank || /^\s*(\||#{1,6}\s)/.test(line))) break; // back at the margin: the list has ended
    if (afterBlank) item.sub.push(clean(line)); // a later paragraph of the same item
    else if (item.sub.length && typeof item.sub[item.sub.length - 1] === 'string') item.sub[item.sub.length - 1] += ' ' + clean(line);
    else item.text += ' ' + clean(line); // the item's wrapped line
    afterBlank = false; i++;
  }
  for (const it of list.items) it.text = clean(it.text);
  return [list, i];
}

function blocks(text) {
  const lines = String(text || '').replace(/```[\s\S]*?```/g, '').split('\n');
  const out = []; let para = [];
  const flush = () => { if (para.length) out.push(clean(para.join(' '))); para = []; };
  for (let i = 0; i < lines.length;) {
    const line = lines[i], item = line.match(ITEM), head = line.match(/^\s*#{1,6}\s+(.*?)\s*#*\s*$/);
    if (!line.trim()) { flush(); i++; continue; }
    if (head) { flush(); out.push({ heading: clean(head[1]) }); i++; continue; }
    if (/^\s*\|/.test(line) && i + 1 < lines.length && RULE.test(lines[i + 1]) && lines[i + 1].includes('-')) {
      flush(); const table = { head: cells(line), rows: [] }; i += 2;
      while (i < lines.length && /^\s*\|/.test(lines[i])) table.rows.push(cells(lines[i++]));
      out.push({ table }); continue;
    }
    // as in CommonMark, a numbered line interrupts a paragraph only when it starts at 1, so wrapped prose stays prose
    if (item && (!para.length || !/\d/.test(item[2]) || parseInt(item[2], 10) === 1)) { flush(); const [list, next] = parseList(lines, i, indentOf(line)); out.push(list); i = next; continue; }
    para.push(line); i++;
  }
  flush();
  return out;
}

// One block's words as one line, for a lead sentence or a length comparison.
function blockText(b) {
  if (typeof b === 'string') return b;
  if (b.heading) return b.heading;
  if (b.table) return [b.table.head, ...b.table.rows].map(r => r.join(' ')).join(' ');
  return b.items.map(it => [it.text, ...it.sub.map(blockText)].join(' ')).join(' ');
}

module.exports = { blocks, blockText };
