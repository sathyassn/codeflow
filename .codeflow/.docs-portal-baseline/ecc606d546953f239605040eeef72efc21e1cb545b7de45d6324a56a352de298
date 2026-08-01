import path from "node:path";
import { assertNoSymlink, collectBuiltArtifacts, readBoundedRegularFile, withPublicationLease, writeText } from "./publication.mjs";

const root = process.cwd();
await withPublicationLease(root, async ({ refresh }) => {
  const evidencePath = path.join(root, ".portal/generated/evidence.json");
  await assertNoSymlink(root, ".portal/generated/evidence.json");
  const evidence = JSON.parse(await readBoundedRegularFile(evidencePath, 8 * 1024 * 1024, "portal evidence"));
  validateEvidenceEnvelope(evidence);
  const dist = path.join(root, "dist");
  const artifacts = await collectBuiltArtifacts(dist, { onProgress: refresh });
  await refresh();
  await assertNoSymlink(root, ".portal/generated/evidence.json");
  evidence.artifacts = artifacts;
  await writeText(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(`portal: recorded ${artifacts.length} built artifact(s)`);
});

function validateEvidenceEnvelope(evidence) {
  const keys = evidence && typeof evidence === "object" && !Array.isArray(evidence) ? Object.keys(evidence).sort().join(",") : "";
  if (keys !== "artifacts,config_sha256,generator,llms,media,pages,primitive_tokens,repository,schema_version" || evidence.schema_version !== 1
    || Object.keys(evidence.generator ?? {}).sort().join(",") !== "name,version" || evidence.generator.name !== "@codeflow/docs-portal" || evidence.generator.version !== "1.0.0"
    || Object.keys(evidence.repository ?? {}).sort().join(",") !== "commit,release_version,root" || !/^[a-f0-9]{40}$/.test(evidence.repository.commit ?? "")
    || !Array.isArray(evidence.pages) || evidence.pages.length > 10_000 || !Array.isArray(evidence.media) || evidence.media.length > 1_000
    || !evidence.llms || typeof evidence.llms !== "object" || Array.isArray(evidence.llms) || !Array.isArray(evidence.artifacts) || evidence.artifacts.length !== 0) {
    throw new Error("portal evidence has an invalid pre-build envelope");
  }
}
