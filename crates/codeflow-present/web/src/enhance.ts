const observed = new WeakSet<Element>();
const MAX_EAGER_ENHANCEMENT_MILLISECONDS = 5_000;

export function enhanceDocument(root: HTMLElement, eager = false): () => void {
  const targets = [
    ...root.querySelectorAll<HTMLElement>("code[data-cf-language]"),
  ];
  if (eager || !("IntersectionObserver" in globalThis)) {
    void enhanceSequentially(targets, MAX_EAGER_ENHANCEMENT_MILLISECONDS);
    return () => undefined;
  }
  let queue = Promise.resolve();
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        observer.unobserve(entry.target);
        queue = queue.then(() => enhance(entry.target as HTMLElement));
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

async function enhanceSequentially(targets: HTMLElement[], budgetMilliseconds: number): Promise<void> {
  const started = performance.now();
  for (const target of targets) {
    if (performance.now() - started > budgetMilliseconds) return;
    await enhance(target);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }
}

async function enhance(target: HTMLElement): Promise<void> {
  if (target.matches("code[data-cf-language]")) {
    const { highlightCode } = await import("./syntax");
    await highlightCode(target, target.dataset.cfLanguage ?? "");
  }
}
