const selector = ".portal-id-preview";
const touchLinks = new WeakSet();

document.addEventListener("touchstart", (event) => {
  const link = event.target?.closest?.(`${selector} > a`);
  if (link) touchLinks.add(link);
}, { passive: true });

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  const preview = document.activeElement?.closest?.(selector);
  if (!preview) return;
  preview.classList.remove("portal-preview-open");
  preview.classList.add("portal-preview-dismissed");
});

document.addEventListener("focusout", (event) => {
  const preview = event.target?.closest?.(selector);
  if (preview && !preview.contains(event.relatedTarget)) preview.classList.remove("portal-preview-open", "portal-preview-dismissed");
});

document.addEventListener("click", (event) => {
  const link = event.target?.closest?.(`${selector} > a`);
  const touched = link ? touchLinks.delete(link) : false;
  const coarsePointerClick = event.detail > 0 && !event.pointerType && matchMedia("(hover: none) and (pointer: coarse)").matches;
  if (!link || (!["touch", "pen"].includes(event.pointerType) && !coarsePointerClick && !touched)) return;
  const preview = link.closest(selector);
  if (preview.classList.contains("portal-preview-open")) return;
  event.preventDefault();
  document.querySelectorAll(`${selector}.portal-preview-open`).forEach((item) => item.classList.remove("portal-preview-open"));
  preview.classList.remove("portal-preview-dismissed");
  preview.classList.add("portal-preview-open");
});

document.addEventListener("pointerdown", (event) => {
  if (event.target?.closest?.(selector)) return;
  document.querySelectorAll(`${selector}.portal-preview-open`).forEach((item) => item.classList.remove("portal-preview-open"));
});
