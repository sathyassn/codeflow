// Altitude tabs (utility presentation system): one visible panel per page,
// arrow-key tablist, hash-addressable panels. Without JavaScript the panels
// stack in document order; portal-chrome.js owns search shortcuts.
const containers = [];

function panelForHash(hash) {
  if (!hash || hash.length < 2) return null;
  let target;
  try {
    target = document.getElementById(decodeURIComponent(hash.slice(1)));
  } catch {
    return null;
  }
  return target ? target.closest(".portal-altitude") : null;
}

function initTabs(tablist) {
  const tabs = [...tablist.querySelectorAll('[role="tab"]')];
  const panels = tabs.map((tab) => document.getElementById(tab.getAttribute("aria-controls"))).filter(Boolean);
  if (tabs.length < 2 || panels.length !== tabs.length) return;
  const select = (index, { hash = true, focus = false } = {}) => {
    tabs.forEach((tab, i) => {
      tab.setAttribute("aria-selected", String(i === index));
      tab.tabIndex = i === index ? 0 : -1;
      panels[i].hidden = i !== index;
    });
    if (hash) history.replaceState(null, "", `#${tabs[index].dataset.anchor}`);
    if (focus) tabs[index].focus();
    document.dispatchEvent(new CustomEvent("cf:altitude"));
  };
  tabs.forEach((tab, index) => {
    tab.addEventListener("click", () => select(index));
    tab.addEventListener("keydown", (event) => {
      const move =
        event.key === "ArrowRight" || event.key === "ArrowDown" ? index + 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp" ? index - 1
        : event.key === "Home" ? 0
        : event.key === "End" ? tabs.length - 1
        : null;
      if (move === null) return;
      event.preventDefault();
      select((move + tabs.length) % tabs.length, { focus: true });
    });
  });
  const entry = { tabs, panels, select };
  containers.push(entry);
  const initial = panels.indexOf(panelForHash(location.hash));
  select(initial >= 0 ? initial : 0, { hash: false });
}

function revealHash() {
  const panel = panelForHash(location.hash);
  if (!panel) return;
  for (const entry of containers) {
    const index = entry.panels.indexOf(panel);
    if (index >= 0) {
      entry.select(index, { hash: false });
      let target = null;
      try {
        target = document.getElementById(decodeURIComponent(location.hash.slice(1)));
      } catch {
        /* undecodable hash: panel already selected */
      }
      if (target) target.scrollIntoView();
    }
  }
}

document.documentElement.dataset.cfpTabs = "on";
document.querySelectorAll(".portal-altitude-tabs").forEach(initTabs);
window.addEventListener("hashchange", revealHash);
if (containers.length > 0) revealHash();
