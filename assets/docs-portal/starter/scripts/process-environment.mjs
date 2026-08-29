const INHERITED_KEYS = [
  "PATH",
  "Path",
  "SystemRoot",
  "WINDIR",
  "COMSPEC",
  "PATHEXT",
  "HOME",
  "USERPROFILE",
  "TMPDIR",
  "TMP",
  "TEMP",
  "XDG_RUNTIME_DIR",
  "LANG",
  "LC_ALL",
  "LC_CTYPE",
  "TZ",
  "TERM",
  "COLORTERM",
  "NO_COLOR",
  "FORCE_COLOR",
];

const OVERRIDE_KEYS = new Set(["BROWSER"]);

/**
 * Return the small, non-credential environment inherited by portal children.
 * Provider, cloud, package-registry, loader-injection, and unrelated project
 * variables are absent because this is an allowlist rather than a blacklist.
 */
export function hardenedChildEnvironment(source = process.env, overrides = {}) {
  const executablePath = source.PATH ?? source.Path;
  if (typeof executablePath !== "string" || executablePath.length === 0) {
    throw new Error("portal child process requires PATH");
  }

  const environment = { PATH: executablePath };
  for (const key of INHERITED_KEYS) {
    if (key === "PATH" || key === "Path") continue;
    if (typeof source[key] === "string" && source[key].length > 0) environment[key] = source[key];
  }
  for (const [key, value] of Object.entries(overrides)) {
    if (!OVERRIDE_KEYS.has(key)) throw new Error(`portal child environment override is not allowed: ${key}`);
    if (typeof value !== "string" || value.length === 0) throw new Error(`portal child environment override must be non-empty: ${key}`);
    environment[key] = value;
  }
  return environment;
}
