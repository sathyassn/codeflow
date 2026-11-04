#!/usr/bin/env node
// Usage: node record-blocks.test.cjs   (exits non-zero on the first failure)
const assert = require('assert');
const { blocks, blockText } = require('./record-blocks.cjs');

assert.deepStrictEqual(blocks('One wrapped\nparagraph.\n\nAnother.'), ['One wrapped paragraph.', 'Another.']);
assert.deepStrictEqual(blocks('Lead:\n- first\n  wrapped\n- second\n\nAfter.'), ['Lead:', { list: 'ul', start: 1, items: [{ text: 'first wrapped', sub: [] }, { text: 'second', sub: [] }] }, 'After.']);
assert.deepStrictEqual(blocks('6. six\n7. seven'), [{ list: 'ol', start: 6, items: [{ text: 'six', sub: [] }, { text: 'seven', sub: [] }] }]);
assert.deepStrictEqual(blocks('- a\n\n- b'), [{ list: 'ul', start: 1, items: [{ text: 'a', sub: [] }, { text: 'b', sub: [] }] }], 'a blank line between items is one list');
assert.deepStrictEqual(blocks('- a\n  - inner\n- b'), [{ list: 'ul', start: 1, items: [{ text: 'a', sub: [{ list: 'ul', start: 1, items: [{ text: 'inner', sub: [] }] }] }, { text: 'b', sub: [] }] }]);
assert.deepStrictEqual(blocks('Fixed in release\n2. Later words stay prose.'), ['Fixed in release 2. Later words stay prose.'], 'a wrapped number is not a list');
assert.deepStrictEqual(blocks('A - B stays one paragraph.'), ['A - B stays one paragraph.'], 'an inline hyphen is not a list');
assert.deepStrictEqual(blocks('### Part\n\nText.'), [{ heading: 'Part' }, 'Text.']);
assert.deepStrictEqual(blocks('| A | B |\n|---|---|\n| 1 | x \\| y |\n\nAfter.'), [{ table: { head: ['A', 'B'], rows: [['1', 'x | y']] } }, 'After.']);
assert.deepStrictEqual(blocks('Before.\n\n```text\n- not a list\n```\n\nAfter.'), ['Before.', 'After.'], 'fenced figures are left out');
assert.strictEqual(blockText(blocks('- a\n- b')[0]), 'a b');
console.log('record-blocks: all checks pass');
