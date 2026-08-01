const root = document.documentElement;
const themeKey = `codeflow-prototype-theme:${document.body.dataset.prototype}`;
const themeButton = document.querySelector('[data-theme-toggle]');
const searchDialog = document.querySelector('[data-search-dialog]');
const searchInput = searchDialog?.querySelector('input');
const searchResults = searchDialog?.querySelector('[data-search-results]');
const searchItems = [...document.querySelectorAll('[data-search-item]')];

function applyTheme(theme) {
  root.dataset.theme = theme;
  themeButton?.setAttribute('aria-label', `Use ${theme === 'dark' ? 'light' : 'dark'} mode`);
  themeButton?.setAttribute('aria-pressed', String(theme === 'dark'));
}

const storedTheme = localStorage.getItem(themeKey);
const preferredTheme = matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
applyTheme(storedTheme || preferredTheme);

themeButton?.addEventListener('click', () => {
  const next = root.dataset.theme === 'dark' ? 'light' : 'dark';
  applyTheme(next);
  localStorage.setItem(themeKey, next);
});

function searchableText(item) {
  return `${item.dataset.searchTitle || ''} ${item.textContent || ''}`.toLowerCase();
}

function renderResults(query = '') {
  if (!searchResults) return;
  const normalized = query.trim().toLowerCase();
  const matches = searchItems
    .filter((item) => !normalized || searchableText(item).includes(normalized))
    .slice(0, 8);

  searchResults.replaceChildren();
  if (!matches.length) {
    const empty = document.createElement('p');
    empty.className = 'search-empty';
    empty.textContent = `No documented surface matches “${query.trim()}”.`;
    searchResults.append(empty);
    return;
  }

  for (const item of matches) {
    const link = document.createElement('a');
    link.href = `#${item.id}`;
    link.className = 'search-result';
    const title = document.createElement('strong');
    title.textContent = item.dataset.searchTitle || item.querySelector('h2, h3')?.textContent || item.id;
    const context = document.createElement('span');
    context.textContent = item.dataset.searchContext || 'CodeFlow documentation';
    link.append(title, context);
    link.addEventListener('click', () => searchDialog.close());
    searchResults.append(link);
  }
}

function openSearch() {
  if (!searchDialog) return;
  renderResults('');
  searchDialog.showModal();
  searchInput?.focus();
}

document.querySelectorAll('[data-open-search]').forEach((button) => {
  button.addEventListener('click', openSearch);
});

searchInput?.addEventListener('input', (event) => renderResults(event.currentTarget.value));
searchDialog?.querySelector('[data-close-search]')?.addEventListener('click', () => searchDialog.close());
searchDialog?.addEventListener('click', (event) => {
  if (event.target === searchDialog) searchDialog.close();
});

document.addEventListener('keydown', (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
    event.preventDefault();
    openSearch();
  }
});

const navigation = document.querySelector('[data-site-navigation]');
const navigationButton = document.querySelector('[data-navigation-toggle]');
navigationButton?.addEventListener('click', () => {
  const open = navigation?.toggleAttribute('data-open');
  navigationButton.setAttribute('aria-expanded', String(Boolean(open)));
});

document.querySelectorAll('[data-level-target]').forEach((button) => {
  button.addEventListener('click', () => {
    const target = button.dataset.levelTarget;
    document.querySelectorAll('[data-level-target]').forEach((candidate) => {
      candidate.setAttribute('aria-pressed', String(candidate === button));
    });
    document.querySelectorAll('[data-level]').forEach((panel) => {
      panel.hidden = panel.dataset.level !== target;
    });
  });
});

document.querySelectorAll('[data-inspect-target]').forEach((button) => {
  button.addEventListener('click', () => {
    const target = button.dataset.inspectTarget;
    document.querySelectorAll('[data-inspect-target]').forEach((candidate) => {
      candidate.setAttribute('aria-pressed', String(candidate === button));
    });
    document.querySelectorAll('[data-inspector]').forEach((panel) => {
      panel.hidden = panel.dataset.inspector !== target;
    });
  });
});
