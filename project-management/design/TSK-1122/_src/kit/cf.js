/* CodeFlow utility surface runtime: prefs (mode, theme, accent, size), the left menu (resize, rail, peek with hover
   intent), the top bar, views with transitions, tabs, the display popover, forms, copy, Comment mode. Plain ES2020, no framework. */
(() => {
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const root = document.documentElement, store = { get: k => { try { return localStorage.getItem('cf4.' + k); } catch { return null; } }, set: (k, v) => { try { v == null ? localStorage.removeItem('cf4.' + k) : localStorage.setItem('cf4.' + k, v); } catch {} } };
  const ACCENTS = { graphite: ['green', 'plum'], slate: ['green', 'violet'], sage: ['green', 'plum'] }; // the operator's 2026-11-02 decision: deep green first in every theme
  const prefs = { mode: store.get('mode') || 'system', theme: store.get('theme') || 'graphite', accent: store.get('accent') || '', size: store.get('size') || 'default' };
  const sysDark = matchMedia('(prefers-color-scheme: dark)');
  function applyPrefs() {
    prefs.mode === 'system' ? root.removeAttribute('data-mode') : root.setAttribute('data-mode', prefs.mode);
    root.setAttribute('data-theme', prefs.theme);
    const def = ACCENTS[prefs.theme][0]; const acc = ACCENTS[prefs.theme].includes(prefs.accent) ? prefs.accent : def;
    acc === def ? root.removeAttribute('data-accent') : root.setAttribute('data-accent', acc);
    root.setAttribute('data-size', prefs.size);
    root.classList.toggle('sys-dark', prefs.mode === 'system' && sysDark.matches);
    $$('[data-pref]').forEach(b => b.setAttribute('aria-pressed', String(prefs[b.dataset.pref] === b.dataset.value || (b.dataset.pref === 'accent' && acc === b.dataset.value))));
    $$('[data-accent-pills]').forEach(box => { box.innerHTML = ACCENTS[prefs.theme].map(a => `<button class="pill" type="button" data-pref="accent" data-value="${a}" aria-pressed="${a === acc}">${a[0].toUpperCase() + a.slice(1)}</button>`).join(''); });
  }
  applyPrefs(); sysDark.addEventListener('change', applyPrefs);
  document.addEventListener('click', e => { const b = e.target.closest('[data-pref]'); if (!b) return; prefs[b.dataset.pref] = b.dataset.value; store.set(b.dataset.pref, b.dataset.value); applyPrefs(); });

  /* Top bar: one component for every page (NAV-SPEC.md "Top bar"). A page writes only
     <header class="bar" data-brand="present"><span class="crumb">…</span></header>, optionally with a text
     nav.bar-nav; cf.js draws the controls: the panel-left toggle, the title and crumb, and one display gear that
     opens Mode and Themes. Any other control a page leaves in the bar is removed. Comment, on pages with a
     comments rail, is one text button at the bottom right of the content, with C as its shortcut. */
  (() => {
    const sh = $('.shell'); if (!sh) return;
    let bar = $('header.bar', sh); if (!bar) { bar = document.createElement('header'); bar.className = 'bar'; sh.prepend(bar); }
    const E = v => String(v).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
    const d = bar.dataset, brand = E(d.brand || 'present');
    const crumb = E(d.crumb || '') + (d.crumbView != null ? `${d.crumb ? ' / ' : ''}<span data-crumb>${E(d.crumbView)}</span>` : '');
    let links = []; try { links = JSON.parse(d.links || '[]'); } catch {}
    bar.innerHTML = '<button class="ibtn" type="button" data-menu-toggle aria-pressed="true" aria-expanded="true" aria-label="Hide navigation" title="Hide navigation"><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="15" rx="3"/><path d="M9.5 4.5v15"/></svg></button>'
      + `<div class="bar-title"><span class="brand">CodeFlow <span class="brand-sub">${brand}</span></span><span class="crumb">${crumb}</span></div>`
      + '<button class="ibtn" type="button" data-gear aria-haspopup="dialog" aria-expanded="false" aria-label="Display settings" title="Display settings"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg></button>';
    if (links.length) { const n = document.createElement('nav'); n.className = 'bar-nav'; n.setAttribute('aria-label', 'Site'); n.innerHTML = links.map(([h, l, cur]) => `<a href="${E(h)}"${cur ? ' aria-current="page"' : ''}>${E(l)}</a>`).join(''); bar.insertBefore(n, $('[data-gear]', bar)); }
    $$('.pop').forEach(p => p.remove());
    const pills = (k, vs) => `<div class="pills">${vs.map(([v, l]) => `<button class="pill" type="button" data-pref="${k}" data-value="${v}">${l}</button>`).join('')}</div>`;
    const pop = document.createElement('div'); pop.className = 'pop'; pop.setAttribute('role', 'dialog'); pop.setAttribute('aria-label', 'Display settings');
    pop.innerHTML = '<span class="kicker">Mode</span>' + pills('mode', [['light', 'Light'], ['dark', 'Dark'], ['system', 'System']])
      + '<span class="kicker">Themes</span>' + pills('theme', [['graphite', 'Graphite'], ['slate', 'Slate'], ['sage', 'Sage']])
      + '<span class="kicker">Accent</span><div class="pills" data-accent-pills aria-label="Theme accent"></div>'
      + '<span class="kicker">Text size</span>' + pills('size', [['default', 'Default'], ['large', 'Large']]);
    bar.after(pop); // right after the bar in the tab sequence, so the gear, the dialog and the page follow each other
    if (!$('.toast')) { const t = document.createElement('div'); t.className = 'toast'; t.setAttribute('role', 'status'); t.setAttribute('aria-live', 'polite'); document.body.appendChild(t); }
    if ($('.rail') || sh.hasAttribute('data-comments')) {
      const c = document.createElement('button'); c.className = 'btn comment-btn'; c.type = 'button'; c.setAttribute('data-comment-toggle', ''); c.setAttribute('aria-pressed', 'false'); c.title = 'Comment (C)'; c.textContent = 'Comment';
      document.body.appendChild(c);
      document.addEventListener('keydown', e => { if ((e.key === 'c' || e.key === 'C') && !e.metaKey && !e.ctrlKey && !e.altKey && !e.target.closest('input, textarea, select, [contenteditable]')) c.click(); });
    }
    applyPrefs();
  })();

  /* Navigation: one two-level component for every page (NAV-SPEC.md). A 56 px rail of area icons is always shown;
     beside it the panel lists the selected area's rows. Pinned, the panel pushes the content; collapsed (toggle,
     Cmd or Ctrl plus backslash, or [) only the rail stays and the content moves left with it, 200 ms. Collapsed,
     150 ms of hover on a rail icon shows that area's panel as a floating card over the content; hovering another
     icon swaps it; it closes 250 ms after the pointer leaves both, on Escape, or when a row is chosen. A rail click
     selects the area (pinned) or goes to its first row (collapsed). Under 768 px rail and panel are one drawer. */
  const shell = $('.shell'), menu = $('.menu'), toggle = $('[data-menu-toggle]');
  const NAV = { closeDrawer() {}, set() {}, peek() {}, state: () => null };
  window.cfNav = NAV;
  if (shell && menu) {
    $$('.peek, .nav-rail').forEach(p => p.remove());
    const mq = matchMedia('(max-width: 767px)'); const narrow = () => mq.matches;
    const still = matchMedia('(prefers-reduced-motion: reduce)');
    const setW = w => { root.style.setProperty('--menu-w', w + 'px'); store.set('menu.w', w); $$('.grip').forEach(g => g.setAttribute('aria-valuenow', w)); };
    const w0 = +store.get('menu.w'); if (w0 >= 220 && w0 <= 360) setW(w0);
    const scrim = document.createElement('div'); scrim.className = 'nav-scrim'; scrim.setAttribute('aria-hidden', 'true'); shell.appendChild(scrim);
    const ICONS = window.CF_ICONS || {}, FALLBACK = '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3.5"/></svg>';
    const icon = n => ICONS[n] || ICONS.default || FALLBACK;
    const scroll = $('.menu-scroll', menu) || menu;
    let areas = $$('section.area', menu);
    if (!areas.length) { const a = document.createElement('section'); a.className = 'area'; a.dataset.area = 'pages'; a.dataset.title = menu.getAttribute('aria-label') || 'Pages'; a.dataset.icon = 'default'; while (scroll.firstChild) a.appendChild(scroll.firstChild); scroll.appendChild(a); areas = [a]; }
    $$('a.row, button.row', menu).forEach(r => { if (r.dataset.icon || !$(':scope > svg', r)) { $$(':scope > svg', r).forEach(x => x.remove()); r.insertAdjacentHTML('afterbegin', icon(r.dataset.icon)); } $(':scope > svg', r).setAttribute('aria-hidden', 'true'); });
    const title = document.createElement('div'); title.className = 'panel-title'; menu.insertBefore(title, menu.firstChild);
    const rail = document.createElement('nav'); rail.className = 'nav-rail'; rail.setAttribute('aria-label', 'Sections');
    const railMain = document.createElement('div'), railUtil = document.createElement('div'); railMain.className = 'nav-rail-main'; railUtil.className = 'nav-rail-util';
    areas.forEach(a => { const b = document.createElement('button'); b.type = 'button'; b.className = 'nav-area'; b.dataset.area = a.dataset.area; b.dataset.tip = a.dataset.title || a.dataset.area; b.setAttribute('aria-label', b.dataset.tip); b.innerHTML = icon(a.dataset.icon); $('svg', b).setAttribute('aria-hidden', 'true'); ('util' in a.dataset ? railUtil : railMain).appendChild(b); });
    rail.append(railMain, railUtil); shell.insertBefore(rail, menu);
    const areaOf = id => areas.find(a => a.dataset.area === id);
    let sel = (areas.find(a => $('.row[aria-current="page"]', a)) || areas.find(a => $('.row[aria-current]', a)) || areas[0]).dataset.area;
    const showArea = id => { areas.forEach(a => { a.hidden = a.dataset.area !== id; }); title.textContent = (areaOf(id) || {}).dataset?.title || ''; menu.dataset.area = id; };
    const select = id => { sel = id; $$('.nav-area', rail).forEach(b => b.dataset.area === id ? b.setAttribute('aria-current', 'true') : b.removeAttribute('aria-current')); showArea(id); };
    select(sel);
    const tip = document.createElement('div'); tip.className = 'nav-tip'; tip.setAttribute('aria-hidden', 'true'); document.body.appendChild(tip);
    const tipOn = b => { const r = b.getBoundingClientRect(); tip.textContent = b.dataset.tip; tip.style.left = (r.right + 10) + 'px'; tip.style.top = (r.top + r.height / 2) + 'px'; tip.classList.add('is-on'); };
    const tipOff = () => tip.classList.remove('is-on');
    let state = store.get('nav'); state = state === 'rail' || state === 'hidden' ? 'rail' : 'full';
    const sync = () => {
      const n = narrow(), drawer = shell.dataset.drawer === 'open', pk = shell.dataset.peek === 'open';
      const hidden = n ? !drawer : state === 'rail' && !pk;
      menu.toggleAttribute('inert', hidden); menu.setAttribute('aria-hidden', String(hidden));
      const railOff = n && !drawer; rail.toggleAttribute('inert', railOff); rail.setAttribute('aria-hidden', String(railOff));
      if (toggle) { const on = n ? drawer : state === 'full'; toggle.setAttribute('aria-pressed', String(on)); toggle.setAttribute('aria-expanded', String(on)); const l = (on ? 'Hide' : 'Show') + ' navigation'; toggle.title = l + ' (Ctrl or Cmd + \\)'; toggle.setAttribute('aria-label', l); }
    };
    let tIn, tOut, tFade;
    const openPeek = id => { clearTimeout(tIn); clearTimeout(tOut); clearTimeout(tFade); if (state !== 'rail' || narrow()) return; showArea(id); if (!menu.classList.contains('peek-card')) { menu.style.transition = 'none'; menu.classList.add('peek-card'); void menu.offsetWidth; menu.style.transition = ''; void menu.offsetWidth; } shell.dataset.peek = 'open'; tipOff(); sync(); };
    const closePeek = now => { clearTimeout(tIn); clearTimeout(tOut); clearTimeout(tFade); const was = shell.dataset.peek === 'open'; delete shell.dataset.peek; sync(); const done = () => { menu.classList.remove('peek-card'); showArea(sel); }; if (now || still.matches || !was) done(); else tFade = setTimeout(done, 140); };
    const PEEK = window.CF_NAV_PEEK !== false;
    // one hover behaviour: where hovering opens the panel there is no tooltip, and the panel opens after one short delay
    // that also swallows a passing mouse; a tooltip appears only where no panel opens (the pinned menu)
    const willPeek = () => PEEK && state === 'rail' && !narrow();
    const PEEK_DELAY = 200;
    rail.addEventListener('pointerover', e => { const b = e.target.closest('.nav-area'); if (!b) { tipOff(); clearTimeout(tIn); return; } if (!willPeek()) { tipOn(b); return; } tipOff(); clearTimeout(tOut); clearTimeout(tIn); if (shell.dataset.peek === 'open' && b.dataset.area === menu.dataset.area) return; tIn = setTimeout(() => openPeek(b.dataset.area), PEEK_DELAY); });
    const leave = e => { clearTimeout(tIn); if (e.relatedTarget && (rail.contains(e.relatedTarget) || menu.contains(e.relatedTarget))) return; if (shell.dataset.peek === 'open') { clearTimeout(tOut); tOut = setTimeout(() => closePeek(), 250); } };
    rail.addEventListener('pointerleave', e => { tipOff(); leave(e); }); menu.addEventListener('pointerleave', leave);
    menu.addEventListener('pointerenter', () => clearTimeout(tOut));
    // keyboard focus: the same behaviour without the delay; the panel closes when focus leaves the rail and the menu
    rail.addEventListener('focusin', e => { const b = e.target.closest('.nav-area'); if (!b) return; if (willPeek()) { tipOff(); openPeek(b.dataset.area); } else tipOn(b); });
    rail.addEventListener('focusout', e => { tipOff(); if (e.relatedTarget && (rail.contains(e.relatedTarget) || menu.contains(e.relatedTarget))) return; if (shell.dataset.peek === 'open' && !(e.relatedTarget && menu.contains(e.relatedTarget))) closePeek(); });
    rail.addEventListener('click', e => {
      const b = e.target.closest('.nav-area'); if (!b) return; const id = b.dataset.area;
      if (narrow() || state === 'full') { select(id); return; }
      closePeek(true); select(id); const r = $$('a.row, button.row', areaOf(id)).find(x => x.getAttribute('aria-current') !== 'page') || $('a.row, button.row', areaOf(id)); if (r) { menu.removeAttribute('inert'); r.click(); sync(); }
    });
    shell.classList.add('no-anim'); shell.dataset.menu = state; sync(); requestAnimationFrame(() => requestAnimationFrame(() => shell.classList.remove('no-anim')));
    const setMenu = s => { state = s === 'rail' || s === 'hidden' ? 'rail' : 'full'; store.set('nav', state); closePeek(true); shell.dataset.menu = state; sync(); if (state === 'rail' && toggle && (menu.contains(document.activeElement) || document.activeElement === document.body || !document.activeElement)) toggle.focus(); };
    const main = $('.main', shell), cbtnEl = () => $('[data-comment-toggle]');
    const setDrawer = open => { const was = shell.dataset.drawer === 'open'; if (open) shell.dataset.drawer = 'open'; else delete shell.dataset.drawer; sync();
      // the drawer behaves as a modal: the page behind it is inert while it is open, and focus returns to the toggle on close
      if (main) main.toggleAttribute('inert', !!open); const cb = cbtnEl(); if (cb) cb.toggleAttribute('inert', !!open);
      if (open) { const r = $('.row', areaOf(sel)); if (r) setTimeout(() => r.focus({ preventScroll: true }), 30); } else if (was && toggle && (!document.activeElement || document.activeElement === document.body || menu.contains(document.activeElement) || rail.contains(document.activeElement))) toggle.focus(); };
    const doToggle = () => { if (narrow()) setDrawer(shell.dataset.drawer !== 'open'); else setMenu(state === 'rail' ? 'full' : 'rail'); };
    if (toggle) toggle.addEventListener('click', doToggle);
    document.addEventListener('keydown', e => {
      if ((e.metaKey || e.ctrlKey) && (e.key === '\\' || e.code === 'Backslash')) { e.preventDefault(); doToggle(); return; }
      if (e.key === '[' && !e.metaKey && !e.ctrlKey && !e.altKey && !/input|textarea|select/i.test(e.target.tagName) && !e.target.isContentEditable) doToggle();
      if (e.key === 'Escape' && shell.dataset.peek === 'open') closePeek();
      if (e.key === 'Escape' && shell.dataset.drawer === 'open') { setDrawer(false); if (toggle) toggle.focus(); }
      if (e.key === 'Tab' && shell.dataset.drawer === 'open') { // keep focus inside the drawer (rail and menu) while it is open
        const f = [...rail.querySelectorAll('button:not([inert])'), ...menu.querySelectorAll('a, button, [tabindex="0"]')].filter(x => x.offsetParent !== null || x.getClientRects().length);
        if (!f.length) return; const first = f[0], last = f[f.length - 1];
        if (e.shiftKey && document.activeElement === first) { last.focus(); e.preventDefault(); } else if (!e.shiftKey && document.activeElement === last) { first.focus(); e.preventDefault(); } else if (!rail.contains(document.activeElement) && !menu.contains(document.activeElement)) { first.focus(); e.preventDefault(); } }
    });
    menu.addEventListener('click', e => { const r = e.target.closest('a.row, button.row, [data-view-link]'); if (!r) return; if (shell.dataset.peek === 'open') { select(menu.dataset.area); closePeek(true); } if (narrow()) setDrawer(false); });
    scrim.addEventListener('click', () => setDrawer(false));
    let sx = null, sy = 0; menu.addEventListener('touchstart', e => { sx = e.touches[0].clientX; sy = e.touches[0].clientY; }, { passive: true });
    menu.addEventListener('touchend', e => { if (sx == null) return; const t = e.changedTouches[0]; if (narrow() && sx - t.clientX > 50 && Math.abs(t.clientY - sy) < 60) setDrawer(false); sx = null; }, { passive: true });
    mq.addEventListener('change', () => { delete shell.dataset.drawer; closePeek(true); sync(); });
    const grip = $('.grip'); if (grip) { let gx, gw; const clamp = w => Math.min(360, Math.max(220, w)); const move = e => setW(clamp(gw + e.clientX - gx)); const up = () => { shell.classList.remove('is-resizing'); removeEventListener('pointermove', move); removeEventListener('pointerup', up); }; grip.addEventListener('pointerdown', e => { gx = e.clientX; gw = menu.getBoundingClientRect().width; shell.classList.add('is-resizing'); addEventListener('pointermove', move); addEventListener('pointerup', up); e.preventDefault(); }); grip.addEventListener('dblclick', () => { root.style.removeProperty('--menu-w'); store.set('menu.w', null); grip.setAttribute('aria-valuenow', parseInt(getComputedStyle(root).getPropertyValue('--menu-w'))); }); grip.addEventListener('keydown', e => { if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') { const w = parseInt(getComputedStyle(root).getPropertyValue('--menu-w')); setW(clamp(w + (e.key === 'ArrowRight' ? 16 : -16))); e.preventDefault(); } }); }
    Object.assign(NAV, { closeDrawer: () => setDrawer(false), set: setMenu, peek: (on, id) => on ? openPeek(id || sel) : closePeek(), state: () => ({ menu: state, drawer: shell.dataset.drawer === 'open', peek: shell.dataset.peek === 'open', area: sel }) });
  }

  /* Current row: the menu row whose section is on screen carries aria-current="true", updated as the reader scrolls
     or follows a link. Only rows that point into this page are tracked; view rows and page rows keep their own state. */
  (() => {
    const here = u => u.replace(/[?#].*$/, '').replace(/index\.html$/, '');
    const rows = $$('.menu a.row[href*="#"]').filter(a => !a.dataset.viewLink && here(a.href) === here(location.href) && a.hash.length > 1);
    const pairs = rows.map(a => { let t = null; try { t = document.getElementById(decodeURIComponent(a.hash.slice(1))); } catch {} return [a, t]; }).filter(p => p[1]);
    if (!pairs.length) return;
    const line = () => (parseFloat(getComputedStyle(root).getPropertyValue('--bar-h')) || 52) + 32;
    let raf = 0, last = null;
    const spy = () => {
      raf = 0; const y = line(); let cur = null, best = -Infinity;
      for (const p of pairs) { const top = p[1].getBoundingClientRect().top; if (top <= y && top > best) { best = top; cur = p; } }
      if (innerHeight + scrollY >= document.documentElement.scrollHeight - 2) cur = pairs.reduce((a, b) => (b[1].getBoundingClientRect().top > a[1].getBoundingClientRect().top ? b : a));
      if (!cur) cur = pairs.reduce((a, b) => (b[1].getBoundingClientRect().top < a[1].getBoundingClientRect().top ? b : a));
      if (cur === last) return; last = cur;
      for (const [a] of pairs) a === cur[0] ? a.setAttribute('aria-current', 'true') : a.removeAttribute('aria-current');
    };
    addEventListener('scroll', () => { raf ||= requestAnimationFrame(spy); }, { passive: true });
    addEventListener('hashchange', () => requestAnimationFrame(spy)); addEventListener('resize', () => { raf ||= requestAnimationFrame(spy); });
    spy();
  })();

  /* Views (menu rows and tabs) with a leave then enter transition; hash routing only to real views */
  const views = $$('.view'); let current = views.find(v => v.classList.contains('is-active')) || views[0] || null;
  const isView = el => !!el && el.classList.contains('view');
  function show(id, push = true, focus = push) {
    const next = document.getElementById(id); if (!isView(next) || next === current) return;
    const dur = parseFloat(getComputedStyle(root).getPropertyValue('--dur-1')) || 0;
    const prev = current; current = next;
    if (prev) { prev.classList.remove('is-active'); prev.classList.add('is-leaving'); }
    setTimeout(() => { if (prev) prev.classList.remove('is-leaving'); next.classList.add('is-active'); window.scrollTo({ top: 0, behavior: 'instant' });
      if (focus) { const h = next.querySelector('h1, h2'); if (h) { h.setAttribute('tabindex', '-1'); h.focus({ preventScroll: true }); } } }, prev ? dur : 0);
    $$('[data-view-link]').forEach(a => { const on = a.dataset.viewLink === id; a.getAttribute('role') === 'tab' ? a.setAttribute('aria-selected', String(on)) : (on ? a.setAttribute('aria-current', 'page') : a.removeAttribute('aria-current')); });
    const crumb = $('.bar .crumb span[data-crumb]'); if (crumb) crumb.textContent = next.dataset.title || '';
    if (push) history.replaceState(null, '', '#' + id);
    window.cfNav.closeDrawer();
  }
  document.addEventListener('click', e => { const a = e.target.closest('[data-view-link]'); if (!a) return; e.preventDefault(); show(a.dataset.viewLink); });
  const hashTarget = location.hash ? document.getElementById(location.hash.slice(1)) : null;
  if (isView(hashTarget)) show(hashTarget.id, false, false); else if (current) $$('[data-view-link]').forEach(a => { if (a.dataset.viewLink === current.id) a.getAttribute('role') === 'tab' ? a.setAttribute('aria-selected', 'true') : a.setAttribute('aria-current', 'page'); });
  window.cfShow = show;

  /* Display popover, Comment mode, forms, copy, toast */
  const pop = $('.pop'), gear = $('[data-gear]');
  const closePop = (refocus = true) => { if (!pop || !gear || !pop.classList.contains('is-open')) return; pop.classList.remove('is-open'); gear.setAttribute('aria-expanded', 'false'); if (refocus) gear.focus(); };
  document.addEventListener('keydown', e => { if (e.key === 'Escape' && pop && pop.classList.contains('is-open')) closePop(true);
    if (e.key === 'Tab' && pop && pop.classList.contains('is-open')) { const f = [...pop.querySelectorAll('button')]; if (!f.length) return; const first = f[0], last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) { last.focus(); e.preventDefault(); } else if (!e.shiftKey && document.activeElement === last) { first.focus(); e.preventDefault(); } } });
  if (gear && pop) { gear.addEventListener('click', () => { const open = !pop.classList.contains('is-open'); if (!open) return closePop(true); pop.classList.add('is-open'); gear.setAttribute('aria-expanded', 'true'); const f = pop.querySelector('[aria-pressed="true"]') || pop.querySelector('button'); if (f) setTimeout(() => f.focus(), 20); });
    document.addEventListener('pointerdown', e => { if (pop.classList.contains('is-open') && !pop.contains(e.target) && !gear.contains(e.target)) closePop(false); }); }
  const toast = $('.toast'); let toastT; window.cfToast = m => { if (!toast) return; toast.textContent = m; toast.classList.add('is-open'); clearTimeout(toastT); toastT = setTimeout(() => toast.classList.remove('is-open'), 1800); };
  const cbtn = $('[data-comment-toggle]'), rail = $('.rail'); let notes = 0;
  if (cbtn && rail) { cbtn.addEventListener('click', () => { const on = root.dataset.comment !== 'armed'; root.dataset.comment = on ? 'armed' : ''; cbtn.setAttribute('aria-pressed', String(on)); rail.classList.toggle('is-open', on || notes > 0); if (!on && !notes) rail.classList.remove('is-open'); });
    document.addEventListener('click', e => { if (root.dataset.comment !== 'armed') return; const b = e.target.closest('[data-block]'); if (!b || e.target.closest('button, a, input, label')) return; notes++; const m = document.createElement('span'); m.className = 'marker'; m.textContent = notes; const r = b.getBoundingClientRect(); b.style.position ||= 'relative'; m.style.left = (e.clientX - r.left) + 'px'; m.style.top = (e.clientY - r.top) + 'px'; b.appendChild(m); const li = document.createElement('li'); li.textContent = `on "${b.dataset.block}"`; $('ol', rail).appendChild(li); rail.classList.add('is-open'); }); }
  $$('[data-copy]').forEach(b => b.addEventListener('click', async () => { const t = $(b.dataset.copy).textContent; try { await navigator.clipboard.writeText(t); cfToast('Copied'); } catch { cfToast('Select the text and copy it'); } }));
  $$('form[data-answers]').forEach(f => f.addEventListener('submit', e => { e.preventDefault(); const d = Object.fromEntries(new FormData(f).entries()); store.set('answers.' + f.dataset.answers, JSON.stringify(d)); cfToast(`${Object.keys(d).length} answer${Object.keys(d).length === 1 ? '' : 's'} stored for the session`); }));
})();
