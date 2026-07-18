use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("CLI crate must live below the workspace root")
        .to_path_buf()
}

#[test]
fn distribution_config_keeps_supported_targets_and_installers() {
    let root = workspace_root();
    let raw = fs::read_to_string(root.join("dist-workspace.toml"))
        .expect("dist-workspace.toml must be readable");
    let config: toml::Value = toml::from_str(&raw).expect("distribution config must be TOML");
    let dist = config
        .get("dist")
        .and_then(toml::Value::as_table)
        .expect("[dist] table must exist");

    let targets = dist
        .get("targets")
        .and_then(toml::Value::as_array)
        .expect("dist targets must be an array")
        .iter()
        .map(|value| value.as_str().expect("target must be a string"))
        .collect::<Vec<_>>();
    for required in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(
            targets.contains(&required),
            "missing release target: {required}"
        );
    }

    let installers = dist
        .get("installers")
        .and_then(toml::Value::as_array)
        .expect("dist installers must be an array")
        .iter()
        .map(|value| value.as_str().expect("installer must be a string"))
        .collect::<Vec<_>>();
    assert!(installers.contains(&"shell"));
    assert!(installers.contains(&"powershell"));
}

#[test]
fn generated_release_workflow_uses_cargo_dist_platform_matrix() {
    let workflow = fs::read_to_string(workspace_root().join(".github/workflows/release.yml"))
        .expect("generated release workflow must be readable");
    for required in [
        "runs-on: ${{ matrix.runner }}",
        "enable windows longpaths",
        "--output-format=json > plan-dist-manifest.json",
    ] {
        assert!(
            workflow.contains(required),
            "generated workflow is missing {required}"
        );
    }
}
