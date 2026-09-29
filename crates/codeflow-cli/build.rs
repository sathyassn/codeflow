#[path = "../codeflow-core/src/hooks/source_identity.rs"]
mod source_identity;

fn main() {
    let manifest =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("package root"));
    let root = manifest.join("../..").canonicalize().expect("source root");
    println!("cargo:rerun-if-env-changed=CODEFLOW_SOURCE_REVISION");
    // Dirty metadata must follow non-hook source edits as well.
    for input in [
        "crates",
        "assets",
        "docs",
        "project-management",
        ".agents",
        ".claude",
        "Cargo.toml",
        "Cargo.lock",
        "AGENTS.md",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    let supplied = std::env::var("CODEFLOW_SOURCE_REVISION").ok();
    let (revision, dirty, metadata) = source_identity::revision(&root, supplied.as_deref());
    for path in source_identity::INPUT_ROOTS {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    for path in source_identity::input_files(&root)
        .expect("enumerate hook source inputs")
        .into_iter()
        .chain(metadata)
    {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let digest = source_identity::input_digest(&root).expect("hash hook source inputs");
    println!("cargo:rustc-env=CODEFLOW_SOURCE_REVISION={revision}");
    println!("cargo:rustc-env=CODEFLOW_SOURCE_DIRTY={dirty}");
    println!("cargo:rustc-env=CODEFLOW_HOOK_INPUT_DIGEST={digest}");
}
