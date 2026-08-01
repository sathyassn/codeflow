import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputRoot = resolve(webRoot, "../assets/supply-chain");
await mkdir(outputRoot, { recursive: true });

const audit = runJson("npm", ["audit", "--omit=dev", "--json"]);
await writeJson("npm-audit.json", audit);

const sbom = runJson("npm", ["sbom", "--omit=dev", "--sbom-format=cyclonedx"]);
delete sbom.serialNumber;
if (sbom.metadata && typeof sbom.metadata === "object") delete sbom.metadata.timestamp;
await writeJson("npm-sbom.cdx.json", sbom);

const lock = JSON.parse(await readFile(join(webRoot, "package-lock.json"), "utf8"));
const allowedLicenses = new Set([
  "Apache-2.0",
  "BSD-3-Clause",
  "ISC",
  "MIT",
  "MIT (verified license file)",
  "(MPL-2.0 OR Apache-2.0)",
  "Unlicense",
]);
const packages = [];
for (const [path, locked] of Object.entries(lock.packages ?? {})) {
  if (!path || locked.dev || !path.startsWith("node_modules/")) continue;
  const name = locked.name ?? path.slice("node_modules/".length);
  const packageJsonPath = join(webRoot, path, "package.json");
  const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8"));
  const license = normalizeLicense(packageJson.license ?? packageJson.licenses);
  const record = { name, version: locked.version, license };
  if (name === "khroma" && locked.version === "2.1.0" && license === "UNKNOWN") {
    const licenseText = await readFile(join(webRoot, path, "LICENSE"));
    const hash = createHash("sha256").update(licenseText).digest("hex");
    if (hash !== "66b333b0f66759a0b710459e03f7029abe17f4358114a128d2c972e642961b49") {
      throw new Error("khroma 2.1.0 license evidence changed");
    }
    record.license = "MIT (verified license file)";
    record.license_file_sha256 = hash;
  }
  if (record.license === "UNKNOWN") throw new Error(`Missing license metadata for ${name}@${locked.version}`);
  if (!allowedLicenses.has(record.license)) {
    throw new Error(`Unreviewed production license ${record.license} for ${name}@${locked.version}`);
  }
  packages.push(record);
}
packages.sort((left, right) => `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`));
await writeJson("licenses.json", { schema_version: 1, packages });
process.stdout.write(`cf-present supply chain: ${packages.length} packages, ${audit.metadata?.vulnerabilities?.total ?? "unknown"} vulnerabilities\n`);

function normalizeLicense(value) {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (Array.isArray(value)) return value.map((item) => item.type).filter(Boolean).join(" OR ") || "UNKNOWN";
  return "UNKNOWN";
}

function runJson(command, args) {
  const result = spawnSync(command, args, { cwd: webRoot, encoding: "utf8", stdio: "pipe" });
  if (result.status !== 0) throw new Error(`${command} failed\n${result.stdout}\n${result.stderr}`);
  return JSON.parse(result.stdout);
}

async function writeJson(name, value) {
  await writeFile(join(outputRoot, name), `${JSON.stringify(value, null, 2)}\n`);
}
