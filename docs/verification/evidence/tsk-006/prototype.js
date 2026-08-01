const params = new URLSearchParams(location.search);
const variants = new Set(["briefing", "workbench", "map"]);
const variant = variants.has(params.get("variant")) ? params.get("variant") : "briefing";
const theme = document.documentElement.dataset.theme;

document.documentElement.dataset.variant = variant;
document.documentElement.dataset.theme = theme;

function updateLinks(nextTheme) {
  document.querySelectorAll("[data-variant-link]").forEach((link) => {
    const linkVariant = link.dataset.variantLink;
    link.href = `?variant=${linkVariant}&theme=${nextTheme}`;
    if (linkVariant === variant) link.setAttribute("aria-current", "page");
  });
}

updateLinks(theme);

document.querySelectorAll(".flow-figure, .table-scroll, pre").forEach((region) => {
  region.tabIndex = 0;
  if (!region.getAttribute("aria-label")) {
    region.setAttribute("aria-label", "Scrollable presentation content");
  }
});

document.querySelector(".theme-toggle").addEventListener("click", () => {
  const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  localStorage.setItem("cf-present-theme", next);
  updateLinks(next);
  history.replaceState(null, "", `?variant=${variant}&theme=${next}`);
});

const targets = document.querySelectorAll(".feedback-target");
document.querySelectorAll("[data-annotate]").forEach((button) => {
  button.addEventListener("click", () => {
    const label = button.dataset.annotate;
    targets.forEach((target) => { target.textContent = label; });
    const area = document.querySelector(`.${variant} textarea`);
    if (area) {
      area.placeholder = `Feedback on ${label}`;
      area.focus();
    }
  });
});

document.querySelectorAll("[data-node]").forEach((node) => {
  node.addEventListener("click", (event) => {
    if (event.target.closest("button")) return;
    document.querySelector(".inspector-title").textContent = node.querySelector("h2").textContent;
    document.querySelector(".inspector-copy").textContent =
      node.querySelector("p:last-child")?.textContent || "Inspect the evidence attached to this decision.";
  });
});

document.querySelectorAll("[data-send-feedback]").forEach((button) => {
  button.addEventListener("click", () => {
    const toast = document.querySelector(".feedback-toast");
    toast.classList.add("visible");
    document.querySelector("#feedback-count").textContent = "2 open notes";
    setTimeout(() => toast.classList.remove("visible"), 1700);
  });
});
