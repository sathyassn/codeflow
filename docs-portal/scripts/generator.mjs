// Actual generator identity, not a claim copied from adoption state. A project
// fork changes this identity and its declared adoption identity together.
export const GENERATOR = Object.freeze({ name: "@codeflow/docs-portal", version: "2.0.0" });

export function assertGeneratorIdentity(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || Object.keys(value).sort().join(",") !== "name,version"
    || ![value.name, value.version].every((item) => typeof item === "string" && item.trim().length > 0 && Buffer.byteLength(item, "utf8") <= 128)
    || value.name !== GENERATOR.name || value.version !== GENERATOR.version) {
    throw new Error("portal evidence generator does not match this runtime");
  }
}

assertGeneratorIdentity(GENERATOR);
