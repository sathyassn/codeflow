import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const limitsSource = await readFile(join(repoRoot, "crates/codeflow-present/src/limits.rs"), "utf8");
const serviceLimit = rustLimit("MAX_SERVICE_BINARY_DELTA_BYTES");
const combinedLimit = rustLimit("MAX_COMBINED_BINARY_DELTA_BYTES");
const runId = `cf-present-binary-delta-${process.pid}-${randomUUID()}`;
const sandbox = await mkdtemp(join(tmpdir(), `${runId}-`));
const marker = join(sandbox, ".owned-binary-delta-root");
const sourceRoot = join(sandbox, "source");
const targetRoot = join(repoRoot, "target", "cf-present-binary-delta");
const savedAssets = join(sandbox, "production-assets");
const childEnvironment = allowedEnvironment([
  "CARGO_HOME", "HOME", "LANG", "LC_ALL", "PATH", "RUSTUP_HOME", "SYSTEMROOT",
  "TEMP", "TMP", "TMPDIR", "USERPROFILE", "WINDIR",
]);
childEnvironment.CARGO_INCREMENTAL = "0";
childEnvironment.CARGO_TARGET_DIR = targetRoot;
childEnvironment.SOURCE_DATE_EPOCH = "0";

try {
  await writeFile(marker, `${runId}\n`, { mode: 0o600 });
  await mkdir(sourceRoot);
  await Promise.all([
    cp(join(repoRoot, "Cargo.toml"), join(sourceRoot, "Cargo.toml")),
    cp(join(repoRoot, "Cargo.lock"), join(sourceRoot, "Cargo.lock")),
    cp(join(repoRoot, "assets"), join(sourceRoot, "assets"), { recursive: true }),
    cp(join(repoRoot, "crates"), join(sourceRoot, "crates"), {
      recursive: true,
      filter: (source) => !excludedFromSourceCopy(source),
    }),
    cp(join(repoRoot, "crates/codeflow-present/assets"), savedAssets, { recursive: true }),
  ]);

  const production = await buildAndMeasure("production");
  await rm(join(sourceRoot, "crates/codeflow-present/assets/export"), { recursive: true });
  cleanOwnedPackages();
  const serviceOnly = await buildAndMeasure("service-only");

  await rm(join(sourceRoot, "crates/codeflow-present/assets/service"), { recursive: true });
  cleanOwnedPackages();
  const manifestBaseline = await buildAndMeasure("manifest-baseline");

  await rm(join(sourceRoot, "crates/codeflow-present/assets"), { recursive: true });
  await cp(savedAssets, join(sourceRoot, "crates/codeflow-present/assets"), { recursive: true });
  cleanOwnedPackages();
  const productionRepeat = await buildAndMeasure("production-repeat");
  if (production.sha256 !== productionRepeat.sha256) {
    throw new Error(
      `Two exact production release builds differed: ${production.sha256} != ${productionRepeat.sha256}`,
    );
  }

  const serviceDelta = serviceOnly.bytes - manifestBaseline.bytes;
  const combinedDelta = production.bytes - manifestBaseline.bytes;
  assertDelta("service payload", serviceDelta, serviceLimit);
  assertDelta("combined service/export payload", combinedDelta, combinedLimit);
  process.stdout.write(`${JSON.stringify({
    schema_version: 1,
    platform: `${process.platform}-${process.arch}`,
    release_profile: "cargo build --release --locked -p codeflow-cli",
    baseline: "same exact source and manifest with generated service/export payloads absent",
    production_bytes: production.bytes,
    production_sha256: production.sha256,
    service_binary_delta_bytes: serviceDelta,
    service_binary_delta_limit_bytes: serviceLimit,
    combined_binary_delta_bytes: combinedDelta,
    combined_binary_delta_limit_bytes: combinedLimit,
    exact_repeat: "pass",
  }, null, 2)}\n`);
} finally {
  const owned = await readFile(marker, "utf8").catch(() => "");
  if (owned !== `${runId}\n`) throw new Error(`Refusing to clean unowned binary-delta root ${sandbox}`);
  await rm(sandbox, { recursive: true });
}

function rustLimit(name) {
  const match = limitsSource.match(new RegExp(`pub const ${name}: u64 = ([0-9_]+);`, "u"));
  if (!match) throw new Error(`Could not read ${name} from limits.rs`);
  return Number(match[1].replaceAll("_", ""));
}

function excludedFromSourceCopy(source) {
  const relative = source.slice(join(repoRoot, "crates").length).split(sep).filter(Boolean);
  return relative.includes("node_modules") || relative.includes("target") || relative.includes(".DS_Store");
}

async function buildAndMeasure(label) {
  runCargo(["build", "--release", "--locked", "-p", "codeflow-cli"]);
  const executable = join(targetRoot, "release", process.platform === "win32" ? "codeflow.exe" : "codeflow");
  const bytes = (await stat(executable)).size;
  const sha256 = createHash("sha256").update(await readFile(executable)).digest("hex");
  if (!bytes) throw new Error(`${label} release binary was empty`);
  return { bytes, sha256 };
}

function cleanOwnedPackages() {
  runCargo(["clean", "-p", "codeflow-cli", "-p", "codeflow-present"]);
}

function runCargo(args) {
  execFileSync("cargo", args, {
    cwd: sourceRoot,
    env: childEnvironment,
    encoding: "utf8",
    stdio: ["ignore", "ignore", "inherit"],
    timeout: 600_000,
  });
}

function assertDelta(label, actual, limit) {
  if (actual < 0) throw new Error(`${label} binary delta was negative: ${actual} B`);
  if (actual > limit) throw new Error(`${label} binary delta ${actual} B exceeds ${limit} B`);
}

function allowedEnvironment(names) {
  return Object.fromEntries(names.flatMap((name) => (
    process.env[name] === undefined ? [] : [[name, process.env[name]]]
  )));
}
