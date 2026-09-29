import assert from "node:assert/strict";

// Exercise the production shell on built routes, not a second chrome fixture.
export async function verifyChrome(page, urls, engine, altitudeRoute = null) {
  const originalReducedMotion = await page.evaluate(() => matchMedia("(prefers-reduced-motion: reduce)").matches);
  const route = altitudeRoute ?? urls.find((url) => /\/architecture\/$/.test(url)) ?? urls[0];
  assert.ok(route, `${engine}: chrome needs a built route`);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(route);
  await page.locator('[data-cf-mounted]').waitFor();
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  const skip = page.getByRole('link', { name: 'Skip to content', exact: true });
  assert.equal(await skip.count(), 1, 'one unobscured skip link');
  // macOS WebKit uses Option-Tab to include links in keyboard navigation.
  await page.keyboard.press(engine === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab');
  assert.equal(await skip.evaluate((el) => el === document.activeElement), true);
  await page.keyboard.press('Enter');
  assert.equal(await page.locator('main').evaluate((el) => el === document.activeElement), true);
  const header = page.locator('.cf-header');
  assert.equal(await header.evaluate((el) => el.getBoundingClientRect().height), 52);
  const display = page.locator('[data-cf-open="display"]');
  await display.click();
  for (const [name, value, attr] of [
    ['skin', 'sage', 'data-cfp-skin'], ['typeface', 'plex', 'data-cfp-typeface'],
    ['scale', 'large', 'data-cfp-scale'], ['appearance', 'dark', 'data-theme'],
  ]) {
    await page.getByTestId(`${name}-${value}`).click();
    assert.equal(await page.locator('html').getAttribute(attr), value);
  }
  await page.getByTestId('skin-sage').press('Home');
  assert.equal(await page.locator('html').getAttribute('data-cfp-skin'), 'graphite');
  await page.keyboard.press('Escape');
  assert.equal(await display.evaluate((el) => el === document.activeElement), true);
  // Observe preferences before the first external runtime executes on another route.
  await page.addInitScript(() => {
    window.__chromePaints = [];
    new MutationObserver(() => {
      const root = document.documentElement;
      if (root?.hasAttribute('data-cfp-skin')) window.__chromePaints.push([
        root.getAttribute('data-cfp-skin'), root.getAttribute('data-theme'),
        root.getAttribute('data-cfp-typeface'), root.getAttribute('data-cfp-scale'),
      ]);
    }).observe(document, { subtree: true, attributes: true, childList: true });
  });
  await page.goto(urls.find((url) => url !== route) ?? route);
  await page.locator('[data-cf-mounted]').waitFor();
  const paints = await page.evaluate(() => window.__chromePaints);
  assert.ok(paints.length > 0);
  assert.ok(paints.every((value) => JSON.stringify(value) === JSON.stringify(['graphite', 'dark', 'plex', 'large'])), `${engine}: preferences flashed`);
  // Update-state controls cover every old skin and explicit font independently.
  for (const [key, saved, skin, face] of [
    ...[['instrument', 'graphite'], ['editorial', 'slate'], ['ink', 'sage'], ['technical', 'graphite']].map(([old, skin]) => ['cf-portal-skin', old, skin, 'plex']),
    ...[['instrument', 'archivo'], ['editorial', 'inter'], ['plex', 'plex']].map(([old, face]) => ['cf-portal-typeface', old, 'sage', face]),
  ]) {
    await page.evaluate(({ key, saved }) => {
      localStorage.clear(); localStorage.setItem('cf-portal-skin', 'sage'); localStorage.setItem('cf-portal-typeface', 'plex'); localStorage.setItem(key, saved);
    }, { key, saved });
    await page.reload();
    await page.locator('[data-cf-mounted]').waitFor();
    const paints = await page.evaluate(() => window.__chromePaints);
    assert.ok(paints.length > 0 && paints.every(p => p[0] === skin && p[2] === face), `${engine}: ${key}=${saved} first paint`);
    await page.locator('[data-cf-open="display"]').click();
    assert.equal(await page.getByTestId(`skin-${skin}`).getAttribute('aria-checked'), 'true');
    assert.equal(await page.getByTestId(`typeface-${face}`).getAttribute('aria-checked'), 'true');
    await page.keyboard.press('Escape');
  }
  await page.evaluate(() => localStorage.clear());
  await page.goto(route);
  await page.locator('[data-cf-mounted]').waitFor();

  for (const name of ['nav', 'toc']) {
    const toggle = page.locator(`[data-cf-toggle="${name}"]`);
    await toggle.click();
    assert.equal(await page.locator('html').getAttribute(`data-cf-${name}`), 'collapsed');
    await page.reload();
    await page.locator('[data-cf-mounted]').waitFor();
    assert.equal(await page.locator('html').getAttribute(`data-cf-${name}`), 'collapsed');
    const sheet = page.locator(`[data-cf-sheet="${name}"]`);
    await page.locator(`[data-cf-peek="${name}"]`).hover();
    await sheet.locator('.cf-sheet-head').waitFor({ state: 'visible' });
    assert.ok((await sheet.getAttribute('class')).includes('is-open'));
    await page.keyboard.press('Escape');
    await toggle.focus();
    await toggle.press('Enter');
    assert.equal(await sheet.getAttribute('aria-modal'), 'true');
    await page.keyboard.press('Escape');
    await page.keyboard.press(name === 'nav' ? '[' : ']');
    assert.ok((await sheet.getAttribute('class')).includes('is-open'));
    await page.keyboard.press('Escape');
    await toggle.click();
  }
  const disclosure = page.locator('#cf-nav .cf-nav-chev').first();
  await disclosure.click();
  assert.equal(await disclosure.getAttribute('aria-expanded'), 'false');
  await page.reload();
  assert.equal(await disclosure.getAttribute('aria-expanded'), 'false');
  await disclosure.click();
  assert.ok(await page.locator('#cf-nav .cf-row-count').count() > 0);
  const grip = page.getByRole('separator', { name: 'Resize navigation' });
  await grip.press('Home');
  assert.equal(await grip.getAttribute('aria-valuenow'), '220');
  await grip.press('ArrowRight');
  assert.equal(await grip.getAttribute('aria-valuenow'), '228');
  await grip.press('End');
  await grip.press('ArrowRight');
  assert.equal(await grip.getAttribute('aria-valuenow'), '360');
  await page.reload();
  assert.equal(await grip.getAttribute('aria-valuenow'), '360');
  const box = await grip.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + 40);
  await page.mouse.down();
  await page.mouse.move(100, box.y + 40, { steps: 8 });
  await page.mouse.up();
  assert.equal(await grip.getAttribute('aria-valuenow'), '220');
  const minimumBox = await grip.boundingBox();
  await page.mouse.move(minimumBox.x + minimumBox.width / 2, minimumBox.y + 40);
  await page.mouse.down();
  await page.mouse.move(600, minimumBox.y + 40, { steps: 8 });
  await page.mouse.up();
  assert.equal(await grip.getAttribute('aria-valuenow'), '360');
  await grip.dblclick();

  const tabs = page.locator('.portal-altitude-tabs [role="tab"]');
  const hasAltitudes = await tabs.count() > 0;
  if (hasAltitudes) {
  assert.equal(await tabs.count(), 3);
  for (const tab of await tabs.all()) {
    const controls = await tab.getAttribute('aria-controls');
    assert.equal(await page.locator('#' + controls).getAttribute('aria-labelledby'), await tab.getAttribute('id'));
  }
  await tabs.first().focus();
  await tabs.first().press('ArrowRight');
  assert.equal(await tabs.nth(1).getAttribute('aria-selected'), 'true');
  assert.match(new URL(page.url()).hash, /architecture/);
  await page.reload();
  assert.equal(await tabs.nth(1).getAttribute('aria-selected'), 'true');
  const style = await tabs.nth(1).evaluate((el) => {
    const s = getComputedStyle(el);
    return { font: s.fontFamily, body: getComputedStyle(document.body).fontFamily, weight: s.fontWeight, transform: s.textTransform, shadow: s.boxShadow, text: el.textContent.trim() };
  });
  assert.equal(style.font, style.body);
  assert.equal(style.weight, '600');
  assert.equal(style.transform, 'none');
  assert.equal(style.text, 'Architecture');
  assert.notEqual(style.shadow, 'none');
  const idle = await tabs.nth(2).evaluate((el) => { const s = getComputedStyle(el); return [s.backgroundColor, s.borderWidth === "0px" ? "none" : s.borderColor, s.boxShadow, s.fontWeight]; });
  await tabs.nth(2).hover();
  const hovered = await tabs.nth(2).evaluate((el) => { const s = getComputedStyle(el); return [s.backgroundColor, s.borderWidth === "0px" ? "none" : s.borderColor, s.boxShadow, s.fontWeight]; });
  assert.equal(idle[3], '500', 'idle tabs use the kit medium weight');
  assert.deepEqual(hovered, idle, 'tab hover changes text color only');
  await tabs.nth(2).click();
  }
  // The outline retains headings outside panels, in document order.
  assert.deepEqual(await page.locator('#cf-toc .cf-toc-link').evaluateAll(links => links.map(a => a.dataset.target)), await page.locator('main').evaluate(main => [...main.querySelectorAll('h2[id], h3[id]')].filter(h => !h.closest('.portal-altitude') || !h.closest('.portal-altitude').hidden).map(h => h.id)));
  for (const group of await page.locator('#cf-nav .cf-nav-group .cf-nav-group').all()) {
    const hierarchy = await group.evaluate(g => {
      const parent = g.parentElement.closest('.cf-nav-group');
      const label = g.querySelector('.cf-kicker');
      return { delta: g.getBoundingClientRect().left - parent.getBoundingClientRect().left, gap: parseFloat(getComputedStyle(g).marginTop), weight: getComputedStyle(label).fontWeight, transform: getComputedStyle(label).textTransform };
    });
    assert.ok(hierarchy.delta >= 12 && hierarchy.gap >= 8, 'nested groups have an inset and a top gap');
    assert.equal(hierarchy.weight, '500'); assert.equal(hierarchy.transform, 'none');
  }
  const crumbs = page.locator('.cf-crumbs a');
  for (const crumb of await crumbs.all()) {
    assert.notEqual(new URL(await crumb.getAttribute('href'), page.url()).pathname, new URL(page.url()).pathname, 'ancestor breadcrumbs do not link to the current page');
  }
  const outline = page.locator('#cf-toc .cf-toc-link');
  if (await outline.count() > 1) {
    const last = outline.last();
    const id = await last.getAttribute('data-target');
    await last.click();
    await page.waitForFunction((id) => document.querySelector('#cf-toc .cf-toc-link.is-active')?.dataset.target === id, id);
  }

  for (const open of ['click', '/', 'Control+k', 'Meta+k']) {
    if (open === 'click') await page.locator('.cf-search-trigger').click();
    else await page.keyboard.press(open);
    await page.locator('.cf-search-input').waitFor({ state: 'visible' });
    await page.keyboard.press('Escape');
  }
  if (hasAltitudes) {
  await page.keyboard.press('/');
  await page.locator('.cf-search-input').fill('technical');
  await page.locator('.cf-hit mark').first().waitFor();
  assert.ok(await page.locator('.cf-search-group').count() > 0);
  const technicalHit = page.locator('.cf-hit').filter({ has: page.locator('.cf-chip', { hasText: 'Technical' }) }).first();
  await technicalHit.waitFor();
  await technicalHit.hover();
  await page.keyboard.press('Enter');
  await page.waitForURL((url) => !!url.hash);
  await page.locator('.portal-altitude[data-altitude="technical"]:not([hidden])').waitFor();
  }

  await page.goto(route);
  await page.locator('[data-cf-mounted]').waitFor();
  const preview = page.locator('.portal-id-preview:visible').first();
  if (await preview.count()) {
    const link = preview.locator(':scope > a');
    const card = preview.locator(':scope > span');
    await link.hover();
    await card.waitFor({ state: 'visible' });
    await page.mouse.move(0, 0);
    await link.focus();
    await card.waitFor({ state: 'visible' });
    await page.setViewportSize({ width: 375, height: 812 });
    assert.equal(await card.evaluate((el) => getComputedStyle(el).position), 'fixed');
    assert.ok(await card.evaluate((el) => el.getBoundingClientRect().bottom <= innerHeight));
    await page.keyboard.press('Escape');
    await card.waitFor({ state: 'hidden' });
  }

  // Every built route must retain a full-width phone column. Wait for the kit's
  // panel motion to settle; the reduced-motion run below has no such delay.
  await page.setViewportSize({ width: 375, height: 812 });
  for (const url of urls) {
    await page.goto(url);
    await page.locator('[data-cf-mounted]').waitFor();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth), 375, `${engine}: overflow ${url}`);
    const breadcrumbState = await page.evaluate(() => {
      const current = document.querySelector('#cf-nav a[aria-current="page"]');
      const labels = [];
      for (let group = current?.closest('.cf-nav-group'); group; group = group.parentElement.closest('.cf-nav-group')) {
        labels.push(group.querySelector(':scope > .cf-nav-group-head .cf-kicker').firstChild.textContent.trim());
      }
      return { labels, crumbs: [...document.querySelectorAll('.cf-crumbs li')].slice(1, -1).map(li => ({ label: li.textContent.trim(), href: li.querySelector('a')?.href })) };
    });
    for (const crumb of breadcrumbState.crumbs) {
      assert.ok(breadcrumbState.labels.some(label => label === crumb.label || label.replace(/[-_]/g, ' ').replace(/\b\w/g, c => c.toUpperCase()) === crumb.label), 'breadcrumb uses the configured group label or a readable folder label');
      if (crumb.href) assert.notEqual(new URL(crumb.href).pathname, new URL(page.url()).pathname, 'no ancestor self-link');
    }

  }
  for (const name of ['nav', 'toc']) {
    await page.locator(`[data-cf-toggle="${name}"]`).click();
    assert.ok((await page.locator(`[data-cf-sheet="${name}"]`).getAttribute('class')).includes('is-open'));
    await page.keyboard.press('Escape');
  }
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await display.click();
  assert.equal(await page.locator('.cf-display').evaluate((el) => getComputedStyle(el).transitionDuration.split(',').every((v) => parseFloat(v) === 0)), true);
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => document.getAnimations().filter((animation) => animation.effect?.getKeyframes().some((frame) => frame.transform !== undefined) && animation.effect.getComputedTiming().activeDuration > 0).length), 0);
  await page.emulateMedia({ reducedMotion: originalReducedMotion ? 'reduce' : 'no-preference' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(route);
  await page.locator('[data-cf-mounted]').waitFor();
  assert.ok(await page.locator('main').evaluate((el) => el.getBoundingClientRect().width >= 646), `${engine}: desktop companion width`);
  const companion = page.locator('.cf-companion figure.cf-fig:visible').first();
  let companionGeometry = null;
  if (await companion.count()) {
    companionGeometry = await companion.evaluate((el) => ({
      width: el.getBoundingClientRect().width,
      wide: [...el.querySelectorAll('.cf-fig-svg--wide')].some(svg => getComputedStyle(svg).display !== 'none'),
      narrow: [...el.querySelectorAll('.cf-fig-svg--narrow')].some(svg => getComputedStyle(svg).display !== 'none'),
    }));
    assert.ok(companionGeometry.width >= 646, `${engine}: companion width ${companionGeometry.width}`);
    assert.equal(companionGeometry.wide, true);
    assert.equal(companionGeometry.narrow, false);
  }
  return { nestedGroups: await page.locator('#cf-nav .cf-nav-group .cf-nav-group').count(), headingsOutsidePanels: await page.locator('main').evaluate(main => [...main.querySelectorAll('h2[id], h3[id]')].filter(h => !h.closest('.portal-altitude')).length), controls: 'pass', hasAltitudes, routesAt375: urls.length, desktopColumnMinimum: 646, companionGeometry };
}
