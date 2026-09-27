//! TSK-128 AC-5: `codeflow update` moves a project scaffolded before the
//! rule re-injection onto it at every tier. The Claude settings gain the
//! explicit `SessionStart` matcher and the `UserPromptSubmit` prompt
//! reminder and the Codex hooks file gains the prompt reminder, while the
//! adopter's own hooks and explicit policy values stay. The
//! `guidance.prompt_reminders` key is not written into `policy.json`, so an
//! older binary still reads the file; its default applies.
//!
//! The previous scaffold is served by an overlay over the current asset
//! tree that removes exactly the TSK-128 additions; a shape guard fails when
//! the shipped files no longer carry them, so the overlay cannot silently
//! serve the current files as "previous".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codeflow_core::scaffold::{
    self, AssetSource, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions,
};
use serde_json::Value;

const MATCHER: &str = "startup|resume|clear|compact";
const PROMPT_HOOK: &str = "codeflow hook prompt-reminder";
const ORIENT_HOOK: &str = "codeflow hook session-orient";
const CODEX_PROMPT_BLOCK: &str = r#",
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "codeflow hook prompt-reminder", "timeout": 10 }
        ]
      }
    ]"#;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn isolate_git() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    });
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&read(path)).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The current tree with the TSK-128 wiring taken out.
struct Previous {
    current: DirSource,
    overrides: BTreeMap<String, Vec<u8>>,
}

impl AssetSource for Previous {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.overrides
            .get(path)
            .cloned()
            .or_else(|| self.current.read(path))
    }
}

fn text(source: &DirSource, path: &str) -> String {
    String::from_utf8(
        source
            .read(path)
            .unwrap_or_else(|| panic!("{path} missing")),
    )
    .unwrap()
}

fn previous_source() -> Previous {
    let current = DirSource::new(repo_root().join("assets"));
    let mut overrides = BTreeMap::new();
    for preset in ["default", "acceptEdits", "bypass-sandboxed"] {
        let path = format!("base/settings/{preset}.json");
        let mut value: Value = serde_json::from_str(&text(&current, &path)).unwrap();
        let hooks = value["hooks"].as_object_mut().unwrap();
        assert!(
            hooks.shift_remove("UserPromptSubmit").is_some(),
            "{path}: shape changed, the prompt reminder is not wired"
        );
        let start = hooks["SessionStart"][0].as_object_mut().unwrap();
        assert_eq!(start.shift_remove("matcher"), Some(Value::from(MATCHER)));
        let mut bytes = serde_json::to_string_pretty(&value).unwrap();
        bytes.push('\n');
        overrides.insert(path, bytes.into_bytes());
    }
    let codex = text(&current, "base/codex/hooks.json");
    assert!(
        codex.contains(CODEX_PROMPT_BLOCK),
        "codex hooks shape changed"
    );
    overrides.insert(
        "base/codex/hooks.json".to_string(),
        codex.replace(CODEX_PROMPT_BLOCK, "").into_bytes(),
    );
    Previous { current, overrides }
}

fn scaffold_at(source: &dyn AssetSource, tier: Tier) -> tempfile::TempDir {
    isolate_git();
    let dir = tempfile::tempdir().expect("project tempdir");
    let options = InitOptions {
        tier: Some(tier),
        force: false,
        binary_version: "3.0.0".to_string(),
        answers: InitAnswers {
            product_one_liner: Some("A billing service for small clinics.".to_string()),
            areas: Some(vec!["api".to_string()]),
            permission_preset: None,
        },
    };
    scaffold::init(source, dir.path(), &options).expect("init succeeds");
    dir
}

fn update(root: &Path) -> String {
    let report = scaffold::update(
        &DirSource::new(repo_root().join("assets")),
        root,
        &UpdateOptions {
            force: false,
            binary_version: "3.0.0".to_string(),
            diff_out: None,
        },
    )
    .expect("update succeeds");
    report.to_string()
}

/// Every (matcher, command) pair wired for `event`.
fn wired(file: &Value, event: &str) -> Vec<(Option<String>, String)> {
    file["hooks"][event]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| {
            let matcher = group["matcher"].as_str().map(str::to_string);
            group["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|hook| hook["command"].as_str())
                .map(move |command| (matcher.clone(), command.to_string()))
        })
        .collect()
}

/// The adopter's own hooks: a start hook in the matcher-less group the
/// previous scaffold wrote, a prompt hook, and a Codex tool hook above the
/// shipped entries.
fn add_adopter_hooks(root: &Path) {
    let settings_path = root.join(".claude/settings.json");
    let mut settings = json(&settings_path);
    settings["hooks"]["SessionStart"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"type": "command", "command": "./scripts/my-start.sh"}));
    settings["hooks"]["UserPromptSubmit"] = serde_json::json!([
        {"hooks": [{"type": "command", "command": "./scripts/my-prompt.sh"}]}
    ]);
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap() + "\n",
    )
    .unwrap();

    let codex_path = root.join(".codex/hooks.json");
    let codex = read(&codex_path).replacen(
        "  \"hooks\": {\n",
        "  \"hooks\": {\n    \"PostToolUse\": [\n      { \"matcher\": \"^Edit$\", \"hooks\": [ { \"type\": \"command\", \"command\": \"./scripts/lint.sh\" } ] }\n    ],\n",
        1,
    );
    assert!(
        codex.contains("./scripts/lint.sh"),
        "codex hooks layout changed"
    );
    std::fs::write(&codex_path, codex).unwrap();

    let policy_path = root.join(".codeflow/policy.json");
    let mut policy = json(&policy_path);
    policy["security"]["headless_peer_runs"] = Value::from("block");
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
}

#[test]
fn update_adds_the_reinjection_wiring_and_keeps_adopter_hooks_at_every_tier() {
    let previous = previous_source();
    for tier in [Tier::Minimal, Tier::Standard, Tier::Full] {
        let project = scaffold_at(&previous, tier);
        let root = project.path();
        let before = json(&root.join(".claude/settings.json"));
        assert_eq!(
            wired(&before, "SessionStart"),
            vec![(None, ORIENT_HOOK.to_string())],
            "{tier}: the previous scaffold had a matcher-less SessionStart"
        );
        assert!(wired(&before, "UserPromptSubmit").is_empty(), "{tier}");
        add_adopter_hooks(root);

        let report = update(root);
        assert!(!report.contains("CONFLICT"), "{tier}: {report}");
        assert!(
            !root.join(".codex/hooks.json.new").exists(),
            "{tier}: {report}"
        );

        let settings = json(&root.join(".claude/settings.json"));
        let start = wired(&settings, "SessionStart");
        assert_eq!(
            start
                .iter()
                .filter(|(_, command)| command == ORIENT_HOOK)
                .collect::<Vec<_>>(),
            vec![&(Some(MATCHER.to_string()), ORIENT_HOOK.to_string())],
            "{tier}: session-orient must run once, under the explicit matcher: {start:?}"
        );
        assert!(
            start.contains(&(None, "./scripts/my-start.sh".to_string())),
            "{tier}: adopter start hook lost: {start:?}"
        );
        let prompt = wired(&settings, "UserPromptSubmit");
        assert!(
            prompt.contains(&(None, PROMPT_HOOK.to_string())),
            "{tier}: {prompt:?}"
        );
        assert!(
            prompt.contains(&(None, "./scripts/my-prompt.sh".to_string())),
            "{tier}: adopter prompt hook lost: {prompt:?}"
        );

        let codex = json(&root.join(".codex/hooks.json"));
        assert_eq!(
            wired(&codex, "UserPromptSubmit"),
            vec![(None, PROMPT_HOOK.to_string())],
            "{tier}"
        );
        assert_eq!(
            wired(&codex, "PostToolUse"),
            vec![(Some("^Edit$".to_string()), "./scripts/lint.sh".to_string())],
            "{tier}: adopter codex hook lost"
        );

        let policy = json(&root.join(".codeflow/policy.json"));
        assert!(policy.get("guidance").is_none(), "{tier}: key written");
        assert_eq!(
            policy["security"]["headless_peer_runs"], "block",
            "{tier}: explicit value lost"
        );

        let snapshot = [
            read(&root.join(".claude/settings.json")),
            read(&root.join(".codex/hooks.json")),
            read(&root.join(".codeflow/policy.json")),
        ];
        let second = update(root);
        assert!(!second.contains("CONFLICT"), "{tier}: {second}");
        assert_eq!(
            [
                read(&root.join(".claude/settings.json")),
                read(&root.join(".codex/hooks.json")),
                read(&root.join(".codeflow/policy.json")),
            ],
            snapshot,
            "{tier}: a second update changed the wiring"
        );
    }
}
