// Producer limits that must remain in exact parity with the read-only Rust
// verifier. Boundary tests on both sides make drift a release-blocking defect.
export const EVIDENCE_LIMITS = Object.freeze({
  manifestBytes: 8 * 1024 * 1024,
  pages: 10_000,
  idsPerPage: 256,
  relationshipsPerPage: 512,
  backlinksPerPage: 512,
  snippetsPerPage: 64,
});

export function assertEvidencePageLimits(page, label = "page") {
  for (const [field, maximum] of [
    ["ids", EVIDENCE_LIMITS.idsPerPage],
    ["relationships", EVIDENCE_LIMITS.relationshipsPerPage],
    ["backlinks", EVIDENCE_LIMITS.backlinksPerPage],
    ["snippets", EVIDENCE_LIMITS.snippetsPerPage],
  ]) {
    if (!Array.isArray(page?.[field]) || page[field].length > maximum) throw new Error(`${label}: ${field} count exceeds ${maximum}`);
  }
}

export function assertEvidenceEnvelope(pages, encoded) {
  if (!Array.isArray(pages) || pages.length > EVIDENCE_LIMITS.pages) throw new Error(`page count exceeds ${EVIDENCE_LIMITS.pages}`);
  if (Buffer.byteLength(encoded) > EVIDENCE_LIMITS.manifestBytes) throw new Error(`evidence manifest exceeds ${EVIDENCE_LIMITS.manifestBytes} bytes`);
}
