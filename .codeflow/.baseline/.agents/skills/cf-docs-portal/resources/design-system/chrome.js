/* CodeFlow design system runtime (chrome.js). Plain ES2020, no framework, no build.
   Load it in <head> without defer, after the three sheets: the prepaint block runs before first paint,
   and the runtime mounts the page's [data-cf-view] element once the document is parsed.
   Owns: prepaint, Display panel, search dialog, panels and peek sheets, altitude tabs, table of contents,
   ID preview card, Comment state machine, toast.
   Page data: an optional <script type="application/json" id="cf-data"> with records, recent, pages (portal)
   or earlier (present).
   Seams: pagefind (search), router (routes outside the page), session api (review delivery). */

/* ---------- Prepaint: apply stored display choices before first paint ----------
   Self-contained so a product can inline it in <head>.
   Keys: cf.skin, cf.theme, cf.typeface, cf.scale, cf.nav.width, cf.nav.open, cf.toc.open */
(function () {
  var d = document.documentElement;
  function get(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function pick(v, list, def) { return list.indexOf(v) >= 0 ? v : def; }
  var skin = pick(get('cf.skin'), ['graphite', 'slate', 'sage'], 'graphite');
  var pref = pick(get('cf.theme'), ['light', 'dark', 'system'], 'system');
  var dark = pref === 'dark' || (pref === 'system' && window.matchMedia && matchMedia('(prefers-color-scheme: dark)').matches);
  d.setAttribute('data-cf-skin', skin);
  d.setAttribute('data-cf-theme', dark ? 'dark' : 'light');
  d.setAttribute('data-cf-typeface', pick(get('cf.typeface'), ['archivo', 'inter', 'plex'], 'inter'));
  d.setAttribute('data-cf-scale', pick(get('cf.scale'), ['compact', 'default', 'large'], 'default'));
  var w = parseInt(get('cf.nav.width'), 10);
  if (w >= 220 && w <= 360) d.style.setProperty('--cf-nav-w', w + 'px');
  if (get('cf.nav.open') === 'false') d.setAttribute('data-cf-nav', 'collapsed');
  if (get('cf.toc.open') === 'false') d.setAttribute('data-cf-toc', 'collapsed');
  d.setAttribute('data-cf-preload', '');
})();

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
  const mqCardSheet = mq('(max-width: 639.98px)');
  const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
  const editable = el => !!el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const mk = (tag, cls) => { const el = d.createElement(tag); if (cls) el.className = cls; return el; };
  const reflow = el => void el.offsetWidth;
  const dur = name => (mqReduce.matches ? 0 : parseFloat(getComputedStyle(root).getPropertyValue(name)) || 0);
  const smooth = () => (mqReduce.matches ? 'auto' : 'smooth');
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  const CF = window.CF = {};
  const CHEVRON = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6"/></svg>';
  const SPEECH = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5h-7l-4.5 3.5v-3.5H5A1.5 1.5 0 0 1 3.5 15V7A1.5 1.5 0 0 1 5 5.5Z"/></svg>';

  /* ---------- Routes inside the hash: #/path#layer or #layer ---------- */
  function splitRoute(h) {
    h = (h || '').replace(/^#/, '');
    if (!h.startsWith('/')) return ['', h];
    const k = h.indexOf('#');
    return k < 0 ? [h, ''] : [h.slice(0, k), h.slice(k + 1)];
  }
  /* A route outside the mounted page belongs to the host site (seam: router) */
  function leave(path) { toast('Outside this reference page: ' + path); }

  /* ---------- Toast ---------- */
  let toastEl = null, toastT = 0;
  function toast(msg) {
    if (!toastEl) {
      toastEl = mk('div', 'cf-toast');
      toastEl.setAttribute('role', 'status');
      toastEl.setAttribute('aria-live', 'polite');
      d.body.appendChild(toastEl);
    }
    toastEl.textContent = msg;
    clearTimeout(toastT);
    toastEl.classList.remove('is-open');
    reflow(toastEl);
    toastEl.classList.add('is-open');
    toastT = setTimeout(() => toastEl.classList.remove('is-open'), 4000 + dur('--cf-dur-toast-in'));
  }
  CF.toast = toast;

  /* ---------- Display preferences ---------- */
  const PREFS = {
    typeface: { key: 'cf.typeface', attr: 'data-cf-typeface', label: 'Font', desc: 'Body and headings', def: 'inter', options: [['archivo', 'Archivo'], ['inter', 'Inter'], ['plex', 'Plex Sans']] },
    scale: { key: 'cf.scale', attr: 'data-cf-scale', label: 'Size', desc: 'Text and controls', def: 'default', options: [['compact', 'Compact'], ['default', 'Default'], ['large', 'Large']] },
    skin: { key: 'cf.skin', attr: 'data-cf-skin', label: 'Palette', desc: 'Surfaces and accent', def: 'graphite', options: [['graphite', 'Graphite'], ['slate', 'Slate'], ['sage', 'Sage']] },
    theme: { key: 'cf.theme', attr: null, label: 'Appearance', desc: 'Light, dark or the system', def: 'system', options: [['light', 'Light'], ['dark', 'Dark'], ['system', 'System']] }
  };
  function getPref(name) {
    const p = PREFS[name], v = store.get(p.key);
    return p.options.some(o => o[0] === v) ? v : p.def;
  }
  function applyTheme() {
    const pref = getPref('theme');
    root.setAttribute('data-cf-theme', pref === 'system' ? (mqDark.matches ? 'dark' : 'light') : pref);
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
    displayEl.hidden = true;
    displayEl.setAttribute('role', 'dialog');
    displayEl.setAttribute('aria-label', 'Display');
    displayEl.setAttribute('data-cf-chrome', '');
    const groups = Object.entries(PREFS).map(([name, p]) => `
      <div class="cf-dgroup">
        <div class="cf-dgroup-head"><span class="cf-dgroup-label" id="cf-dl-${name}">${p.label}</span><span class="cf-dgroup-desc">${p.desc}</span></div>
        <div class="cf-pills" role="radiogroup" aria-labelledby="cf-dl-${name}" data-pref="${name}">
          ${p.options.map(([v, l]) => `<button type="button" class="cf-pill" role="radio" aria-checked="false" tabindex="-1" data-value="${v}">${name === 'skin' ? `<span class="cf-pill-swatch" data-cf-skin="${v}" data-cf-theme="light" aria-hidden="true"><i class="cf-dot-canvas"></i><i class="cf-dot-accent"></i></span>` : ''}${l}</button>`).join('')}
        </div>
      </div>`).join('');
    displayEl.innerHTML = `
      <div class="cf-display-preview" data-cf-skin="graphite" data-cf-theme="light" aria-hidden="true">
        <span class="cf-sw cf-sw--canvas"></span><span class="cf-sw cf-sw--surface"></span><span class="cf-sw cf-sw--accent"></span>
        <span class="cf-dp-body">Body Aa</span><span class="cf-dp-mono">Mono 012</span>
      </div>${groups}
      <p class="cf-display-foot">Remembered in this browser for every page of this guide. <a href="#/portal/font-licenses">Font licenses</a></p>`;
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
      if (pill) previewEl.setAttribute('data-cf-skin', pill.dataset.value);
    });
    displayEl.addEventListener('pointerout', e => {
      if (e.target.closest('[data-pref="skin"] .cf-pill')) previewEl.setAttribute('data-cf-skin', root.getAttribute('data-cf-skin'));
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
    const theme = root.getAttribute('data-cf-theme');
    previewEl.setAttribute('data-cf-skin', root.getAttribute('data-cf-skin'));
    previewEl.setAttribute('data-cf-theme', theme);
    $$('.cf-pill-swatch', displayEl).forEach(s => s.setAttribute('data-cf-theme', theme));
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
  function render(q) {
    const qs = q.trim();
    S.q = qs;
    let groups;
    if (!qs) {
      groups = [{ title: 'Recent', crumb: '', items: CF.view.recent }];
      S.status.textContent = '';
      S.status.hidden = true;
    } else {
      const hits = CF.view.search(qs);
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
        html += `<div class="cf-hit" role="option" id="cf-opt-${i}" data-i="${i}" aria-selected="false"><span class="cf-hit-main"><span class="cf-hit-title">${highlight(h.title, qs)}</span>${h.text ? `<span class="cf-hit-excerpt">${excerpt(h.text, qs)}</span>` : ''}</span>${h.altitude ? `<span class="cf-chip">${esc(h.altitude)}</span>` : ''}</div>`;
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

  /* Route and anchor follow: in-page when the path matches the mounted view */
  function go(route, anchor) {
    const [path, layer] = splitRoute(route);
    const view = CF.view;
    if (view && view.name === 'portal' && (!path || path === view.path)) {
      if (layer) view.selectLayer(layer, { push: true });
      if (anchor) {
        requestAnimationFrame(() => {
          const el = d.getElementById(anchor);
          if (!el) return;
          el.scrollIntoView({ behavior: smooth(), block: 'start' });
          if (!el.hasAttribute('tabindex')) el.setAttribute('tabindex', '-1');
          el.focus({ preventScroll: true });
        });
      } else if (view.mainEl) view.mainEl.focus({ preventScroll: true });
    } else {
      leave(path);
    }
  }

  /* ---------- ID preview card ---------- */
  let card = null;
  const STATUS_SHAPE = { shipped: 'square', accepted: 'dot', proposed: 'ring', todo: 'ring', planning: 'ring' };
  function ensureCard() {
    if (card) return card;
    card = mk('div', 'cf-preview');
    card.id = 'cf-preview';
    card.setAttribute('role', 'tooltip');
    card.setAttribute('data-cf-chrome', '');
    d.body.appendChild(card);
    return card;
  }

  /* ---------- Portal view ---------- */
  /* Local search over this page's sections plus the page data's entries for other pages (seam: pagefind) */
  function buildIndex(el, path, pages) {
    const clean = n => n.textContent.replace(/\s+/g, ' ').trim();
    const page = clean($('.cf-title', el)), crumb = $$('.cf-crumbs li', el).slice(0, -1).map(clean).join(' / ');
    const index = [];
    $$('.cf-tabpanel', el).forEach(panel => {
      const layer = panel.dataset.layer, altitude = layer[0].toUpperCase() + layer.slice(1), route = '#' + path + '#' + layer;
      $$('[data-cf-section]', panel).forEach(sec => {
        const h2 = $('h2', sec);
        let title = clean(h2), anchor = h2.id;
        const add = text => { if (text) index.push({ page, crumb, title, altitude, route, anchor, text }); };
        Array.from(sec.children).forEach(ch => {
          if (ch.matches('h3')) { title = clean(ch); anchor = ch.id; add(title); }
          else if (ch.matches('p, pre')) add(clean(ch));
          else if (ch.matches('ul')) $$('li', ch).forEach(li => add(clean(li)));
          else if (ch.matches('figure')) {
            add(clean($('figcaption', ch)) + ' Legend: ' + $$('.cf-legend li', ch).map(clean).join(', ') + '.');
            const desc = $('.cf-fig-svg--wide desc', ch);
            if (desc) add(clean(desc));
            const twin = $('.cf-fig-details table', ch);
            if (twin) add('Table twin: ' + $$('tbody tr', twin).map(tr => Array.from(tr.cells).map(clean).join(', ')).join('. '));
          }
        });
        index.push({ page, crumb, title: clean(h2), altitude, route, anchor: h2.id, text: '', isTitle: true });
      });
    });
    return index.concat(pages || []);
  }
  function searchIndex(index, records, q) {
    const needle = q.toLowerCase();
    const recs = Object.keys(records).filter(id => id.toLowerCase().includes(needle)).map(id => ({
      page: 'Records', crumb: 'Jump to an ID', title: id + ' ' + records[id].title, altitude: records[id].kind, route: records[id].route, text: records[id].summary
    }));
    /* One row per section: a title match ranks first; the excerpt is the first text in that section that contains the query */
    const rows = new Map();
    index.forEach((e, i) => {
      if (!(e.isTitle ? e.title : e.text).toLowerCase().includes(needle)) return;
      const k = e.page + '|' + (e.anchor || e.title);
      const row = rows.get(k) || { first: i, titleHit: false, e: null };
      if (e.isTitle) row.titleHit = true; else if (!row.e) row.e = e;
      row.first = Math.min(row.first, i);
      rows.set(k, row);
    });
    const rank = r => (r.titleHit ? 0 : 1000) + r.first;
    const hits = [...rows.values()].sort((a, b) => rank(a) - rank(b)).map(r => {
      if (r.e) return r.e;
      const t = index[r.first], body = index.find(x => !x.isTitle && x.page === t.page && x.anchor === t.anchor);
      return Object.assign({}, t, { text: body ? body.text : '' });
    });
    return recs.concat(hits).slice(0, 30);
  }

  function mountPortal(el, data, signal) {
    const view = { name: 'portal', el, path: el.dataset.cfRoute || '', mainEl: $('#cf-main', el) };
    const records = data.records || {};
    const index = buildIndex(el, view.path, data.pages);
    view.search = q => searchIndex(index, records, q);
    view.recent = data.recent || [];
    const header = $('.cf-header', el);
    const shell = $('.cf-shell', el);

    /* skip link and header shadow */
    $('[data-cf-skip]', el).addEventListener('click', e => { e.preventDefault(); view.mainEl.focus(); view.mainEl.scrollIntoView({ block: 'start' }); });
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

    /* altitude tabs */
    const tablist = $('.cf-tabs', el), tabs = $$('.cf-tab', tablist), ind = $('.cf-tabs-ind', tablist);
    const panels = tabs.map(t => d.getElementById(t.getAttribute('aria-controls')));
    let layer = null;
    function placeInd(t, animate) {
      if (!animate) ind.style.transition = 'none';
      ind.style.width = t.offsetWidth + 'px';
      ind.style.transform = `translateX(${t.offsetLeft}px)`;
      if (!animate) { reflow(ind); ind.style.transition = ''; }
    }
    function layerFromHash() {
      const [, rest] = splitRoute(location.hash);
      return tabs.some(t => t.dataset.layer === rest) ? rest : '';
    }
    function writeHash(l) {
      const [path] = splitRoute(location.hash);
      const next = (path ? '#' + path : '') + '#' + l;
      if (location.hash !== next) history.pushState(null, '', next);
    }
    function selectLayer(name, opt = {}) {
      const t = tabs.find(x => x.dataset.layer === name) || tabs[0];
      const changed = layer !== null && layer !== t.dataset.layer;
      tabs.forEach((x, i) => {
        const on = x === t;
        x.setAttribute('aria-selected', String(on));
        x.tabIndex = on ? 0 : -1;
        panels[i].hidden = !on;
      });
      if (changed) {
        const p = panels[tabs.indexOf(t)];
        p.classList.remove('is-entering');
        reflow(p);
        p.classList.add('is-entering');
      }
      const first = layer === null;
      layer = t.dataset.layer;
      placeInd(t, !first);
      if (opt.focus) t.focus();
      if (opt.push) writeHash(layer);
      $$('[data-cf-layer-link]', el).forEach(a => a.classList.toggle('is-active', a.dataset.cfLayerLink === layer));
      buildToc();
    }
    panels.forEach(p => p.addEventListener('animationend', () => p.classList.remove('is-entering'), { signal }));
    tablist.addEventListener('click', e => {
      const t = e.target.closest('.cf-tab');
      if (t) selectLayer(t.dataset.layer, { push: true });
    }, { signal });
    tablist.addEventListener('keydown', e => {
      const i = tabs.indexOf(d.activeElement);
      if (i < 0) return;
      const j = { ArrowRight: (i + 1) % tabs.length, ArrowLeft: (i - 1 + tabs.length) % tabs.length, Home: 0, End: tabs.length - 1 }[e.key];
      if (j === undefined) return;
      e.preventDefault();
      selectLayer(tabs[j].dataset.layer, { push: true, focus: true });
    }, { signal });
    const fromHash = () => { const l = layerFromHash(); if (l !== layer) selectLayer(l || tabs[0].dataset.layer); };
    addEventListener('hashchange', fromHash, { signal });
    addEventListener('popstate', fromHash, { signal });

    /* table of contents with scroll spy */
    const tocLists = $$('[data-cf-toc]', el);
    let heads = [], activeId = '';
    function buildToc() {
      const panel = panels.find(p => !p.hidden);
      heads = $$('h2[id], h3[id]', panel);
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
      h.scrollIntoView({ behavior: smooth(), block: 'start' });
      h.setAttribute('tabindex', '-1');
      h.focus({ preventScroll: true });
    }, { signal }));

    /* ID previews */
    const pc = ensureCard();
    let showT = 0, hideCardT = 0, cur = null, dismissed = null, lastPointer = 'mouse';
    function openCard(a) {
      const r = records[a.dataset.cfId];
      if (!r) return;
      clearTimeout(hideCardT);
      cur = a;
      const shape = STATUS_SHAPE[r.status] || 'ring';
      pc.innerHTML = `<span class="cf-kicker">${esc(r.kind)} · ${esc(a.dataset.cfId)}</span>
        <p class="cf-preview-title">${esc(r.title)}</p>
        <span class="cf-status-chip"><span class="cf-shape cf-shape--${shape}" aria-hidden="true"></span>${esc(r.statusLabel || r.status)}</span>
        <p class="cf-preview-sum">${esc(r.summary)}</p>
        <div class="cf-preview-foot"><a href="${esc(a.getAttribute('href'))}" tabindex="-1">Open</a></div>`;
      a.setAttribute('aria-describedby', 'cf-preview');
      pc.classList.remove('is-above');
      if (mqCardSheet.matches) {
        pc.classList.add('is-sheet-mode');
        pc.style.left = ''; pc.style.top = '';
      } else {
        pc.classList.remove('is-sheet-mode');
        const rect = a.getBoundingClientRect(), w = pc.offsetWidth;
        pc.style.left = (clamp(rect.left, 12, innerWidth - w - 12) + scrollX) + 'px';
        if (innerHeight - rect.bottom < 200) {
          pc.classList.add('is-above');
          pc.style.top = (rect.top + scrollY - pc.offsetHeight - 8) + 'px';
        } else {
          pc.style.top = (rect.bottom + scrollY + 8) + 'px';
        }
      }
      pc.classList.add('is-open');
    }
    function closeCard() {
      clearTimeout(showT);
      if (cur) cur.removeAttribute('aria-describedby');
      cur = null;
      pc.classList.remove('is-open');
    }
    el.addEventListener('pointerdown', e => { lastPointer = e.pointerType || 'mouse'; }, { signal });
    el.addEventListener('pointerover', e => {
      const a = e.target.closest('.cf-id');
      if (!a || e.pointerType === 'touch') return;
      clearTimeout(hideCardT); clearTimeout(showT);
      if (cur !== a) showT = setTimeout(() => openCard(a), 120);
    }, { signal });
    el.addEventListener('pointerout', e => {
      const a = e.target.closest('.cf-id');
      if (!a || (e.relatedTarget && a.contains(e.relatedTarget))) return;
      clearTimeout(showT);
      hideCardT = setTimeout(closeCard, 160);
    }, { signal });
    pc.addEventListener('pointerenter', () => clearTimeout(hideCardT), { signal });
    pc.addEventListener('pointerleave', () => { hideCardT = setTimeout(closeCard, 160); }, { signal });
    el.addEventListener('focusin', e => { const a = e.target.closest('.cf-id'); if (a && a !== dismissed) openCard(a); }, { signal });
    el.addEventListener('focusout', e => { const a = e.target.closest('.cf-id'); if (a) { if (dismissed === a) dismissed = null; closeCard(); } }, { signal });
    el.addEventListener('click', e => {
      const a = e.target.closest('.cf-id');
      if (a && lastPointer === 'touch' && cur !== a) { e.preventDefault(); openCard(a); }
    }, { signal });
    signal.addEventListener('abort', closeCard);

    /* public view api */
    view.selectLayer = selectLayer;
    view.peek = name => { if (name === 'nav' ? navHidden() : tocHidden()) toggleSheet(name, true); else if (openSheet) hideSheet(openSheet, true); };
    view.relayout = () => { const t = tabs.find(x => x.getAttribute('aria-selected') === 'true'); if (t) placeInd(t, false); spy(); };
    view.onEscape = () => {
      if (cur) { dismissed = cur; closeCard(); return true; }
      if (openSheet) return hideSheet(openSheet, sheetKeyboard);
      return false;
    };
    d.addEventListener('cf:prefs', view.relayout, { signal });
    addEventListener('resize', () => { view.relayout(); if (displayOpen()) placeDisplay(); }, { signal });

    selectLayer(layerFromHash() || tabs[0].dataset.layer);
    return view;
  }

  /* ---------- Present view: Comment state machine ---------- */
  function mountPresent(app, data, signal) {
    const view = { name: 'present', el: app };
    const doc = $('[data-cf-document]', app), wrap = $('.cf-doc-wrap', app), layer = $('[data-cf-markers]', app);
    const toggle = $('.cf-comment-toggle', app), badge = $('[data-cf-badge]', app), meta = $('[data-cf-note-meta]', app);
    const stripText = $('[data-cf-strip-text]', app), rail = $('.cf-rail', app);
    const list = $('[data-cf-notes]', rail), empty = $('[data-cf-empty]', rail), railCount = $('[data-cf-rail-count]', rail);
    const verdictSel = $('[data-cf-verdict]', rail), summary = $('[data-cf-summary]', rail), summaryLabel = $('[data-cf-summary-label]', rail);
    const summaryErr = $('[data-cf-summary-error]', rail), submitBtn = $('[data-cf-act="submit"]', rail), envSlot = $('[data-cf-envelope-slot]', rail);
    const st = { armed: false, tool: null, float: null, composer: null, notes: [], lastRange: null, drag: null, seq: 0 };
    const STRIP = {
      none: 'Comment: select words, click a figure part, or drag a box. Esc leaves.',
      element: 'Pick element: Tab to a figure part and press Enter, or click it. Esc cancels the tool.',
      area: 'Select area: drag a box anywhere, even over text. Esc cancels the tool.'
    };
    const HL = typeof Highlight === 'function' && window.CSS && CSS.highlights;
    rail.inert = true;

    const hover = mk('div', 'cf-hoverbox'); hover.hidden = true;
    const marquee = mk('div', 'cf-marquee'); marquee.hidden = true;
    layer.append(hover, marquee);
    const floatEl = mk('button', 'cf-float');
    floatEl.type = 'button';
    floatEl.hidden = true;
    floatEl.setAttribute('data-cf-chrome', '');
    const comp = mk('div', 'cf-composer');
    comp.hidden = true;
    comp.setAttribute('role', 'dialog');
    comp.setAttribute('aria-labelledby', 'cf-comp-title');
    comp.setAttribute('data-cf-chrome', '');
    comp.innerHTML = `<h2 class="cf-composer-title" id="cf-comp-title">Comment</h2>
      <p class="cf-composer-quote"></p>
      <label class="cf-sr" for="cf-comp-body">Note</label>
      <textarea class="cf-textarea" id="cf-comp-body" rows="4" placeholder="What should change?"></textarea>
      <p class="cf-composer-error" hidden>Write a note before saving.</p>
      <div class="cf-composer-actions"><button type="button" class="cf-btn cf-btn--danger" data-act="delete">Delete</button><span class="cf-grow"></span><button type="button" class="cf-btn" data-act="cancel">Cancel</button><button type="button" class="cf-btn cf-btn--primary" data-act="save">Save note</button></div>
      <p class="cf-composer-hint"><kbd class="cf-kbd">${isMac ? '⌘' : 'Ctrl'}</kbd> <kbd class="cf-kbd">Enter</kbd> saves</p>`;
    d.body.append(floatEl, comp);
    signal.addEventListener('abort', () => { floatEl.remove(); comp.remove(); if (HL) { CSS.highlights.delete('cf-quote'); CSS.highlights.delete('cf-notes'); } });
    const compTitle = $('.cf-composer-title', comp), compQuote = $('.cf-composer-quote', comp), compBody = $('textarea', comp), compErr = $('.cf-composer-error', comp), compDel = $('[data-act="delete"]', comp);

    const inChrome = n => { const e = n && (n.nodeType === 1 ? n : n.parentElement); return !!(e && e.closest('[data-cf-chrome]')); };
    const anchorAt = t => { const a = t && t.closest && t.closest('[data-cf-anchor]'); return a && doc.contains(a) ? a : null; };
    const blockAt = t => (t && t.closest && t.closest('[data-cf-block-id]')) || doc;
    const blockKind = b => (b === doc ? 'document' : (b.dataset.cfFigure ? 'figure' : (b.dataset.cfBlockKind || 'block')));
    const isTextTarget = t => !!t && !t.closest('svg') && !!t.closest('p, li, h1, h2, h3, h4, figcaption, td, th, summary, pre, code, strong, em, a, label, .cf-status-word, .cf-status-name');
    const visibleAnchor = id => $$('[data-cf-anchor="' + id + '"]', doc).find(e => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; }) || null;
    const quoteOf = r => r.toString().replace(/\s+/g, ' ').trim();
    const short = (s, n) => (s.length > n ? s.slice(0, n).trimEnd() + '…' : s);

    function setStrip() { stripText.textContent = STRIP[st.tool || 'none']; }
    function arm() {
      st.armed = true;
      app.setAttribute('data-cf-comment', 'armed');
      rail.inert = false;
      toggle.setAttribute('aria-pressed', 'true');
      toggle.title = 'Exit Comment mode (C or Esc)';
      setStrip();
      relayoutSoon();
    }
    function exit() {
      setTool(null);
      hideFloat(false);
      closeComposer(false);
      hoverOff();
      st.armed = false;
      app.removeAttribute('data-cf-comment');
      rail.inert = true;
      toggle.setAttribute('aria-pressed', 'false');
      toggle.title = 'Comment mode (C)';
      toggle.focus();
      relayoutSoon();
    }
    view.toggleComment = () => (st.armed ? exit() : arm());
    toggle.addEventListener('click', view.toggleComment, { signal });
    $('[data-cf-act="close"]', rail).addEventListener('click', exit, { signal });

    /* hover outline on anchors */
    function hoverOn(a) {
      const wr = wrap.getBoundingClientRect(), r = a.getBoundingClientRect();
      Object.assign(hover.style, { left: (r.left - wr.left - 4) + 'px', top: (r.top - wr.top - 4) + 'px', width: (r.width + 8) + 'px', height: (r.height + 8) + 'px' });
      hover.hidden = false;
    }
    function hoverOff() { hover.hidden = true; }

    /* float */
    function showFloat(spec, focus) {
      st.float = spec;
      const label = spec.kind === 'text' ? 'Text' : spec.kind === 'element' ? 'Element' : 'Area';
      const detail = spec.kind === 'text' ? '“' + short(spec.quote, 40) + '”' : spec.label;
      floatEl.innerHTML = `${SPEECH}<span class="cf-float-kind">${label}</span><span class="cf-float-text">${spec.kind === 'text' ? '' : '· '}${esc(detail)}</span><kbd class="cf-kbd" aria-hidden="true">esc</kbd>`;
      floatEl.setAttribute('aria-label', `Comment on ${label.toLowerCase()}: ${detail}`);
      floatEl.hidden = false;
      placeFloat();
      if (spec.kind === 'text' && HL) CSS.highlights.set('cf-quote', new Highlight(spec.range));
      if (focus) floatEl.focus();
    }
    function placeFloat() {
      const spec = st.float, r = spec.rect(), w = floatEl.offsetWidth, h = floatEl.offsetHeight;
      let top = r.bottom + 8;
      if (spec.kind === 'text') {
        /* Keep the paragraph readable: above the selection when it starts in the top half of a multi-line text block,
           else 6 px under the block's last line. A one-line block always goes below, so a chip never covers the heading above it */
        const sc = spec.range.startContainer, se = sc.nodeType === 1 ? sc : sc.parentElement;
        const tb = (se.closest('p, li, dd, dt, h1, h2, h3, h4, blockquote, figcaption, td, th, pre') || spec.blockEl).getBoundingClientRect();
        const first = spec.range.getClientRects()[0] || r;
        const multiLine = tb.height >= first.height * 1.8;
        top = multiLine && first.top < tb.top + tb.height / 2 && first.top - h - 6 > 8 ? first.top - h - 6 : tb.bottom + 6;
      }
      floatEl.style.left = (clamp(r.left, 12, innerWidth - w - 12) + scrollX) + 'px';
      floatEl.style.top = (top + scrollY) + 'px';
    }
    function hideFloat(clearSel) {
      if (!st.float) return false;
      st.float = null;
      floatEl.hidden = true;
      if (HL) CSS.highlights.delete('cf-quote');
      if (clearSel) { const s = getSelection(); if (s) s.removeAllRanges(); st.lastRange = null; }
      return true;
    }
    floatEl.addEventListener('pointerdown', e => e.preventDefault(), { signal });
    floatEl.addEventListener('click', () => { if (st.float) openComposer({ mode: 'new', spec: st.float }); }, { signal });

    /* specs */
    function textSpec(range) {
      const r = range.cloneRange();
      const start = r.startContainer.nodeType === 1 ? r.startContainer : r.startContainer.parentElement;
      const blk = start.closest('.cf-block') || doc;
      const pre = d.createRange(); pre.selectNodeContents(blk); pre.setEnd(r.startContainer, r.startOffset);
      const post = d.createRange(); post.selectNodeContents(blk); post.setStart(r.endContainer, r.endOffset);
      const quote = quoteOf(r);
      return {
        kind: 'text', range: r, quote, blockEl: blk, blockId: blk.dataset.cfBlockId || blk.id || 'document',
        prefix: pre.toString().replace(/\s+/g, ' ').slice(-32), suffix: post.toString().replace(/\s+/g, ' ').slice(0, 32),
        label: '“' + short(quote, 60) + '”',
        rect: () => { const rs = r.getClientRects(); return rs.length ? rs[rs.length - 1] : r.getBoundingClientRect(); }
      };
    }
    function elementSpec(a) {
      const id = a.dataset.cfAnchor, blk = a.closest('[data-cf-block-id]') || doc;
      let h = 5381; const s = id + '|' + a.textContent.trim();
      for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) >>> 0;
      return {
        kind: 'element', anchorId: id, label: a.dataset.cfAnchorLabel || id, blockEl: blk, blockId: blk.dataset.cfBlockId || 'document',
        path: `[data-cf-block-id=${blk.dataset.cfBlockId || 'document'}] > [data-cf-anchor=${id}]`, digest: h.toString(16).padStart(8, '0'),
        rect: () => { const e = visibleAnchor(id); return e ? e.getBoundingClientRect() : a.getBoundingClientRect(); }
      };
    }
    function areaSpec(blk, box) {
      const b = blk.getBoundingClientRect();
      const x0 = clamp(box.left, b.left, b.right), x1 = clamp(box.right, b.left, b.right);
      const y0 = clamp(box.top, b.top, b.bottom), y1 = clamp(box.bottom, b.top, b.bottom);
      const area = { x: (x0 - b.left) / b.width, y: (y0 - b.top) / b.height, w: (x1 - x0) / b.width, h: (y1 - y0) / b.height };
      const pct = Math.max(1, Math.round(area.w * area.h * 100));
      const spec = {
        kind: 'area', area, blockEl: blk, blockId: blk.dataset.cfBlockId || 'document',
        label: `${pct} percent of ${blockKind(blk)}`,
        box: () => { const c = spec.blockEl.getBoundingClientRect(); return { left: c.left + area.x * c.width, top: c.top + area.y * c.height, width: area.w * c.width, height: area.h * c.height }; },
        rect: () => { const q = spec.box(); return { left: q.left, top: q.top, right: q.left + q.width, bottom: q.top + q.height }; }
      };
      return spec;
    }

    /* pointer gestures */
    doc.addEventListener('pointermove', e => {
      if (st.drag) return;
      if (!st.armed || st.composer || st.tool === 'area') { hoverOff(); return; }
      const sel = getSelection();
      if (sel && !sel.isCollapsed) { hoverOff(); return; }
      const a = anchorAt(e.target);
      if (a) hoverOn(a); else hoverOff();
    }, { signal });
    doc.addEventListener('pointerleave', () => { if (!st.drag && st.tool !== 'element') hoverOff(); }, { signal });
    doc.addEventListener('pointerdown', e => {
      if (!st.armed || e.button !== 0 || inChrome(e.target)) return;
      const anchor = anchorAt(e.target);
      if (isTextTarget(e.target) && !anchor && !e.shiftKey && st.tool !== 'area') return; /* native text selection */
      e.preventDefault();
      if (st.composer) closeComposer(false);
      hideFloat(true);
      doc.setPointerCapture(e.pointerId);
      st.drag = { x: e.clientX, y: e.clientY, anchor: st.tool === 'area' ? null : anchor, block: blockAt(e.target), moving: false };
    }, { signal });
    doc.addEventListener('pointermove', e => {
      const g = st.drag;
      if (!g) return;
      if (!g.moving && Math.hypot(e.clientX - g.x, e.clientY - g.y) > 6) { g.moving = true; hoverOff(); marquee.hidden = false; }
      if (!g.moving) return;
      const wr = wrap.getBoundingClientRect();
      Object.assign(marquee.style, {
        left: (Math.min(g.x, e.clientX) - wr.left) + 'px', top: (Math.min(g.y, e.clientY) - wr.top) + 'px',
        width: Math.abs(e.clientX - g.x) + 'px', height: Math.abs(e.clientY - g.y) + 'px'
      });
    }, { signal });
    const endDrag = e => {
      const g = st.drag;
      if (!g) return;
      st.drag = null;
      marquee.hidden = true;
      if (g.moving) {
        const box = { left: Math.min(g.x, e.clientX), right: Math.max(g.x, e.clientX), top: Math.min(g.y, e.clientY), bottom: Math.max(g.y, e.clientY) };
        showFloat(areaSpec(g.block, box), true);
      } else if (g.anchor) {
        showFloat(elementSpec(g.anchor), true);
      }
    };
    doc.addEventListener('pointerup', endDrag, { signal });
    doc.addEventListener('pointercancel', () => { st.drag = null; marquee.hidden = true; }, { signal });

    /* keyboard element picking */
    doc.addEventListener('keydown', e => {
      const a = anchorAt(e.target);
      if (!a || st.tool !== 'element' || (e.key !== 'Enter' && e.key !== ' ')) return;
      e.preventDefault();
      showFloat(elementSpec(a), true);
    }, { signal });
    doc.addEventListener('focusin', e => { const a = anchorAt(e.target); if (a && st.tool === 'element') hoverOn(a); }, { signal });

    /* text selection */
    let selT = 0;
    function checkSelection() {
      const sel = getSelection();
      if (!sel || !sel.rangeCount) return;
      const r = sel.getRangeAt(0);
      if (!sel.isCollapsed && doc.contains(r.commonAncestorContainer) && !inChrome(r.commonAncestorContainer) && quoteOf(r)) {
        st.lastRange = r.cloneRange();
        if (st.armed && !st.composer && !st.drag && st.tool !== 'area') showFloat(textSpec(r), false);
      } else if (st.float && st.float.kind === 'text' && !st.composer && d.activeElement !== floatEl) {
        hideFloat(false);
      }
    }
    d.addEventListener('selectionchange', () => { clearTimeout(selT); selT = setTimeout(checkSelection, 140); }, { signal });

    /* composer */
    function placeComposer(r) {
      const w = comp.offsetWidth, h = comp.offsetHeight;
      let top = r.bottom + 8;
      if (top + h > innerHeight - 12 && r.top - h - 8 > 12) top = r.top - h - 8;
      comp.style.left = (clamp(r.left, 12, innerWidth - w - 12) + scrollX) + 'px';
      comp.style.top = (Math.max(12, top) + scrollY) + 'px';
    }
    function openComposer(o) {
      const spec = o.mode === 'edit' ? o.note : o.spec;
      st.composer = o;
      compTitle.textContent = o.mode === 'edit' ? 'Edit note' : 'Comment';
      const kind = spec.kind === 'text' ? 'Text' : spec.kind === 'element' ? 'Element' : 'Area';
      compQuote.innerHTML = `<b>${kind}</b> · ${esc(spec.kind === 'text' ? '“' + short(spec.quote, 120) + '”' : spec.label)}`;
      compBody.value = o.mode === 'edit' ? o.note.body : '';
      compErr.hidden = true;
      compDel.hidden = o.mode !== 'edit';
      comp.hidden = false;
      if (spec.kind === 'text' && HL) CSS.highlights.set('cf-quote', new Highlight(spec.range));
      let anchorRect;
      if (!floatEl.hidden && o.mode === 'new') anchorRect = floatEl.getBoundingClientRect();
      else if (o.mode === 'edit' && o.note.marker && !o.note.marker.hidden) {
        o.note.marker.scrollIntoView({ block: 'center', behavior: 'auto' });
        anchorRect = o.note.marker.getBoundingClientRect();
      } else anchorRect = spec.rect();
      placeComposer(anchorRect);
      compBody.focus({ preventScroll: true });
    }
    function closeComposer(returnFocus) {
      if (!st.composer) return false;
      const o = st.composer;
      st.composer = null;
      comp.hidden = true;
      if (HL && !st.float) CSS.highlights.delete('cf-quote');
      if (returnFocus) {
        if (st.float && !floatEl.hidden) floatEl.focus();
        else if (o.mode === 'edit' && o.note.row) o.note.row.focus();
      }
      return true;
    }
    function saveComposer() {
      const o = st.composer;
      if (!o) return;
      const body = compBody.value.trim();
      if (!body) { compErr.hidden = false; compBody.focus(); return; }
      if (o.mode === 'edit') o.note.body = body;
      else {
        const note = Object.assign({}, o.spec, { id: ++st.seq, body });
        st.notes.push(note);
        if (verdictSel.value === 'approve') { verdictSel.value = 'approve-with-notes'; syncVerdict(); }
      }
      closeComposer(false);
      hideFloat(true);
      renderNotes();
      const n = o.mode === 'edit' ? o.note : st.notes[st.notes.length - 1];
      if (n.marker) n.marker.focus({ preventScroll: true });
    }
    comp.addEventListener('click', e => {
      const b = e.target.closest('[data-act]');
      if (!b) return;
      if (b.dataset.act === 'save') saveComposer();
      else if (b.dataset.act === 'cancel') closeComposer(true);
      else if (b.dataset.act === 'delete' && st.composer && st.composer.mode === 'edit') { removeNote(st.composer.note); closeComposer(false); toggle.focus(); }
    }, { signal });
    compBody.addEventListener('keydown', e => {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); saveComposer(); }
    }, { signal });

    /* notes, markers, rail */
    function removeNote(n) {
      st.notes = st.notes.filter(x => x !== n);
      if (n.marker) n.marker.remove();
      if (n.region) n.region.remove();
      if (!st.notes.length && verdictSel.value === 'approve-with-notes') { verdictSel.value = 'approve'; syncVerdict(); }
      renderNotes();
    }
    function targetText(n) { return n.kind === 'text' ? '“' + short(n.quote, 80) + '”' : n.label; }
    function renderNotes() {
      const count = st.notes.length;
      st.notes.forEach((n, i) => {
        if (!n.marker) {
          n.marker = mk('button', 'cf-marker');
          n.marker.type = 'button';
          n.marker.setAttribute('data-cf-chrome', '');
          n.marker.addEventListener('click', () => openComposer({ mode: 'edit', note: n }));
          layer.appendChild(n.marker);
          if (n.kind === 'area') { n.region = mk('div', 'cf-region-mark'); layer.appendChild(n.region); }
        }
        n.marker.textContent = String(i + 1);
        n.marker.setAttribute('aria-label', `Note ${i + 1}, ${n.kind}: ${targetText(n)}`);
      });
      list.innerHTML = '';
      st.notes.forEach((n, i) => {
        const row = mk('div', 'cf-note-row');
        row.setAttribute('role', 'listitem');
        row.tabIndex = 0;
        row.innerHTML = `<div class="cf-note-kicker">${SPEECH}<span class="cf-kicker">${n.kind} · #${i + 1}</span><button type="button" class="cf-link-btn" data-act="remove">Remove</button></div>
          <div class="cf-note-target">${esc(targetText(n))}</div><div class="cf-note-body">${esc(n.body)}</div>`;
        row.addEventListener('click', e => { if (e.target.closest('[data-act="remove"]')) { removeNote(n); return; } openComposer({ mode: 'edit', note: n }); });
        row.addEventListener('keydown', e => { if (e.key === 'Enter' && e.target === row) { e.preventDefault(); openComposer({ mode: 'edit', note: n }); } });
        n.row = row;
        list.appendChild(row);
      });
      empty.hidden = count > 0;
      badge.textContent = String(count);
      badge.classList.toggle('has-count', count > 0);
      badge.setAttribute('aria-label', `${count} note${count === 1 ? '' : 's'}`);
      railCount.textContent = String(count);
      meta.textContent = `${count} note${count === 1 ? '' : 's'}`;
      meta.hidden = count === 0;
      submitBtn.textContent = `Submit review (${count})`;
      if (HL) {
        const ranges = st.notes.filter(n => n.kind === 'text').map(n => n.range);
        if (ranges.length) CSS.highlights.set('cf-notes', new Highlight(...ranges)); else CSS.highlights.delete('cf-notes');
      }
      placeAll();
    }
    function anchorPoint(n) {
      if (n.kind === 'text') {
        const rs = n.range.getClientRects();
        if (!rs.length) return null;
        /* Gutter marker: 32 px left of the block, never left of the document's own content edge (the title and meta sit outside any block) */
        const b = n.blockEl.getBoundingClientRect(), dr = doc.getBoundingClientRect();
        const edge = Math.max(b.left, dr.left + (parseFloat(getComputedStyle(doc).paddingLeft) || 0));
        return { x: edge - 32, y: rs[0].top + rs[0].height / 2 - 11 };
      }
      if (n.kind === 'element') {
        const e = visibleAnchor(n.anchorId);
        if (!e) return null;
        const r = e.getBoundingClientRect();
        return { x: r.right + 2, y: r.top - 18 };
      }
      const q = n.box();
      return q.width ? { x: q.left - 11, y: q.top - 11 } : null;
    }
    function placeAll() {
      const wr = wrap.getBoundingClientRect();
      st.notes.forEach(n => {
        const p = anchorPoint(n);
        if (!n.marker) return;
        n.marker.hidden = !p;
        if (p) n.marker.style.transform = `translate(${Math.round(p.x - wr.left)}px, ${Math.round(p.y - wr.top)}px)`;
        if (n.region) {
          const q = n.box();
          Object.assign(n.region.style, { left: (q.left - wr.left) + 'px', top: (q.top - wr.top) + 'px', width: q.width + 'px', height: q.height + 'px' });
        }
      });
      if (st.float && !floatEl.hidden) placeFloat();
    }
    function relayoutSoon() { placeAll(); setTimeout(placeAll, dur('--cf-dur-panel') + 40); }
    view.relayout = placeAll;
    const ro = new ResizeObserver(() => placeAll());
    ro.observe(wrap);
    signal.addEventListener('abort', () => ro.disconnect());
    addEventListener('resize', () => { placeAll(); if (displayOpen()) placeDisplay(); }, { signal });
    d.addEventListener('cf:prefs', () => requestAnimationFrame(placeAll), { signal });

    /* tools */
    function setTool(tool) {
      st.tool = tool;
      if (tool) app.setAttribute('data-cf-tool', tool); else app.removeAttribute('data-cf-tool');
      $$('[data-cf-tool]', rail).forEach(b => { if (b.dataset.cfTool !== 'text') b.setAttribute('aria-pressed', String(b.dataset.cfTool === tool)); });
      $$('[data-cf-anchor]', doc).forEach(a => {
        if (tool === 'element') {
          const r = a.getBoundingClientRect();
          if (r.width > 0) { a.setAttribute('tabindex', '0'); a.setAttribute('role', 'button'); a.setAttribute('aria-label', a.dataset.cfAnchorLabel || a.dataset.cfAnchor); }
        } else if (a.hasAttribute('tabindex')) { a.removeAttribute('tabindex'); a.removeAttribute('role'); a.removeAttribute('aria-label'); }
      });
      if (tool === 'element') { const first = $('[data-cf-anchor][tabindex="0"]', doc); if (first) first.focus(); }
      if (!tool) hoverOff();
      setStrip();
    }
    rail.addEventListener('click', e => {
      const t = e.target.closest('[data-cf-tool]');
      if (!t) return;
      const tool = t.dataset.cfTool;
      if (tool === 'text') {
        const r = st.lastRange;
        if (!r || r.collapsed || !doc.contains(r.commonAncestorContainer) || !quoteOf(r)) { toast('Select words in the document first.'); return; }
        const spec = textSpec(r);
        hideFloat(false);
        openComposer({ mode: 'new', spec });
        return;
      }
      setTool(st.tool === tool ? null : tool);
    }, { signal });

    /* verdict and submit */
    function syncVerdict() {
      const req = verdictSel.value === 'request-changes';
      summaryLabel.textContent = req ? 'Required change' : 'Review summary (optional)';
      summary.required = req;
      if (!req) summaryErr.hidden = true;
    }
    verdictSel.addEventListener('change', syncVerdict, { signal });
    submitBtn.addEventListener('click', () => {
      const verdict = verdictSel.value, text = summary.value.trim();
      if (verdict === 'request-changes' && !text) { summaryErr.hidden = false; summary.focus(); return; }
      summaryErr.hidden = true;
      const serial = n => (n.kind === 'text'
        ? { type: 'text-quote', block: n.blockId, exact: n.quote, prefix: n.prefix, suffix: n.suffix }
        : n.kind === 'element'
          ? { type: 'element', block: n.blockId, path: n.path, digest: n.digest }
          : { type: 'area', block: n.blockId, x: +n.area.x.toFixed(4), y: +n.area.y.toFixed(4), w: +n.area.w.toFixed(4), h: +n.area.h.toFixed(4) });
      const env = {
        subject: app.dataset.cfSubject, revision: Number(app.dataset.cfRevision), verdict, summary: text || null,
        notes: st.notes.map((n, i) => ({ n: i + 1, kind: n.kind, quote: n.kind === 'text' ? n.quote : null, target: n.kind === 'text' ? null : n.label, body: n.body, anchor: serial(n) }))
      };
      const count = st.notes.length, vtxt = verdictSel.options[verdictSel.selectedIndex].text.toLowerCase();
      /* seam: session api. A product posts env; the kit shows it */
      envSlot.innerHTML = `<details class="cf-disclosure cf-envelope"><summary>${CHEVRON}Envelope (mock)</summary><pre>${esc(JSON.stringify(env, null, 2))}</pre></details>`;
      toast(`Review submitted: ${count} note${count === 1 ? '' : 's'}, ${vtxt}`);
      st.notes.slice().forEach(n => { if (n.marker) n.marker.remove(); if (n.region) n.region.remove(); });
      st.notes = [];
      summary.value = '';
      verdictSel.value = 'approve';
      syncVerdict();
      renderNotes();
    }, { signal });

    /* earlier feedback */
    const earlier = data.earlier || [];
    const eb = $('[data-cf-earlier-body]', rail);
    const VERDICT = { approve: ['dot', 'positive', 'Approve'], 'approve-with-notes': ['dot', 'positive', 'Approve with notes'], 'request-changes': ['cross', 'danger', 'Request changes'] };
    eb.innerHTML = earlier.length ? earlier.map(rv => {
      const v = VERDICT[rv.verdict] || VERDICT.approve;
      return `<div class="cf-earlier-head"><span>Revision ${esc(rv.revision)}</span><span class="cf-status-chip"><span class="cf-shape cf-shape--${v[0]} cf-tone-${v[1]}" aria-hidden="true"></span>${v[2]}</span><span>${esc(rv.date)}</span></div>` +
        rv.notes.map(n => {
          const orphan = n.state === 'orphaned';
          return `<div class="cf-earlier-note"><span class="cf-earlier-state"><span class="cf-shape cf-shape--${orphan ? 'ring' : 'dot'} cf-tone-${orphan ? 'warning' : 'positive'}" aria-hidden="true"></span>${orphan ? 'Unresolved: ' + esc(n.reason) : 'Reanchored'}</span><p class="cf-note-target">${n.kind} · ${esc(n.target)}</p><p class="cf-note-body">${esc(n.body)}</p></div>`;
        }).join('');
    }).join('') : '<p class="cf-note-target">No earlier feedback.</p>';

    /* section route */
    const routeLinks = $$('.cf-route .cf-toc-link', app);
    routeLinks.forEach(a => a.addEventListener('click', e => {
      e.preventDefault();
      const t = d.getElementById(a.dataset.target);
      if (t) t.scrollIntoView({ behavior: smooth(), block: 'start' });
    }, { signal }));
    const blocks = routeLinks.map(a => d.getElementById(a.dataset.target)).filter(Boolean);
    let routeRaf = 0;
    const spyRoute = () => {
      routeRaf = 0;
      let act = blocks[0];
      blocks.forEach(b => { if (b.getBoundingClientRect().top <= 140) act = b; });
      if (innerHeight + scrollY >= root.scrollHeight - 4) act = blocks[blocks.length - 1];
      routeLinks.forEach(a => a.classList.toggle('is-active', a.dataset.target === act.id));
    };
    addEventListener('scroll', () => { if (!routeRaf) routeRaf = requestAnimationFrame(spyRoute); }, { passive: true, signal });
    spyRoute();

    view.onEscape = () => {
      if (closeComposer(true)) return true;
      if (st.float) { const f = d.activeElement === floatEl; hideFloat(true); if (f) toggle.focus(); return true; }
      if (st.tool) { setTool(null); return true; }
      if (st.armed) { exit(); return true; }
      return false;
    };
    view.state = st;
    setStrip();
    renderNotes();
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
    CF.view = el.dataset.cfView === 'present' ? mountPresent(el, data, ac.signal) : mountPortal(el, data, ac.signal);
    el.setAttribute('data-cf-mounted', '');
    return CF.view;
  };

  /* ---------- Global wiring (once) ---------- */
  d.addEventListener('click', e => {
    const a = e.target.closest('a[href^="#/"]');
    if (!a || e.defaultPrevented) return;
    const [path] = splitRoute(a.getAttribute('href'));
    if (CF.view && path === CF.view.path) return;
    e.preventDefault();
    leave(path);
  });
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
    } else if (view.name === 'present') {
      if (field || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === 'c' || e.key === 'C') { e.preventDefault(); view.toggleComment(); }
    }
  });
  const release = () => requestAnimationFrame(() => requestAnimationFrame(() => root.removeAttribute('data-cf-preload')));
  const auto = () => { const el = $('[data-cf-view]'); if (el && !CF.view) CF.mount(el); release(); };
  if (d.readyState === 'loading') d.addEventListener('DOMContentLoaded', auto); else auto();
})();
