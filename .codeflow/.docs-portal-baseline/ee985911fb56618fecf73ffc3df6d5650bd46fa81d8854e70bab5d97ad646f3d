import { lstat, readFile } from "node:fs/promises";
import path from "node:path";
import { assertNoSymlink, collectBuiltArtifacts, withPublicationLease, writeText } from "./lib.mjs";

const root = process.cwd();
await withPublicationLease(root, async ({ refresh }) => {
  const evidencePath = path.join(root, ".portal/generated/evidence.json");
  await assertNoSymlink(root, ".portal/generated/evidence.json");
  const metadata = await lstat(evidencePath);
  if (!metadata.isFile() || metadata.size > 8 * 1024 * 1024) throw new Error("portal evidence is not a regular file or exceeds 8388608 bytes");
  const evidence = JSON.parse(await readFile(evidencePath, "utf8"));
  if (!evidence || typeof evidence !== "object" || Array.isArray(evidence) || evidence.schema_version !== 1 || !evidence.generator || !evidence.repository || !Array.isArray(evidence.pages) || !evidence.llms || !Array.isArray(evidence.artifacts)) {
    throw new Error("portal evidence has an invalid envelope");
  }
  const dist = path.join(root, "dist");
  const artifacts = await collectBuiltArtifacts(dist, { onProgress: refresh });
  await refresh();
  await assertNoSymlink(root, ".portal/generated/evidence.json");
  evidence.artifacts = artifacts;
  await writeText(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(`portal: recorded ${artifacts.length} built artifact(s)`);
});
