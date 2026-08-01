const observed = new WeakSet<Element>();

export function enhanceDocument(root: HTMLElement, eager = false): () => void {
  const targets = [
    ...root.querySelectorAll<HTMLElement>("code[data-cf-language]"),
    ...root.querySelectorAll<HTMLElement>("[data-cf-diagram='pending']"),
  ];
  if (eager || !("IntersectionObserver" in globalThis)) {
    targets.forEach((target) => void enhance(target));
    return () => undefined;
  }
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        observer.unobserve(entry.target);
        void enhance(entry.target as HTMLElement);
      });
    },
    { rootMargin: "240px" },
  );
  targets.forEach((target) => {
    if (observed.has(target)) return;
    observed.add(target);
    observer.observe(target);
  });
  return () => observer.disconnect();
}

async function enhance(target: HTMLElement): Promise<void> {
  if (target.matches("code[data-cf-language]")) {
    const { highlightCode } = await import("./syntax");
    await highlightCode(target, target.dataset.cfLanguage ?? "");
    return;
  }
  if (target.matches("[data-cf-diagram='pending']")) {
    const { renderDiagram } = await import("./diagram");
    await renderDiagram(target);
  }
}
