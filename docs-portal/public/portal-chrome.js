// Product adaptation of the approved design kit. Search uses Pagefind; routes use real URLs.
/* ---------- Runtime ---------- */
(() => {
  'use strict';
  const d = document;
  const root = d.documentElement;
  const $ = (s, r = d) => r.querySelector(s);
  const $$ = (s, r = d) => Array.from(r.querySelectorAll(s));
  const mem = {};
  const store = {
    get(k) {
      try { const v = localStorage.getItem(k); if (v !== null) return v; } catch (e) { /* storage unavailable */ }
      return Object.prototype.hasOwnProperty.call(mem, k) ? mem[k] : null;
    },
    set(k, v) {
      mem[k] = String(v);
      try { localStorage.setItem(k, String(v)); } catch (e) { /* keep the choice for this page view only */ }
    }
  };
  const mq = q => window.matchMedia(q);
  const mqDark = mq('(prefers-color-scheme: dark)');
  const mqReduce = mq('(prefers-reduced-motion: reduce)');
  const mqNavSheet = mq('(max-width: 879.98px)');
  const mqTocSheet = mq('(max-width: 1119.98px)');
  const mqPhone = mq('(max-width: 599.98px)');
  const editable = el => !!el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const mk = (tag, cls) => { const el = d.createElement(tag); if (cls) el.className = cls; return el; };
  const reflow = el => void el.offsetWidth;
  const dur = name => (mqReduce.matches ? 0 : parseFloat(getComputedStyle(root).getPropertyValue(name)) || 0);
  const smooth = () => (mqReduce.matches ? 'auto' : 'smooth');
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  const CF = window.CF = {};
  /* ---------- Display preferences ---------- */
  const PREFS = {
    typeface: { key: 'cf-portal-typeface', attr: 'data-cfp-typeface', label: 'Font', desc: 'Body and headings', def: 'inter', options: [['archivo', 'Archivo'], ['inter', 'Inter'], ['plex', 'Plex Sans']] },
    scale: { key: 'cf-portal-scale', attr: 'data-cfp-scale', label: 'Size', desc: 'Text and controls', def: 'default', options: [['compact', 'Compact'], ['default', 'Default'], ['large', 'Large']] },
    skin: { key: 'cf-portal-skin', attr: 'data-cfp-skin', label: 'Palette', desc: 'Surfaces and accent', def: 'graphite', options: [['graphite', 'Graphite'], ['slate', 'Slate'], ['sage', 'Sage']] },
    theme: { key: 'starlight-theme', attr: null, label: 'Appearance', desc: 'Light, dark or the system', def: 'system', options: [['light', 'Light'], ['dark', 'Dark'], ['system', 'System']] }
  };
  function getPref(name) {
    const p = PREFS[name], saved = store.get(p.key);
    const aliases = name === 'skin' ? {instrument: 'graphite', technical: 'graphite', editorial: 'slate', ink: 'sage'} : name === 'typeface' ? {instrument: 'archivo', editorial: 'inter'} : {};
    const v = Object.hasOwn(aliases, saved) ? aliases[saved] : saved;
    return p.options.some(o => o[0] === v) ? v : (p.attr && root.getAttribute(p.attr)) || p.def;
  }
  function applyTheme() {
    const pref = getPref('theme');
    root.setAttribute('data-theme', pref === 'system' ? (mqDark.matches ? 'dark' : 'light') : pref);
  }
  function setPref(name, value) {
    store.set(PREFS[name].key, value);
    if (name === 'theme') applyTheme(); else root.setAttribute(PREFS[name].attr, value);
    syncDisplay();
    d.dispatchEvent(new CustomEvent('cf:prefs', { detail: { name, value } }));
  }
  mqDark.addEventListener('change', () => {
    if (getPref('theme') === 'system') { applyTheme(); syncDisplay(); d.dispatchEvent(new CustomEvent('cf:prefs', { detail: { name: 'theme' } })); }
  });
  CF.setPref = setPref;
  CF.getPref = getPref;

  let displayEl = null, displayOpener = null, previewEl = null;
  function buildDisplay() {
    displayEl = mk('div', 'cf-popover cf-display');
    displayEl.id = 'cf-display';
    displayEl.dataset.testid = 'portal-display-panel';
    displayEl.hidden = true;
    displayEl.setAttribute('role', 'dialog');
    displayEl.setAttribute('aria-label', 'Display');
    displayEl.setAttribute('data-cf-chrome', '');
    const groups = Object.entries(PREFS).map(([name, p]) => `
      <div class="cf-dgroup">
        <div class="cf-dgroup-head"><span class="cf-dgroup-label" id="cf-dl-${name}">${p.label}</span><span class="cf-dgroup-desc">${p.desc}</span></div>
        <div class="cf-pills" role="radiogroup" aria-labelledby="cf-dl-${name}" data-pref="${name}">
          ${p.options.map(([v, l]) => `<button type="button" class="cf-pill" role="radio" aria-checked="false" tabindex="-1" data-value="${v}" data-testid="${name === 'theme' ? 'appearance' : name}-${v}">${name === 'skin' ? `<span class="cf-pill-swatch" data-cfp-skin="${v}" data-theme="light" aria-hidden="true"><i class="cf-dot-canvas"></i><i class="cf-dot-accent"></i></span>` : ''}${l}</button>`).join('')}
        </div>
      </div>`).join('');
    displayEl.innerHTML = `
      <div class="cf-display-preview" data-cfp-skin="graphite" data-theme="light" aria-hidden="true">
        <span class="cf-sw cf-sw--canvas"></span><span class="cf-sw cf-sw--surface"></span><span class="cf-sw cf-sw--accent"></span>
        <span class="cf-dp-body">Body Aa</span><span class="cf-dp-mono">Mono 012</span>
      </div>${groups}
      <p class="cf-display-foot">Remembered in this browser for every page of this guide. <a href="${CF.view.el.dataset.base}fonts-LICENSE.txt">Font licenses</a></p>`;
    d.body.appendChild(displayEl);
    previewEl = $('.cf-display-preview', displayEl);
    displayEl.addEventListener('click', e => {
      const pill = e.target.closest('.cf-pill');
      if (pill) choose(pill);
    });
    displayEl.addEventListener('keydown', e => {
      const pill = e.target.closest('.cf-pill');
      if (!pill) return;
      const pills = $$('.cf-pill', pill.parentElement), i = pills.indexOf(pill);
      let j = null;
      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') j = (i + 1) % pills.length;
      else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') j = (i - 1 + pills.length) % pills.length;
      else if (e.key === 'Home') j = 0;
      else if (e.key === 'End') j = pills.length - 1;
      if (j === null) return;
      e.preventDefault();
      pills[j].focus();
      choose(pills[j]);
    });
    displayEl.addEventListener('pointerover', e => {
      const pill = e.target.closest('[data-pref="skin"] .cf-pill');
      if (pill) previewEl.setAttribute('data-cfp-skin', pill.dataset.value);
    });
    displayEl.addEventListener('pointerout', e => {
      if (e.target.closest('[data-pref="skin"] .cf-pill')) previewEl.setAttribute('data-cfp-skin', root.getAttribute('data-cfp-skin'));
    });
  }
  function choose(pill) {
    const name = pill.parentElement.dataset.pref;
    setPref(name, pill.dataset.value);
  }
  function syncDisplay() {
    if (!displayEl) return;
    $$('.cf-pills', displayEl).forEach(g => {
      const v = getPref(g.dataset.pref);
      $$('.cf-pill', g).forEach(p => {
        const on = p.dataset.value === v;
        p.setAttribute('aria-checked', String(on));
        p.tabIndex = on ? 0 : -1;
      });
    });
    const theme = root.getAttribute('data-theme');
    previewEl.setAttribute('data-cfp-skin', root.getAttribute('data-cfp-skin'));
    previewEl.setAttribute('data-theme', theme);
    $$('.cf-pill-swatch', displayEl).forEach(s => s.setAttribute('data-theme', theme));
  }
  const displayOpen = () => !!displayEl && !displayEl.hidden && displayEl.classList.contains('is-open');
  function placeDisplay() {
    if (!displayOpener) return;
    if (mqPhone.matches) {
      displayEl.classList.add('is-sheet');
      displayEl.style.left = '';
      displayEl.style.top = '';
      return;
    }
    displayEl.classList.remove('is-sheet');
    const b = displayOpener.getBoundingClientRect(), w = displayEl.offsetWidth;
    displayEl.style.left = clamp(b.right - w, 12, innerWidth - w - 12) + 'px';
    displayEl.style.top = (b.bottom + 8) + 'px';
  }
  function openDisplay(btn) {
    if (!displayEl) buildDisplay();
    if (displayOpen() && displayOpener === btn) { closeDisplay(true); return; }
    clearTimeout(displayEl._t);
    displayOpener = btn;
    syncDisplay();
    displayEl.hidden = false;
    placeDisplay();
    $$('[data-cf-open="display"]').forEach(b => b.setAttribute('aria-expanded', String(b === btn)));
    reflow(displayEl);
    displayEl.classList.add('is-open');
    const first = $('.cf-pill[aria-checked="true"]', displayEl);
    if (first) first.focus({ preventScroll: true });
  }
  function closeDisplay(returnFocus) {
    if (!displayOpen()) return false;
    displayEl.classList.remove('is-open');
    $$('[data-cf-open="display"]').forEach(b => b.setAttribute('aria-expanded', 'false'));
    clearTimeout(displayEl._t);
    displayEl._t = setTimeout(() => { if (!displayEl.classList.contains('is-open')) displayEl.hidden = true; }, dur('--cf-dur-pop-out'));
    if (returnFocus && displayOpener) displayOpener.focus();
    return true;
  }

  /* ---------- Search dialog ---------- */
  const S = { el: null, box: null, input: null, list: null, status: null, items: [], active: -1, opener: null, q: '' };
  function buildSearch() {
    S.el = mk('div', 'cf-dialog-backdrop');
    S.el.hidden = true;
    S.el.setAttribute('data-cf-chrome', '');
    S.box = mk('div', 'cf-search');
    S.box.hidden = true;
    S.box.setAttribute('role', 'dialog');
    S.box.setAttribute('aria-modal', 'true');
    S.box.setAttribute('aria-label', 'Search the guide');
    S.box.setAttribute('data-cf-chrome', '');
    S.box.innerHTML = `
      <div class="cf-search-bar"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/></svg>
        <input class="cf-search-input" type="search" role="combobox" aria-expanded="true" aria-controls="cf-search-list" aria-autocomplete="list" aria-label="Search the guide" placeholder="Search the guide" autocomplete="off" spellcheck="false">
        <kbd class="cf-kbd" aria-hidden="true">esc</kbd></div>
      <p class="cf-search-status" role="status" aria-live="polite"></p>
      <div class="cf-search-list" id="cf-search-list" role="listbox" aria-label="Results"></div>`;
    d.body.append(S.el, S.box);
    S.input = $('.cf-search-input', S.box);
    S.list = $('.cf-search-list', S.box);
    S.status = $('.cf-search-status', S.box);
    S.el.addEventListener('click', () => closeSearch(true));
    S.input.addEventListener('input', () => render(S.input.value));
    S.input.addEventListener('keydown', e => {
      if (e.key === 'ArrowDown') { e.preventDefault(); setActive(S.items.length ? (S.active + 1) % S.items.length : -1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); setActive(S.items.length ? (S.active - 1 + S.items.length) % S.items.length : -1); }
      else if (e.key === 'Home' && e.ctrlKey) { e.preventDefault(); setActive(0); }
      else if (e.key === 'Enter') { e.preventDefault(); if (S.items[S.active]) follow(S.items[S.active]); }
      else if (e.key === 'Tab') { e.preventDefault(); }
    });
    S.list.addEventListener('pointermove', e => {
      const o = e.target.closest('.cf-hit');
      if (o && +o.dataset.i !== S.active) setActive(+o.dataset.i, false);
    });
    S.list.addEventListener('pointerdown', e => e.preventDefault());
    S.list.addEventListener('click', e => {
      const o = e.target.closest('.cf-hit');
      if (o) follow(S.items[+o.dataset.i]);
    });
  }
  const searchOpen = () => !!S.box && !S.box.hidden && S.box.classList.contains('is-open');
  function openSearch(opener) {
    if (!CF.view || CF.view.name !== 'portal') return;
    if (!S.box) buildSearch();
    closeDisplay(false);
    clearTimeout(S.t);
    S.opener = opener || d.activeElement;
    S.el.hidden = false;
    S.box.hidden = false;
    reflow(S.box);
    S.el.classList.add('is-open');
    S.box.classList.add('is-open');
    S.input.value = '';
    render('');
    S.input.focus();
  }
  function closeSearch(returnFocus) {
    if (!searchOpen()) return false;
    searchSequence++;
    S.el.classList.remove('is-open');
    S.box.classList.remove('is-open');
    clearTimeout(S.t);
    S.t = setTimeout(() => { S.el.hidden = true; S.box.hidden = true; }, dur('--cf-dur-pop-out'));
    if (returnFocus && S.opener && S.opener.isConnected) S.opener.focus();
    return true;
  }
  function highlight(s, q) {
    if (!q) return esc(s);
    const re = new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
    let out = '', last = 0;
    s.replace(re, (m, off) => { out += esc(s.slice(last, off)) + '<mark>' + esc(m) + '</mark>'; last = off + m.length; return m; });
    return out + esc(s.slice(last));
  }
  function excerpt(text, q) {
    const t = String(text).replace(/\s+/g, ' ').trim();
    const i = q ? t.toLowerCase().indexOf(q.toLowerCase()) : 0;
    let start = 0;
    if (i > 48) start = t.lastIndexOf(' ', i - 36) + 1;
    let s = t.slice(start, start + 150);
    if (start > 0) s = '… ' + s;
    if (start + 150 < t.length) s += ' …';
    return highlight(s, q);
  }
  let searchSequence = 0;
  async function render(q) {
    const sequence = ++searchSequence;
    const qs = q.trim();
    S.q = qs;
    let groups;
    if (!qs) {
      groups = [{ title: 'Recent', crumb: '', items: CF.view.recent }];
      S.status.textContent = '';
      S.status.hidden = true;
    } else {
      S.status.hidden = false;
      S.status.textContent = 'Searching…';
      let hits;
      try { hits = await CF.view.search(qs); }
      catch { if (sequence === searchSequence) { S.status.textContent = 'Search is unavailable. Browse the navigation or try again.'; S.list.replaceChildren(); S.items = []; } return; }
      if (sequence !== searchSequence) return;
      const byPage = new Map();
      hits.forEach(h => {
        if (!byPage.has(h.page)) byPage.set(h.page, { title: h.page, crumb: h.crumb, items: [] });
        byPage.get(h.page).items.push(h);
      });
      groups = Array.from(byPage.values());
      S.status.hidden = false;
      S.status.textContent = hits.length ? `${hits.length} result${hits.length === 1 ? '' : 's'} for ${qs}` : `No results for ${qs}. Try a command name or an ID.`;
    }
    S.items = [];
    let html = '';
    groups.forEach((g, gi) => {
      if (!g.items.length) return;
      html += `<div class="cf-search-group" role="group" aria-labelledby="cf-sg-${gi}"><div class="cf-search-group-head" id="cf-sg-${gi}"><span class="cf-search-group-title">${esc(g.title)}</span>${g.crumb ? `<span class="cf-search-group-crumb">${esc(g.crumb)}</span>` : ''}</div>`;
      g.items.forEach(h => {
        const i = S.items.push(h) - 1;
        html += `<div class="cf-hit" role="option" id="cf-opt-${i}" data-i="${i}" data-href="${esc(h.route)}" aria-selected="false"><span class="cf-hit-main"><span class="cf-hit-title">${highlight(h.title, qs)}</span>${h.text ? `<span class="cf-hit-excerpt">${excerpt(h.text, qs)}</span>` : ''}</span>${h.altitude ? `<span class="cf-chip">${esc(h.altitude)}</span>` : ''}</div>`;
      });
      html += '</div>';
    });
    if (!qs) html += '<p class="cf-search-hint">Type an ID such as <code>CAP-016</code> to jump.</p>';
    S.list.innerHTML = html;
    S.list.scrollTop = 0;
    setActive(S.items.length ? 0 : -1);
  }
  function setActive(i, scroll = true) {
    S.active = i;
    $$('.cf-hit', S.list).forEach(o => {
      const on = +o.dataset.i === i;
      o.classList.toggle('is-active', on);
      o.setAttribute('aria-selected', String(on));
      if (on && scroll) o.scrollIntoView({ block: 'nearest' });
    });
    if (i >= 0) S.input.setAttribute('aria-activedescendant', 'cf-opt-' + i);
    else S.input.removeAttribute('aria-activedescendant');
  }
  function follow(hit) {
    closeSearch(false);
    go(hit.route, hit.anchor);
  }

  function go(route, anchor) {
    const url = new URL(route, location.href);
    if (url.origin !== location.origin) return;
    if (anchor) url.hash = anchor;
    location.assign(url.href);
  }
  let pagefind;
  async function searchPages(query) {
    pagefind ||= import(new URL(CF.view.el.dataset.base + 'pagefind/pagefind.js', location.origin).href);
    const engine = await pagefind;
    const found = await engine.search(query);
    const pages = await Promise.all(found.results.slice(0, 20).map(result => result.data()));
    return pages.flatMap(page => (page.sub_results?.length ? page.sub_results : [page]).map(hit => {
      const url = new URL(hit.url || page.url, location.href);
      const text = new DOMParser().parseFromString(hit.excerpt || page.excerpt || '', 'text/html').body.textContent;
      const section = (page.anchors || []).filter(anchor => /^portal-panel-(concept|architecture|technical)$/.test(anchor.id)
        && anchor.location <= (hit.anchor?.location ?? -1)).at(-1);
      const altitude = section?.id.slice('portal-panel-'.length);
      return { page: page.meta.title, title: hit.title || page.meta.title, crumb: url.pathname,
        route: url.pathname + url.hash, text, altitude: altitude ? altitude[0].toUpperCase() + altitude.slice(1) : 'Page' };
    }));
  }

  /* ---------- Portal view ---------- */
  function mountPortal(el, data, signal) {
    const view = { name: 'portal', el, path: el.dataset.cfRoute || '', mainEl: $('main', el) };
    view.mainEl.tabIndex = -1;
    view.search = searchPages;
    view.recent = data.recent || [];
    const header = $('.cf-header', el);
    const shell = $('.cf-shell', el);

    /* skip link and header shadow */
    $('[data-cf-skip]').addEventListener('click', e => { e.preventDefault(); view.mainEl.focus(); view.mainEl.scrollIntoView({ block: 'start' }); });
    let raf = 0;
    const onScroll = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => { raf = 0; header.classList.toggle('is-scrolled', scrollY > 8); spy(); });
    };
    addEventListener('scroll', onScroll, { passive: true, signal });

    /* nav groups, collapse state remembered */
    let groupState = {};
    try { groupState = JSON.parse(store.get('cf.nav.groups') || '{}') || {}; } catch (e) { groupState = {}; }
    function applyGroups() {
      $$('.cf-nav-group', el).forEach(g => {
        const closed = !!groupState[g.dataset.group];
        g.classList.toggle('is-collapsed', closed);
        const btn = $('.cf-nav-chev', g), body = $('.cf-nav-group-body', g);
        btn.setAttribute('aria-expanded', String(!closed));
        body.inert = closed;
      });
    }

    /* sheets: clone the nav into the left sheet */
    const sheets = { nav: $('[data-cf-sheet="nav"]', el), toc: $('[data-cf-sheet="toc"]', el) };
    const navClone = $('.cf-panel--nav nav', el).cloneNode(true);
    $$('[id]', navClone).forEach(n => { n.id += '-sheet'; });
    $$('[aria-controls],[aria-labelledby]', navClone).forEach(n => {
      ['aria-controls', 'aria-labelledby'].forEach(a => { if (n.hasAttribute(a)) n.setAttribute(a, n.getAttribute(a) + '-sheet'); });
    });
    $('[data-cf-sheet-body]', sheets.nav).appendChild(navClone);
    applyGroups();
    el.addEventListener('click', e => {
      const chev = e.target.closest('.cf-nav-chev');
      if (!chev) return;
      const g = chev.closest('.cf-nav-group').dataset.group;
      groupState[g] = !groupState[g];
      store.set('cf.nav.groups', JSON.stringify(groupState));
      applyGroups();
    }, { signal });

    const navToggle = $('[data-cf-toggle="nav"]', el), tocToggle = $('[data-cf-toggle="toc"]', el);
    const navHidden = () => mqNavSheet.matches || root.getAttribute('data-cf-nav') === 'collapsed';
    const tocHidden = () => mqTocSheet.matches || root.getAttribute('data-cf-toc') === 'collapsed';
    let openSheet = null, sheetKeyboard = false, lastFocus = null, hideT = 0;
    function syncToggles() {
      const navShown = !navHidden() || openSheet === 'nav';
      const tocShown = !tocHidden() || openSheet === 'toc';
      navToggle.setAttribute('aria-pressed', String(navShown));
      navToggle.title = navShown ? 'Hide navigation' : 'Show navigation';
      tocToggle.setAttribute('aria-pressed', String(tocShown));
      tocToggle.title = tocShown ? 'Hide page outline' : 'Show page outline';
      const grip = $('.cf-grip', el);
      grip.tabIndex = navHidden() ? -1 : 0;
    }
    function showSheet(name, keyboard) {
      clearTimeout(hideT);
      if (openSheet && openSheet !== name) hideSheet(openSheet, false);
      const s = sheets[name];
      s.classList.add('is-open');
      s.setAttribute('aria-modal', keyboard ? 'true' : 'false');
      openSheet = name;
      sheetKeyboard = !!keyboard;
      if (keyboard) {
        lastFocus = d.activeElement;
        const target = $('.cf-row[aria-current="page"], .cf-toc-link.is-active', s) || $('a, button', s);
        if (target) target.focus({ preventScroll: true });
      }
      syncToggles();
    }
    function hideSheet(name, returnFocus) {
      const s = sheets[name];
      if (!s || !s.classList.contains('is-open')) return false;
      s.classList.remove('is-open');
      if (openSheet === name) openSheet = null;
      if (returnFocus && lastFocus && lastFocus.isConnected) lastFocus.focus();
      sheetKeyboard = false;
      syncToggles();
      return true;
    }
    function toggleSheet(name, keyboard) { openSheet === name ? hideSheet(name, keyboard) : showSheet(name, keyboard); }
    function setColumn(name, open) {
      const attr = name === 'nav' ? 'data-cf-nav' : 'data-cf-toc';
      if (open) root.removeAttribute(attr); else root.setAttribute(attr, 'collapsed');
      store.set(name === 'nav' ? 'cf.nav.open' : 'cf.toc.open', open);
      hideSheet(name, false);
      syncToggles();
      setTimeout(() => view.relayout(), dur('--cf-dur-panel') + 20);
    }
    navToggle.addEventListener('click', e => {
      if (mqNavSheet.matches) { toggleSheet('nav', e.detail === 0); return; }
      setColumn('nav', root.getAttribute('data-cf-nav') === 'collapsed');
    }, { signal });
    navToggle.addEventListener('keydown', e => {
      if (e.key === 'Enter' && !mqNavSheet.matches && root.getAttribute('data-cf-nav') === 'collapsed') { e.preventDefault(); toggleSheet('nav', true); }
    }, { signal });
    tocToggle.addEventListener('click', e => {
      if (mqTocSheet.matches) { toggleSheet('toc', e.detail === 0); return; }
      setColumn('toc', root.getAttribute('data-cf-toc') === 'collapsed');
    }, { signal });
    tocToggle.addEventListener('keydown', e => {
      if (e.key === 'Enter' && !mqTocSheet.matches && root.getAttribute('data-cf-toc') === 'collapsed') { e.preventDefault(); toggleSheet('toc', true); }
    }, { signal });
    $$('[data-cf-pin]', el).forEach(b => b.addEventListener('click', () => setColumn(b.dataset.cfPin, true), { signal }));
    $$('[data-cf-close-sheet]', el).forEach(b => b.addEventListener('click', () => hideSheet(openSheet, sheetKeyboard), { signal }));
    $$('[data-cf-peek]', el).forEach(strip => {
      const name = strip.dataset.cfPeek;
      strip.addEventListener('pointerenter', () => { if ((name === 'nav' ? navHidden() : tocHidden())) showSheet(name, false); }, { signal });
      strip.addEventListener('pointerleave', () => { if (!sheetKeyboard) { clearTimeout(hideT); hideT = setTimeout(() => hideSheet(name, false), 200); } }, { signal });
    });
    Object.entries(sheets).forEach(([name, s]) => {
      s.addEventListener('pointerenter', () => clearTimeout(hideT), { signal });
      s.addEventListener('pointerleave', e => {
        if (sheetKeyboard || e.pointerType === 'touch') return;
        clearTimeout(hideT);
        hideT = setTimeout(() => hideSheet(name, false), 200);
      }, { signal });
      s.addEventListener('click', e => { if (e.target.closest('a')) hideSheet(name, false); }, { signal });
      s.addEventListener('keydown', e => {
        if (e.key !== 'Tab' || !sheetKeyboard) return;
        const f = $$('a[href], button:not([disabled])', s).filter(x => x.offsetParent !== null && !x.closest('[inert]'));
        if (!f.length) return;
        const first = f[0], last = f[f.length - 1];
        if (e.shiftKey && d.activeElement === first) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && d.activeElement === last) { e.preventDefault(); first.focus(); }
      }, { signal });
    });
    d.addEventListener('pointerdown', e => {
      if (!openSheet) return;
      const s = sheets[openSheet];
      if (s.contains(e.target) || e.target.closest('[data-cf-toggle], [data-cf-peek]')) return;
      hideSheet(openSheet, false);
    }, { signal, capture: true });
    [mqNavSheet, mqTocSheet].forEach(m => m.addEventListener('change', () => { if (openSheet) hideSheet(openSheet, false); syncToggles(); view.relayout(); }, { signal }));
    syncToggles();

    /* resize grip */
    const grip = $('.cf-grip', el);
    const navW = () => parseInt(getComputedStyle(root).getPropertyValue('--cf-nav-w'), 10) || 264;
    function setW(w, persist) {
      w = Math.round(clamp(w, 220, 360));
      root.style.setProperty('--cf-nav-w', w + 'px');
      grip.setAttribute('aria-valuenow', String(w));
      if (persist) store.set('cf.nav.width', w);
    }
    grip.setAttribute('aria-valuenow', String(navW()));
    grip.addEventListener('pointerdown', e => {
      if (e.button !== 0) return;
      e.preventDefault();
      grip.setPointerCapture(e.pointerId);
      const x0 = e.clientX, w0 = navW();
      shell.classList.add('is-resizing');
      grip.classList.add('is-dragging');
      const move = ev => setW(w0 + ev.clientX - x0, false);
      const up = () => {
        grip.removeEventListener('pointermove', move);
        shell.classList.remove('is-resizing');
        grip.classList.remove('is-dragging');
        setW(navW(), true);
        view.relayout();
      };
      grip.addEventListener('pointermove', move);
      grip.addEventListener('pointerup', up, { once: true });
      grip.addEventListener('pointercancel', up, { once: true });
    }, { signal });
    grip.addEventListener('keydown', e => {
      const step = { ArrowLeft: -8, ArrowRight: 8 }[e.key];
      if (step) { e.preventDefault(); shell.classList.add('is-resizing'); setW(navW() + step, true); requestAnimationFrame(() => { shell.classList.remove('is-resizing'); view.relayout(); }); }
      else if (e.key === 'Home' || e.key === 'End') { e.preventDefault(); setW(e.key === 'Home' ? 220 : 360, true); view.relayout(); }
    }, { signal });
    grip.addEventListener('dblclick', () => { setW(264, true); view.relayout(); }, { signal });

    // Altitudes remain owned by portal-tabs.js, including deep section hashes.
    function selectLayer(name) {
      const tab = $$('.portal-altitude-tabs [role="tab"]', el).find(t => t.dataset.anchor === name);
      if (tab) tab.click();
    }
    d.addEventListener('cf:altitude', () => buildToc(), { signal });
    /* table of contents with scroll spy */
    const tocLists = $$('[data-cf-toc]', el);
    let heads = [], activeId = '';
    function buildToc() {
      const panel = $('.portal-altitude:not([hidden])', el) || view.mainEl;
      heads = $$('h2[id], h3[id]', view.mainEl).filter(h => !h.closest('.portal-altitude') || h.closest('.portal-altitude') === panel);
      const html = heads.map(h => `<li><a class="cf-toc-link${h.tagName === 'H3' ? ' is-l3' : ''}" href="#${h.id}" data-target="${h.id}">${esc(h.textContent.trim())}</a></li>`).join('');
      tocLists.forEach(l => { l.innerHTML = html; });
      activeId = '';
      spy();
    }
    function spy() {
      if (!heads.length) return;
      let act = heads[0];
      heads.forEach(h => { if (h.getBoundingClientRect().top <= 96) act = h; });
      if (innerHeight + scrollY >= root.scrollHeight - 4) {
        const vis = heads.filter(h => h.getBoundingClientRect().top < innerHeight - 40);
        if (vis.length) act = vis[vis.length - 1];
      }
      if (act.id === activeId) return;
      activeId = act.id;
      tocLists.forEach(l => $$('.cf-toc-link', l).forEach(a => {
        const on = a.dataset.target === activeId;
        a.classList.toggle('is-active', on);
        if (on) a.setAttribute('aria-current', 'location'); else a.removeAttribute('aria-current');
      }));
    }
    tocLists.forEach(l => l.addEventListener('click', e => {
      const a = e.target.closest('.cf-toc-link');
      if (!a) return;
      e.preventDefault();
      const h = d.getElementById(a.dataset.target);
      if (!h) return;
      history.replaceState(null, '', '#' + encodeURIComponent(h.id));
      h.scrollIntoView({ behavior: smooth(), block: 'start' });
      h.setAttribute('tabindex', '-1');
      h.focus({ preventScroll: true });
    }, { signal }));

    /* public view api */
    view.selectLayer = selectLayer;
    view.peek = name => { if (name === 'nav' ? navHidden() : tocHidden()) toggleSheet(name, true); else if (openSheet) hideSheet(openSheet, true); };
    view.relayout = () => spy();
    view.onEscape = () => {
      if (openSheet) return hideSheet(openSheet, sheetKeyboard);
      return false;
    };
    d.addEventListener('cf:prefs', view.relayout, { signal });
    addEventListener('resize', () => { view.relayout(); if (displayOpen()) placeDisplay(); }, { signal });

    buildToc();
    return view;
  }

  /* ---------- Mount ---------- */
  let ac = null;
  /* Mount a [data-cf-view] element; mounting again tears down the previous view first */
  CF.mount = el => {
    if (ac) ac.abort();
    if (CF.view) CF.view.el.removeAttribute('data-cf-mounted');
    ac = new AbortController();
    closeDisplay(false);
    if (S.box && !S.box.hidden) { S.el.hidden = true; S.box.hidden = true; S.box.classList.remove('is-open'); S.el.classList.remove('is-open'); }
    const dataEl = d.getElementById('cf-data'), data = dataEl ? JSON.parse(dataEl.textContent) : {};
    CF.view = mountPortal(el, data, ac.signal);
    el.setAttribute('data-cf-mounted', '');
    return CF.view;
  };

  /* ---------- Global wiring (once) ---------- */
  d.addEventListener('click', e => {
    const b = e.target.closest('[data-cf-open]');
    if (!b) return;
    if (b.dataset.cfOpen === 'display') openDisplay(b);
    else if (b.dataset.cfOpen === 'search') openSearch(b);
  });
  d.addEventListener('pointerdown', e => {
    if (displayOpen() && !displayEl.contains(e.target) && !e.target.closest('[data-cf-open="display"]')) closeDisplay(false);
  }, true);
  addEventListener('resize', () => { if (displayOpen()) placeDisplay(); });
  d.addEventListener('keydown', e => {
    const view = CF.view;
    if (e.key === 'Escape') {
      if (closeSearch(true) || closeDisplay(true)) { e.preventDefault(); return; }
      if (view && view.onEscape && view.onEscape()) e.preventDefault();
      return;
    }
    if (searchOpen() || !view) return;
    const field = editable(e.target);
    if (view.name === 'portal') {
      if ((e.key === 'k' || e.key === 'K') && (e.metaKey || e.ctrlKey) && !e.altKey) { e.preventDefault(); openSearch(d.activeElement); return; }
      if (field || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === '/') { e.preventDefault(); openSearch(d.activeElement); }
      else if (e.key === '[' || e.key === ']') { e.preventDefault(); view.peek(e.key === '[' ? 'nav' : 'toc'); }
    }
  });
  const release = () => requestAnimationFrame(() => requestAnimationFrame(() => root.removeAttribute('data-cf-preload')));
  const auto = () => { const el = $('[data-cf-view]'); if (el && !CF.view) CF.mount(el); release(); };
  if (d.readyState === 'loading') d.addEventListener('DOMContentLoaded', auto); else auto();
})();
