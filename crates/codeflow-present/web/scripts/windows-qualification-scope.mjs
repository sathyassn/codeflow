import { win32 } from "node:path";

// This proves confinement at harness start. The outer native-Windows lane owns
// creation and teardown of the disposable account/profile or whole VM and must
// retain separate teardown evidence.
export function validateWindowsQualificationConfinement(scope) {
  const required = [
    "disposableProfileRoot", "userProfile", "localAppData", "appData", "temp", "tmp",
    "knownLocalAppData",
  ];
  for (const name of required) {
    if (typeof scope[name] !== "string" || scope[name].length === 0 || !win32.isAbsolute(scope[name])) {
      throw new Error(`Windows qualification requires an absolute ${name}`);
    }
  }
  const root = normalized(scope.disposableProfileRoot);
  if (normalized(scope.userProfile) !== root) {
    throw new Error("Windows qualification requires USERPROFILE to equal the disposable profile root");
  }
  if (normalized(scope.localAppData) !== normalized(scope.knownLocalAppData)) {
    throw new Error("Windows qualification requires LOCALAPPDATA to match the actual Known Folder");
  }
  for (const name of ["localAppData", "appData", "temp", "tmp", "knownLocalAppData"]) {
    if (!isWithin(root, normalized(scope[name]))) {
      throw new Error(`Windows qualification ${name} escapes the disposable profile root`);
    }
  }
  return {
    externally_provisioned_profile_root: win32.resolve(scope.disposableProfileRoot),
    actual_known_local_app_data: win32.resolve(scope.knownLocalAppData),
    proof_scope: "start-time profile confinement only; outer lane must prove profile or VM teardown",
  };
}

function normalized(path) {
  return win32.resolve(path).toLocaleLowerCase("en-US");
}

function isWithin(root, candidate) {
  const relative = win32.relative(root, candidate);
  return relative === "" || (!relative.startsWith("..\\") && relative !== ".." && !win32.isAbsolute(relative));
}
