// One control per container; nested tabs retain their own event owner.
const enhanced = new WeakSet<Element>();

export function enhanceTabs(root: HTMLElement): void {
  for (const container of root.querySelectorAll<HTMLElement>("[data-cf-tabs]")) {
    if (enhanced.has(container)) continue;
    enhanced.add(container);
    const tabs = [...container.querySelectorAll<HTMLButtonElement>("[role='tab']")]
      .filter(tab => tab.closest("[data-cf-tabs]") === container);
    const activate = (tab: HTMLButtonElement, focus: boolean): void => {
      for (const item of tabs) {
        const selected = item === tab;
        item.setAttribute("aria-selected", String(selected));
        item.tabIndex = selected ? 0 : -1;
        const panel = document.getElementById(item.getAttribute("aria-controls") ?? "");
        if (panel) panel.hidden = !selected;
      }
      if (focus) tab.focus();
    };
    for (const tab of tabs) {
      tab.addEventListener("click", event => {
        if (root.dataset.cfCommenting === "true") {
          event.preventDefault();
          return;
        }
        activate(tab, false);
      });
      tab.addEventListener("keydown", event => {
        if (event.defaultPrevented || root.dataset.cfCommenting === "true") return;
        let index = tabs.indexOf(tab);
        if (event.key === "ArrowRight") index = (index + 1) % tabs.length;
        else if (event.key === "ArrowLeft") index = (index + tabs.length - 1) % tabs.length;
        else if (event.key === "Home") index = 0;
        else if (event.key === "End") index = tabs.length - 1;
        else return;
        event.preventDefault();
        activate(tabs[index]!, true);
      });
    }
  }
}
