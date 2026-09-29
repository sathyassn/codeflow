const observed = new WeakSet<Element>();
const MAX_EAGER_ENHANCEMENT_MILLISECONDS = 5_000;

export function enhanceDocument(root: HTMLElement, eager = false): () => void {
  const targets = [
    ...root.querySelectorAll<HTMLElement>("code[data-cf-language]"),
    ...root.querySelectorAll<HTMLElement>("[data-cf-figure-block='pending']"),
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
  for (const [index, target] of targets.entries()) {
    if (performance.now() - started > budgetMilliseconds) {
      for (const remaining of targets.slice(index)) markDeferredFailure(remaining);
      return;
    }
    await enhance(target);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }
}

function markDeferredFailure(target: HTMLElement): void {
  if (!target.matches("[data-cf-figure-block='pending']")) return;
  void import("./figure").then(({ markFigureFailure }) => markFigureFailure(target, "Figure drawing stopped at the local time budget."));
}

async function enhance(target: HTMLElement): Promise<void> {
  if (target.matches("code[data-cf-language]")) {
    const { highlightCode } = await import("./syntax");
    await highlightCode(target, target.dataset.cfLanguage ?? "");
    return;
  }
  if (target.matches("[data-cf-figure-block='pending']")) {
    const { renderFigureBlock } = await import("./figure");
    renderFigureBlock(target);
  }
}
