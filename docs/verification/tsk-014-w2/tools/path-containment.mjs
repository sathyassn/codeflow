// Platform-correct path containment, kept pure so it can be unit-tested for
// hosts this study cannot execute on.
//
// `flavor` is a node:path flavor (path.posix or path.win32). Callers pass
// already-canonical (realpath-resolved) absolute paths; this module performs no
// I/O and makes no claim about symlinks — resolving those is the caller's job.

export function isContained(root, target, flavor) {
  if (typeof root !== "string" || typeof target !== "string" || !root || !target) return false;
  if (!flavor.isAbsolute(root) || !flavor.isAbsolute(target)) return false;

  const normalisedRoot = stripTrailing(flavor.normalize(root), flavor);
  const normalisedTarget = stripTrailing(flavor.normalize(target), flavor);

  // Windows paths are case-insensitive and mix separators; the root of one
  // volume never contains a path on another, and a UNC share root is not the
  // same as a drive root.
  const relative = flavor.relative(normalisedRoot, normalisedTarget);
  if (relative === "") return true;
  if (flavor.isAbsolute(relative)) return false;           // different volume or share
  const first = relative.split(/[\\/]/u)[0];
  return first !== "..";
}

function stripTrailing(value, flavor) {
  const sep = flavor.sep;
  if (value.length > 1 && value.endsWith(sep) && !value.endsWith(`:${sep}`)) return value.slice(0, -1);
  return value;
}

// A small, explicit table of cases this helper must satisfy on every host,
// including the Windows semantics this study's live runs cannot exercise.
export const CONTAINMENT_CASES = [
  { flavor: "posix", root: "/a/study", target: "/a/study", contained: true, why: "the root itself" },
  { flavor: "posix", root: "/a/study", target: "/a/study/x/y.png", contained: true, why: "descendant" },
  { flavor: "posix", root: "/a/study/", target: "/a/study/x", contained: true, why: "trailing separator on root" },
  { flavor: "posix", root: "/a/study", target: "/a/study-other/x", contained: false, why: "sibling prefix" },
  { flavor: "posix", root: "/a/study", target: "/a/other", contained: false, why: "sibling" },
  { flavor: "posix", root: "/a/study", target: "/a", contained: false, why: "ancestor" },
  { flavor: "posix", root: "/a/study", target: "/etc/hosts", contained: false, why: "unrelated absolute" },
  { flavor: "posix", root: "/a/study", target: "x/y", contained: false, why: "relative target rejected" },
  { flavor: "win32", root: "C:\\a\\study", target: "C:\\a\\study\\x.png", contained: true, why: "drive descendant" },
  { flavor: "win32", root: "C:\\a\\study", target: "C:/a/study/x.png", contained: true, why: "mixed separators" },
  { flavor: "win32", root: "C:\\a\\study", target: "c:\\a\\study\\x.png", contained: true, why: "case-insensitive drive" },
  { flavor: "win32", root: "C:\\a\\study", target: "C:\\a\\study-other\\x", contained: false, why: "sibling prefix" },
  { flavor: "win32", root: "C:\\a\\study", target: "D:\\a\\study\\x", contained: false, why: "different volume" },
  { flavor: "win32", root: "C:\\a\\study", target: "\\\\server\\share\\x", contained: false, why: "UNC path is not on the drive" },
  { flavor: "win32", root: "\\\\server\\share\\study", target: "\\\\server\\share\\study\\x", contained: true, why: "UNC descendant" },
  { flavor: "win32", root: "\\\\server\\share\\study", target: "\\\\server\\other\\study\\x", contained: false, why: "different share" },
];
