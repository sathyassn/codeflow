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

const themeToggle = document.querySelector(".theme-toggle");

function updateThemeControl(nextTheme) {
  const dark = nextTheme === "dark";
  themeToggle.setAttribute("aria-pressed", String(dark));
  themeToggle.querySelector(".theme-state").textContent = dark
    ? "Dark mode active; switch to light mode"
    : "Light mode active; switch to dark mode";
  themeToggle.title = dark ? "Use light mode" : "Use dark mode";
}

updateThemeControl(theme);

document.querySelectorAll(".flow-figure, .table-scroll, pre").forEach((region) => {
  region.tabIndex = 0;
  if (!region.getAttribute("aria-label")) {
    region.setAttribute("aria-label", "Scrollable presentation content");
  }
});

themeToggle.addEventListener("click", () => {
  const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  localStorage.setItem("cf-present-theme", next);
  updateLinks(next);
  updateThemeControl(next);
  history.replaceState(null, "", `?variant=${variant}&theme=${next}`);
});

const route = document.querySelector("[data-section-route]");
if (route && variant === "workbench") {
  const links = [...route.querySelectorAll("a[href^='#']")];
  const sections = links.map((link) => document.querySelector(link.hash)).filter(Boolean);
  let routeFrame;

  const selectRoute = (id) => {
    links.forEach((link) => {
      if (link.hash === `#${id}`) link.setAttribute("aria-current", "location");
      else link.removeAttribute("aria-current");
    });
  };

  const updateRoute = () => {
    routeFrame = undefined;
    const threshold = Math.min(innerHeight * 0.35, 220);
    const current = sections.reduce((nearest, section) => {
      const distance = Math.abs(section.getBoundingClientRect().top - threshold);
      return distance < nearest.distance ? { section, distance } : nearest;
    }, { section: sections[0], distance: Number.POSITIVE_INFINITY }).section;
    if (current) selectRoute(current.id);
  };

  const requestRouteUpdate = () => {
    if (!routeFrame) routeFrame = requestAnimationFrame(updateRoute);
  };

  links.forEach((link) => link.addEventListener("click", () => selectRoute(link.hash.slice(1))));
  addEventListener("scroll", requestRouteUpdate, { passive: true });
  addEventListener("resize", requestRouteUpdate);
  updateRoute();
}

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

const inspectorButtons = document.querySelectorAll("[data-inspect-node]");
inspectorButtons.forEach((button) => {
  button.addEventListener("click", () => {
    const node = button.closest("[data-node]");
    inspectorButtons.forEach((candidate) => {
      candidate.setAttribute("aria-pressed", String(candidate === button));
    });
    document.querySelector(".inspector-title").textContent = node.querySelector("h2").textContent;
    document.querySelector(".inspector-copy").textContent = node.dataset.inspectorCopy;
    const target = document.querySelector(".map .feedback-target");
    if (target) target.textContent = `${node.querySelector("h2").textContent} node`;
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
