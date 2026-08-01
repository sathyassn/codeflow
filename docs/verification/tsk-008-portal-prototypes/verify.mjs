import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const prototypeRoot = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(prototypeRoot, '../../..');
const prototypeNames = ['guided-path', 'system-atlas', 'evidence-notebook'];
const documents = new Map(
  prototypeNames.map((name) => [name, readFileSync(join(prototypeRoot, name, 'index.html'), 'utf8')]),
);

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function textFromMarkup(markup) {
  return markup
    .replace(/<[^>]+>/g, '')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&');
}

const allMarkup = [...documents.values()].join('\n');
for (const invented of ['RunPhase', 'DelegateRun', 'launch.json', 'terminal.json', 'd1f3e074']) {
  assert(!allMarkup.includes(invented), `invented or stale fixture token remains: ${invented}`);
}

const delegateSource = readFileSync(join(repositoryRoot, 'crates/codeflow-core/src/delegate.rs'), 'utf8');
const sourceExcerpt = delegateSource.split('\n').slice(23, 42).join('\n');
const atlasExcerptMatch = documents
  .get('system-atlas')
  .match(/<div class="map-canvas code-view"[\s\S]*?<pre><code>([\s\S]*?)<\/code><\/pre>/);
assert(atlasExcerptMatch, 'System Atlas code excerpt is missing');
assert(textFromMarkup(atlasExcerptMatch[1]) === sourceExcerpt, 'System Atlas excerpt drifted from delegate.rs:24-42');

for (const filename of ['settings.json', 'ready.json', 'request.json', 'accepted.json', 'result.json']) {
  assert(delegateSource.includes(`"${filename}"`), `delegate.rs no longer names ${filename}`);
  assert(documents.get('guided-path').includes(filename), `Guided Path no longer names ${filename}`);
}

const capabilities = readFileSync(join(repositoryRoot, 'docs/capabilities.md'), 'utf8');
assert((capabilities.match(/^## CAP-/gm) || []).length === 14, 'expected 14 capability records');
assert((capabilities.match(/^status: shipped$/gm) || []).length === 13, 'expected 13 shipped capabilities');
assert((capabilities.match(/^status: building$/gm) || []).length === 1, 'expected one building capability');
assert(documents.get('system-atlas').includes('13 shipped capabilities · 1 building'), 'Atlas capability count is stale');
assert(documents.get('guided-path').includes('Prototype base e96db64b'), 'Guided Path prototype base is stale');

for (const [name, markup] of documents) {
  assert(markup.indexOf('theme-init.js') < markup.indexOf('base.css'), `${name} does not select theme before CSS`);
  assert(/<form class="search-head" role="search" data-search-form>/.test(markup), `${name} lacks a search landmark`);
  assert(/data-search-status aria-live="polite"/.test(markup), `${name} lacks a live search status`);
  assert(!/data-theme-toggle[^>]*aria-pressed/.test(markup), `${name} double-signals theme state`);

  for (const match of markup.matchAll(/href="(\/[^"]+)"/g)) {
    const sourcePath = decodeURIComponent(match[1].split('#', 1)[0]);
    assert(existsSync(join(repositoryRoot, sourcePath)), `${name} links to missing source ${sourcePath}`);
  }
}

const digest = createHash('sha256')
  .update([...documents.values()].join('\n'))
  .digest('hex');
console.log(`prototype verification: PASS (${prototypeNames.length} directions, sha256:${digest})`);
