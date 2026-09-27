//! TSK-128 AC-5: `codeflow update` moves a project onto the rule
//! re-injection wiring at every tier, from the presets before this task and
//! from this task's interim presets (which wired a separate
//! `prompt-reminder` command). Claude gets the explicit `SessionStart`
//! sources with `fork` and the stable advisory entry on `UserPromptSubmit`,
//! Codex gets the entry on `UserPromptSubmit`, and Grok loses the
//! `session-orient` registrations whose output no Grok model receives. The
//! adopter's own hooks, the guards and explicit policy values stay, and
//! repeated updates leave one invocation per event. The
//! `guidance.prompt_reminders` key is not written into `policy.json`, so an
//! older binary still reads the file; its default applies.
//!
//! Older scaffolds are served by overlays over the current asset tree that
//! rewrite exactly the TSK-128 changes; a shape guard fails when the shipped
//! files change, so an overlay cannot silently serve the current files.
//!
//! The file also holds the contract that the guidance block lists every
//! skill and agent a fresh scaffold installs at each tier.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codeflow_core::hooks::guidance;
use codeflow_core::scaffold::rule_map::Kernel;
use codeflow_core::scaffold::{
    self, AssetSource, DirSource, InitAnswers, InitOptions, Tier, UpdateOptions,
};
use serde_json::Value;

const CLAUDE_SOURCES: &str = "startup|resume|clear|compact|fork";
const INTERIM_SOURCES: &str = "startup|resume|clear|compact";
const ADVISORY: &str = "codeflow hook session-orient";
const INTERIM_PROMPT: &str = "codeflow hook prompt-reminder";
const CODEX_PROMPT_BLOCK: &str = r#",
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "codeflow hook session-orient", "timeout": 10 }
        ]
      }
    ]"#;
const GROK_GUARDS_END: &str = r#"          { "type": "command", "command": "codeflow hook exec-guard", "timeout": 10 }
        ]
      }
    ]"#;
const GROK_PREVIOUS_ORIENT: &str = r#",
    "SessionStart": [
      {
        "matcher": "startup|resume|clear",
        "hooks": [
          { "type": "command", "command": "codeflow hook session-orient", "timeout": 10 }
        ]
      }
    ],
    "PreCompact": [
      {
        "hooks": [
          { "type": "command", "command": "codeflow hook session-orient", "timeout": 10 }
        ]
      }
    ],
    "PostCompact": [
      {
        "hooks": [
          { "type": "command", "command": "codeflow hook session-orient", "timeout": 10 }
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

fn assets() -> DirSource {
    DirSource::new(repo_root().join("assets"))
}

/// The current tree with some assets rewritten.
struct Overlay {
    current: DirSource,
    overrides: BTreeMap<String, Vec<u8>>,
}

impl AssetSource for Overlay {
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

/// Which older scaffold an overlay serves.
#[derive(Clone, Copy, Debug)]
enum Older {
    /// Before TSK-128: a matcher-less Claude `SessionStart`, no prompt hook,
    /// and Grok's stdout-only orient registrations.
    Previous,
    /// This task's first review head: a separate `prompt-reminder` command
    /// and a four-source Claude matcher.
    Interim,
}

fn older_source(older: Older) -> Overlay {
    let current = assets();
    let mut overrides = BTreeMap::new();
    for preset in ["default", "acceptEdits", "bypass-sandboxed"] {
        let path = format!("base/settings/{preset}.json");
        let mut value: Value = serde_json::from_str(&text(&current, &path)).unwrap();
        let hooks = value["hooks"].as_object_mut().unwrap();
        let start = hooks["SessionStart"][0].as_object_mut().unwrap();
        assert_eq!(start["matcher"], CLAUDE_SOURCES, "{path}: shape changed");
        match older {
            Older::Previous => {
                start.shift_remove("matcher");
                assert!(hooks.shift_remove("UserPromptSubmit").is_some(), "{path}");
            }
            Older::Interim => {
                start.insert("matcher".into(), Value::from(INTERIM_SOURCES));
                let prompt = &mut hooks["UserPromptSubmit"][0]["hooks"][0]["command"];
                assert_eq!(*prompt, ADVISORY, "{path}: shape changed");
                *prompt = Value::from(INTERIM_PROMPT);
            }
        }
        let mut bytes = serde_json::to_string_pretty(&value).unwrap();
        bytes.push('\n');
        overrides.insert(path, bytes.into_bytes());
    }
    let codex = text(&current, "base/codex/hooks.json");
    assert!(
        codex.contains(CODEX_PROMPT_BLOCK),
        "codex hooks shape changed"
    );
    let codex = match older {
        Older::Previous => codex.replace(CODEX_PROMPT_BLOCK, ""),
        Older::Interim => codex.replace(
            CODEX_PROMPT_BLOCK,
            &CODEX_PROMPT_BLOCK.replace(ADVISORY, INTERIM_PROMPT),
        ),
    };
    overrides.insert("base/codex/hooks.json".to_string(), codex.into_bytes());
    let grok = text(&current, "base/grok/hooks.json");
    assert!(grok.contains(GROK_GUARDS_END), "grok hooks shape changed");
    assert!(!grok.contains("session-orient"), "grok hooks shape changed");
    overrides.insert(
        "base/grok/hooks.json".to_string(),
        grok.replacen(
            GROK_GUARDS_END,
            &format!("{GROK_GUARDS_END}{GROK_PREVIOUS_ORIENT}"),
            1,
        )
        .into_bytes(),
    );
    Overlay { current, overrides }
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
        &assets(),
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

fn count(pairs: &[(Option<String>, String)], command: &str) -> usize {
    pairs.iter().filter(|(_, c)| c == command).count()
}

const OWN_CODEX_OR_GROK_HOOK: &str = "    \"PostToolUse\": [\n      { \"matcher\": \"^Edit$\", \"hooks\": [ { \"type\": \"command\", \"command\": \"./scripts/lint.sh\" } ] }\n    ],\n";

/// The adopter's own hooks: a start hook in the shipped `SessionStart` group
/// as the older scaffold wrote it, a prompt hook, and a tool hook above the
/// shipped entries in the Codex and Grok files.
fn add_adopter_hooks(root: &Path) {
    let settings_path = root.join(".claude/settings.json");
    let mut settings = json(&settings_path);
    settings["hooks"]["SessionStart"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"type": "command", "command": "./scripts/my-start.sh"}));
    let prompt = settings["hooks"]
        .as_object_mut()
        .unwrap()
        .entry("UserPromptSubmit")
        .or_insert_with(|| serde_json::json!([]));
    prompt.as_array_mut().unwrap().push(
        serde_json::json!({"hooks": [{"type": "command", "command": "./scripts/my-prompt.sh"}]}),
    );
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap() + "\n",
    )
    .unwrap();

    for rel in [".codex/hooks.json", ".grok/hooks/codeflow.json"] {
        let path = root.join(rel);
        let text = read(&path).replacen(
            "  \"hooks\": {\n",
            &format!("  \"hooks\": {{\n{OWN_CODEX_OR_GROK_HOOK}"),
            1,
        );
        assert!(text.contains("./scripts/lint.sh"), "{rel}: layout changed");
        std::fs::write(&path, text).unwrap();
    }

    let policy_path = root.join(".codeflow/policy.json");
    let mut policy = json(&policy_path);
    policy["security"]["headless_peer_runs"] = Value::from("block");
    std::fs::write(
        &policy_path,
        serde_json::to_string_pretty(&policy).unwrap() + "\n",
    )
    .unwrap();
}

fn assert_current_wiring(root: &Path, label: &str) {
    let settings = json(&root.join(".claude/settings.json"));
    let start = wired(&settings, "SessionStart");
    assert_eq!(count(&start, ADVISORY), 1, "{label}: {start:?}");
    assert!(
        start.contains(&(Some(CLAUDE_SOURCES.to_string()), ADVISORY.to_string())),
        "{label}: session-orient must sit under the Claude sources: {start:?}"
    );
    assert_eq!(
        count(&start, "./scripts/my-start.sh"),
        1,
        "{label}: adopter start hook lost: {start:?}"
    );
    let prompt = wired(&settings, "UserPromptSubmit");
    assert_eq!(count(&prompt, ADVISORY), 1, "{label}: {prompt:?}");
    assert_eq!(count(&prompt, INTERIM_PROMPT), 0, "{label}: {prompt:?}");
    assert!(
        prompt.contains(&(None, "./scripts/my-prompt.sh".to_string())),
        "{label}: adopter prompt hook lost: {prompt:?}"
    );
    assert_eq!(
        count(&wired(&settings, "PreToolUse"), "codeflow hook git-guard"),
        1
    );

    let codex = json(&root.join(".codex/hooks.json"));
    assert_eq!(
        wired(&codex, "UserPromptSubmit"),
        vec![(None, ADVISORY.to_string())],
        "{label}"
    );
    assert_eq!(
        count(&wired(&codex, "SessionStart"), ADVISORY),
        1,
        "{label}"
    );
    assert_eq!(
        wired(&codex, "PostToolUse"),
        vec![(Some("^Edit$".to_string()), "./scripts/lint.sh".to_string())],
        "{label}: adopter codex hook lost"
    );

    let grok = json(&root.join(".grok/hooks/codeflow.json"));
    for event in [
        "SessionStart",
        "PreCompact",
        "PostCompact",
        "UserPromptSubmit",
    ] {
        assert!(
            wired(&grok, event).is_empty(),
            "{label}: Grok still wires {event}"
        );
    }
    let guards = wired(&grok, "PreToolUse");
    assert_eq!(count(&guards, "codeflow hook git-guard"), 1, "{label}");
    assert_eq!(count(&guards, "codeflow hook exec-guard"), 1, "{label}");
    assert_eq!(
        wired(&grok, "PostToolUse"),
        vec![(Some("^Edit$".to_string()), "./scripts/lint.sh".to_string())],
        "{label}: adopter grok hook lost"
    );

    let policy = json(&root.join(".codeflow/policy.json"));
    assert!(policy.get("guidance").is_none(), "{label}: key written");
    assert_eq!(
        policy["security"]["headless_peer_runs"], "block",
        "{label}: explicit value lost"
    );
}

fn snapshot(root: &Path) -> [String; 4] {
    [
        read(&root.join(".claude/settings.json")),
        read(&root.join(".codex/hooks.json")),
        read(&root.join(".grok/hooks/codeflow.json")),
        read(&root.join(".codeflow/policy.json")),
    ]
}

#[test]
fn update_moves_older_scaffolds_onto_the_wiring_and_keeps_adopter_hooks_at_every_tier() {
    for older in [Older::Previous, Older::Interim] {
        let source = older_source(older);
        for tier in [Tier::Minimal, Tier::Standard, Tier::Full] {
            let label = format!("{older:?} {tier}");
            let project = scaffold_at(&source, tier);
            let root = project.path();
            let before = json(&root.join(".claude/settings.json"));
            match older {
                Older::Previous => {
                    assert_eq!(
                        wired(&before, "SessionStart"),
                        vec![(None, ADVISORY.to_string())],
                        "{label}"
                    );
                    assert!(wired(&before, "UserPromptSubmit").is_empty(), "{label}");
                }
                Older::Interim => assert_eq!(
                    wired(&before, "UserPromptSubmit"),
                    vec![(None, INTERIM_PROMPT.to_string())],
                    "{label}"
                ),
            }
            let grok_before = json(&root.join(".grok/hooks/codeflow.json"));
            assert_eq!(count(&wired(&grok_before, "PreCompact"), ADVISORY), 1);
            add_adopter_hooks(root);

            let report = update(root);
            assert!(!report.contains("CONFLICT"), "{label}: {report}");
            for rel in [".codex/hooks.json.new", ".grok/hooks/codeflow.json.new"] {
                assert!(!root.join(rel).exists(), "{label}: {rel}\n{report}");
            }
            assert_current_wiring(root, &label);

            let first = snapshot(root);
            for round in 0..2 {
                let again = update(root);
                assert!(!again.contains("CONFLICT"), "{label} {round}: {again}");
                assert_eq!(snapshot(root), first, "{label}: update {round} changed it");
            }
        }
    }
}

/// F3: the guidance block lists every skill a fresh scaffold installs under
/// `.agents/skills/` and every agent under `.claude/agents/`, the agents on
/// their own line, at every tier. Sizes are printed as guidance numbers.
#[test]
fn guidance_lists_every_installed_skill_and_agent_at_every_tier() {
    let kernel = Kernel::shipped();
    for tier in [Tier::Minimal, Tier::Standard, Tier::Full] {
        let project = scaffold_at(&assets(), tier);
        let root = project.path();
        let listed = |dir: &str, skill: bool| -> Vec<String> {
            let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
                return Vec::new();
            };
            let mut names: Vec<String> = entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if skill {
                        entry.path().join("SKILL.md").exists().then_some(name)
                    } else {
                        name.strip_suffix(".md").map(str::to_string)
                    }
                })
                .collect();
            names.sort();
            names
        };
        let installed = guidance::Inventory {
            skills: listed(".agents/skills", true),
            agents: listed(".claude/agents", false),
        };
        let inventory = guidance::installed_inventory(guidance::SCAFFOLD_MANIFEST, tier);
        assert_eq!(
            inventory, installed,
            "{tier}: inventory differs from the scaffold"
        );

        let block = guidance::guidance_block(&kernel, &inventory, tier);
        let line = |prefix: &str| {
            block.lines().find(|l| l.starts_with(prefix)).map(|l| {
                l.trim_start_matches(prefix)
                    .trim_end_matches('.')
                    .to_string()
            })
        };
        let skills: Vec<String> = line("Skills: ")
            .filter(|l| l != "none at this tier")
            .map(|l| l.split(", ").map(str::to_string).collect())
            .unwrap_or_default();
        let agents: Vec<String> = line("Agents: ")
            .map(|l| l.split(", ").map(str::to_string).collect())
            .unwrap_or_default();
        assert_eq!(skills, installed.skills, "{tier}\n{block}");
        assert_eq!(agents, installed.agents, "{tier}\n{block}");
        println!(
            "{tier}: {} skills, {} agents, block {} bytes (guideline {})",
            skills.len(),
            agents.len(),
            block.len(),
            guidance::GUIDANCE_BLOCK_GUIDELINE_BYTES
        );
    }
}
