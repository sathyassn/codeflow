import { validateWindowsQualificationConfinement } from "./windows-qualification-scope.mjs";

const valid = {
  disposableProfileRoot: "C:\\Users\\codeflow-task",
  userProfile: "c:\\users\\CODEFLOW-TASK",
  localAppData: "C:\\Users\\codeflow-task\\AppData\\Local",
  knownLocalAppData: "c:\\users\\codeflow-task\\AppData\\Local",
  appData: "C:\\Users\\codeflow-task\\AppData\\Roaming",
  temp: "C:\\Users\\codeflow-task\\AppData\\Local\\Temp",
  tmp: "C:\\Users\\codeflow-task\\AppData\\Local\\Temp",
};
validateWindowsQualificationConfinement(valid);

for (const [label, changed] of [
  ["ordinary operator profile without explicit marker", {
    disposableProfileRoot: undefined,
    userProfile: "C:\\Users\\operator",
    localAppData: "C:\\Users\\operator\\AppData\\Local",
    knownLocalAppData: "C:\\Users\\operator\\AppData\\Local",
    appData: "C:\\Users\\operator\\AppData\\Roaming",
    temp: "C:\\Users\\operator\\AppData\\Local\\Temp",
    tmp: "C:\\Users\\operator\\AppData\\Local\\Temp",
  }],
  ["foreign user profile", { userProfile: "C:\\Users\\operator" }],
  ["Known Folder mismatch", { knownLocalAppData: "C:\\Users\\operator\\AppData\\Local" }],
  ["environment mismatch", { localAppData: "C:\\Users\\codeflow-task\\synthetic-local" }],
  ["foreign temp", { temp: "D:\\Temp" }],
]) {
  try {
    validateWindowsQualificationConfinement({ ...valid, ...changed });
    throw new Error(`${label} unexpectedly passed`);
  } catch (error) {
    if (error.message === `${label} unexpectedly passed`) throw error;
  }
}

process.stdout.write("Windows externally provisioned profile-confinement guard passed\n");
