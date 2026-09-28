import { brotliCompressSync, constants as zlibConstants, gzipSync } from "node:zlib";
import { createHash } from "node:crypto";
import { cp, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const crateRoot = resolve(webRoot, "..");
const defaultAssetsRoot = join(crateRoot, "assets");
const expectedNode = "v26.4.0";
const expectedNpm = "11.17.0";
// Official Node builds use these compressors. Distribution builds can keep the
// Node version while changing gzip bytes; the full tree check remains the proof.
const expectedCompression = Object.freeze({ zlib: "1.3.2.1-motley-3246f1b", brotli: "1.2.0" });

// Each budget is the measurement taken when TSK-114 removed the diagram
// renderer from the 3.0.0 source, plus 10% and rounded up to the next
// 5,000 B, so the room it freed cannot return without a reviewed budget change.
export const budgets = Object.freeze({
  raw_corpus_bytes: 1_060_000, // measured 961,762 B
  largest_raw_chunk_bytes: 205_000, // measured 186,275 B
  brotli_corpus_bytes: 280_000, // measured 251,598 B
  largest_brotli_chunk_bytes: 145_000, // measured 131,206 B
  export_gzip_bytes: 280_000, // measured 251,370 B
});

export async function buildAssets(assetsRoot = defaultAssetsRoot) {
  assertToolchain();
  const scratch = await mkdtemp(join(tmpdir(), "cf-present-web-"));
  try {
    const rawService = join(scratch, "service");
    await mkdir(rawService, { recursive: true });
    const serviceResult = await build({
      absWorkingDir: webRoot,
      entryPoints: { app: "src/main.tsx" },
      outdir: rawService,
      bundle: true,
      splitting: true,
      format: "esm",
      platform: "browser",
      target: ["chrome120", "firefox121", "safari17"],
      entryNames: "[name]-[hash]",
      chunkNames: "chunk-[name]-[hash]",
      assetNames: "asset-[name]-[hash]",
      loader: { ".woff2": "dataurl", ".LICENSE": "text" },
      jsx: "automatic",
      jsxImportSource: "preact",
      minify: true,
      treeShaking: true,
      sourcemap: false,
      legalComments: "none",
      charset: "utf8",
      metafile: true,
      define: { "process.env.NODE_ENV": '"production"' },
    });

    const prepaintResult = await build({
      absWorkingDir: webRoot,
      entryPoints: ["src/prepaint.ts"],
      bundle: true,
      write: false,
      format: "iife",
      platform: "browser",
      target: ["chrome120", "firefox121", "safari17"],
      minify: true,
      legalComments: "none",
      charset: "utf8",
    });
    const prepaint = new TextDecoder().decode(prepaintResult.outputFiles[0]?.contents).trim();

    const rawExport = join(scratch, "export-renderer.js");
    await build({
      absWorkingDir: webRoot,
      entryPoints: ["src/export.ts"],
      outfile: rawExport,
      bundle: true,
      splitting: false,
      format: "esm",
      platform: "browser",
      target: ["chrome120", "firefox121", "safari17"],
      jsx: "automatic",
      jsxImportSource: "preact",
      loader: { ".css": "text", ".woff2": "dataurl", ".LICENSE": "text" },
      minify: true,
      treeShaking: true,
      sourcemap: false,
      legalComments: "none",
      charset: "utf8",
      define: { "process.env.NODE_ENV": '"production"' },
    });

    const staging = join(scratch, "assets");
    const serviceDirectory = join(staging, "service");
    const exportDirectory = join(staging, "export");
    await mkdir(serviceDirectory, { recursive: true });
    await mkdir(exportDirectory, { recursive: true });

    const serviceFiles = await listFiles(rawService);
    const metadata = normalizeMetadata(serviceResult.metafile, rawService);
    const serviceAssets = [];
    let rawCorpusBytes = 0;
    let brotliCorpusBytes = 0;
    let largestRawChunkBytes = 0;
    let largestBrotliChunkBytes = 0;
    for (const rawPath of serviceFiles) {
      const name = basename(rawPath);
      const raw = await readFile(rawPath);
      const encoded = brotliCompressSync(raw, {
        params: {
          [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
          [zlibConstants.BROTLI_PARAM_MODE]: zlibConstants.BROTLI_MODE_TEXT,
        },
      });
      const storedPath = `service/${name}.br`;
      await writeFile(join(staging, storedPath), encoded);
      rawCorpusBytes += raw.byteLength;
      brotliCorpusBytes += encoded.byteLength;
      largestRawChunkBytes = Math.max(largestRawChunkBytes, raw.byteLength);
      largestBrotliChunkBytes = Math.max(largestBrotliChunkBytes, encoded.byteLength);
      serviceAssets.push({
        request_path: `/app/assets/${name}`,
        stored_path: storedPath,
        media_type: mediaType(name),
        content_encoding: "br",
        raw_bytes: raw.byteLength,
        encoded_bytes: encoded.byteLength,
        sha256: sha256Hex(encoded),
        etag: `"sha256-${sha256Base64(encoded)}"`,
        imports: (metadata[name]?.imports ?? []).map((item) => ({
          request_path: `/app/assets/${item.path}`,
          kind: item.kind,
        })),
      });
    }
    serviceAssets.sort((left, right) => left.request_path.localeCompare(right.request_path));

    const exportRaw = await readFile(rawExport);
    const exportEncoded = gzipSync(exportRaw, { level: 9, mtime: 0 });
    const exportHash = sha256Hex(exportEncoded);
    const exportStoredPath = `export/renderer-${exportHash.slice(0, 16)}.js.gz`;
    await writeFile(join(staging, exportStoredPath), exportEncoded);

    const appAsset = serviceAssets.find((asset) => /^\/app\/assets\/app-[A-Z0-9]+\.js$/u.test(asset.request_path));
    const styleAsset = serviceAssets.find((asset) => /^\/app\/assets\/app-[A-Z0-9]+\.css$/u.test(asset.request_path));
    if (!appAsset || !styleAsset) throw new Error("esbuild did not produce canonical app and style entrypoints");

    enforceBudget("raw service corpus", rawCorpusBytes, budgets.raw_corpus_bytes);
    enforceBudget("largest raw service chunk", largestRawChunkBytes, budgets.largest_raw_chunk_bytes);
    enforceBudget("Brotli service corpus", brotliCorpusBytes, budgets.brotli_corpus_bytes);
    enforceBudget("largest Brotli service chunk", largestBrotliChunkBytes, budgets.largest_brotli_chunk_bytes);
    enforceBudget("gzip export renderer", exportEncoded.byteLength, budgets.export_gzip_bytes);

    const manifest = {
      schema_version: 1,
      toolchain: {
        node: expectedNode.slice(1),
        npm: expectedNpm,
        esbuild: "0.25.9",
        preact: "10.29.8",
        shiki: "4.4.1",
        targets: ["chrome120", "firefox121", "safari17"],
      },
      service: {
        entrypoints: {
          "present.app": appAsset.request_path,
          "present.style": styleAsset.request_path,
        },
        inline: {
          "present.prepaint": {
            source: prepaint,
            sha256: sha256Hex(prepaint),
            csp_sha256: `sha256-${sha256Base64(prepaint)}`,
          },
        },
        assets: serviceAssets,
        metrics: {
          asset_count: serviceAssets.length,
          raw_corpus_bytes: rawCorpusBytes,
          brotli_corpus_bytes: brotliCorpusBytes,
          largest_raw_chunk_bytes: largestRawChunkBytes,
          largest_brotli_chunk_bytes: largestBrotliChunkBytes,
        },
      },
      export: {
        "present.export": {
          stored_path: exportStoredPath,
          media_type: "text/javascript; charset=utf-8",
          content_encoding: "gzip",
          raw_bytes: exportRaw.byteLength,
          encoded_bytes: exportEncoded.byteLength,
          sha256: exportHash,
        },
      },
      budgets,
      corpus_sha256: corpusHash(serviceAssets, exportHash, prepaint),
    };
    await writeFile(join(staging, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

    await installBuiltAssets(staging, assetsRoot);
    return manifest;
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

async function installBuiltAssets(staging, assetsRoot) {
  const localStaging = `${assetsRoot}.stage-${process.pid}`;
  await rm(localStaging, { recursive: true, force: true });
  await cp(staging, localStaging, { recursive: true, force: true });
  await mkdir(assetsRoot, { recursive: true });
  try {
    for (const name of ["service", "export", "manifest.json"]) {
      const destination = join(assetsRoot, name);
      await rm(destination, { recursive: true, force: true });
      await rename(join(localStaging, name), destination);
    }
  } finally {
    await rm(localStaging, { recursive: true, force: true });
  }
}

function normalizeMetadata(metafile, outputRoot) {
  const normalized = {};
  for (const [outputPath, details] of Object.entries(metafile.outputs)) {
    const name = basename(outputPath);
    normalized[name] = {
      imports: details.imports
        .map((item) => ({ path: basename(item.path), kind: item.kind }))
        .sort((left, right) => left.path.localeCompare(right.path)),
    };
  }
  if (!Object.keys(normalized).length || !outputRoot) throw new Error("Empty esbuild metadata");
  return normalized;
}

async function listFiles(root) {
  const entries = await readdir(root, { withFileTypes: true });
  const paths = [];
  for (const entry of entries) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) paths.push(...(await listFiles(path)));
    else if (entry.isFile()) paths.push(path);
  }
  return paths.sort();
}

function mediaType(path) {
  if (path.endsWith(".js")) return "text/javascript; charset=utf-8";
  if (path.endsWith(".css")) return "text/css; charset=utf-8";
  throw new Error(`Unexpected service asset type: ${path}`);
}

function enforceBudget(label, actual, limit) {
  if (actual > limit) throw new Error(`${label} is ${actual} B; budget is ${limit} B`);
}

function corpusHash(assets, exportHash, prepaint) {
  const lines = assets.map((asset) => `${asset.request_path}\0${asset.sha256}`).sort();
  lines.push(`present.export\0${exportHash}`, `present.prepaint\0${sha256Hex(prepaint)}`);
  return sha256Hex(lines.join("\n"));
}

function sha256Hex(value) {
  return createHash("sha256").update(value).digest("hex");
}

function sha256Base64(value) {
  return createHash("sha256").update(value).digest("base64");
}

export function assertToolchain(versions = process.versions, userAgent = process.env.npm_config_user_agent) {
  if (`v${versions.node}` !== expectedNode) {
    throw new Error(`Node ${expectedNode.slice(1)} is required; found ${versions.node}`);
  }
  const npmVersion = userAgent?.match(/npm\/([^ ]+)/u)?.[1];
  if (npmVersion && npmVersion !== expectedNpm) {
    throw new Error(`npm ${expectedNpm} is required; found ${npmVersion}`);
  }
  for (const [name, expected] of Object.entries(expectedCompression)) {
    if (versions[name] !== expected) {
      throw new Error(`${name} ${expected} is required for reproducible assets; found ${versions[name] ?? "unavailable"}. Use the official Node ${expectedNode.slice(1)} distribution, not a system-library rebuild.`);
    }
  }
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  const manifest = await buildAssets();
  process.stdout.write(
    `cf-present assets: ${manifest.service.metrics.asset_count} service files, ` +
      `${manifest.service.metrics.brotli_corpus_bytes} B Brotli, ` +
      `${manifest.export["present.export"].encoded_bytes} B export gzip\n`,
  );
}
