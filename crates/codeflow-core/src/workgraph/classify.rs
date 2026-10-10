//! Path classification for pull request ranges (SPC-013 R-70, R-114).
//!
//! One embedded table, `path_sets.toml`, defines the adopter-facing path set
//! used by the journey criterion and release contract (R-114). Every PR
//! names its task or epic (R-70). The planning-only, planning amendment
//! (ADR-0078) and spike path rules live here too, so every caller judges a
//! range the same way.

use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

/// One named member of a path set.
#[derive(Debug, Clone, Deserialize)]
pub struct PathMember {
    /// Stable member name, reported when a path matches.
    pub member: String,
    /// Globs (the `glob` crate syntax) against repository-relative paths.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// A policy key whose globs extend this member (the watched contract
    /// paths); the caller supplies its value.
    #[serde(default)]
    pub policy_key: Option<String>,
    /// One path of this member, classified by the tests and journeys.
    #[serde(default)]
    pub example: Option<String>,
    /// A release contract path (TSK-106): `.release/config.json` watches it
    /// too, so a release impact assessment names it.
    #[serde(default)]
    pub release_contract: bool,
}

/// The path sets of `path_sets.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct PathSets {
    /// Surfaces that reach adopters (R-114).
    pub adopter_facing: Vec<PathMember>,
    /// Paths that are not behaviour for the local release preflight (R-93).
    #[serde(default)]
    pub not_behaviour: Vec<PathMember>,
    /// Paths that are behaviour whatever `not_behaviour` says: the skill
    /// trees, which change shipped agent behaviour.
    #[serde(default)]
    pub behaviour_always: Vec<PathMember>,
}

/// The embedded path-set table, as shipped.
pub const PATH_SETS_TOML: &str = include_str!("path_sets.toml");

/// The parsed path sets.
///
/// # Panics
///
/// Panics only when the embedded table does not parse, which the unit tests
/// rule out for every build.
#[must_use]
pub fn path_sets() -> &'static PathSets {
    static SETS: OnceLock<PathSets> = OnceLock::new();
    SETS.get_or_init(|| toml::from_str(PATH_SETS_TOML).expect("embedded path_sets.toml parses"))
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    glob::Pattern::new(pattern).is_ok_and(|glob| glob.matches(path))
}

/// The project-owned globs the table's policy members read.
#[derive(Debug, Clone, Default)]
pub struct ProjectPaths {
    /// `git.product_paths`, or the stack default when the key is absent.
    pub product: Vec<String>,
    /// `git.breaking_watch_paths`.
    pub watched: Vec<String>,
}

impl ProjectPaths {
    /// Read the project's paths from its effective policy, falling back to the
    /// default for the stack recorded in `.codeflow/project.toml` (or the
    /// generic default when no stack is recorded).
    ///
    /// # Errors
    ///
    /// Returns an error if the effective policy or project stack cannot be read.
    pub fn load(repo_root: &Path) -> Result<Self, String> {
        let (policy, _) = crate::hooks::policy::Policy::load_effective(repo_root)?;
        let product = if let Some(paths) = policy.git.product_paths {
            paths
        } else {
            let project = crate::hooks::policy::read_project_toml(repo_root)?;
            let stack = match project.as_ref().and_then(|value| value.get("stack")) {
                None => "",
                Some(value) => value.as_str().ok_or("project stack is not a string")?,
            };
            stack_product_paths(stack)
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        let paths = Self {
            product,
            watched: policy.git.breaking_watch_paths,
        };
        paths.validate()?;
        Ok(paths)
    }

    /// Validate configured globs before they can classify a decision path.
    ///
    /// # Errors
    /// Returns the invalid policy key and pattern, with a repair instruction.
    pub fn validate(&self) -> Result<(), String> {
        for (key, patterns) in [
            ("product_paths", &self.product),
            ("breaking_watch_paths", &self.watched),
        ] {
            for pattern in patterns {
                glob::Pattern::new(pattern).map_err(|error| {
                    format!("invalid git.{key} glob {pattern:?}: {error}; correct the path pattern in .codeflow/policy.json")
                })?;
            }
        }
        Ok(())
    }

    fn for_key(&self, key: &str) -> &[String] {
        match key {
            "git.product_paths" => &self.product,
            "git.breaking_watch_paths" => &self.watched,
            _ => &[],
        }
    }
}

/// The default `git.product_paths` for a stack, as `codeflow init` writes it
/// and as the binary assumes when the key is absent. An unknown stack gets
/// the common source roots of several ecosystems.
#[must_use]
pub fn stack_product_paths(stack: &str) -> &'static [&'static str] {
    match stack {
        "rust" => &["src/**", "crates/**", "build.rs"],
        "node" => &["src/**", "lib/**", "app/**", "packages/**"],
        "python" => &["src/**", "**/*.py"],
        _ => &[
            "src/**",
            "lib/**",
            "app/**",
            "cmd/**",
            "pkg/**",
            "internal/**",
        ],
    }
}

fn member_matches(member: &PathMember, path: &str, project: &ProjectPaths) -> bool {
    let policy_globs = member
        .policy_key
        .as_deref()
        .map_or(&[][..], |key| project.for_key(key));
    member
        .patterns
        .iter()
        .chain(policy_globs)
        .any(|pattern| glob_matches(pattern, path))
}

impl PathSets {
    /// Whether an adopter-facing member matches `path` in any letter case,
    /// for the planning amendment, which must not admit a path a
    /// case-insensitive file system would read as an adopter-facing one.
    #[must_use]
    pub fn adopter_facing_any_case(&self, path: &str, project: &ProjectPaths) -> bool {
        let options = glob::MatchOptions {
            case_sensitive: false,
            ..glob::MatchOptions::new()
        };
        self.adopter_facing.iter().any(|member| {
            let policy_globs = member
                .policy_key
                .as_deref()
                .map_or(&[][..], |key| project.for_key(key));
            member.patterns.iter().chain(policy_globs).any(|pattern| {
                glob::Pattern::new(pattern).is_ok_and(|glob| glob.matches_with(path, options))
            })
        })
    }

    /// The adopter-facing member `path` belongs to, if any.
    #[must_use]
    pub fn adopter_facing_member(&self, path: &str, project: &ProjectPaths) -> Option<&str> {
        self.adopter_facing
            .iter()
            .find(|member| member_matches(member, path, project))
            .map(|member| member.member.as_str())
    }
}

impl PathSets {
    /// Whether `path` is behaviour for the local release preflight (R-93,
    /// R-114): every path outside the documentation, the records and the
    /// record templates, and the skill trees always. `scripts/release.py`
    /// reads the same two sections of the table.
    #[must_use]
    pub fn is_behaviour(&self, path: &str) -> bool {
        let none = ProjectPaths::default();
        let within = |members: &[PathMember]| {
            members
                .iter()
                .any(|member| member_matches(member, path, &none))
        };
        within(&self.behaviour_always) || !within(&self.not_behaviour)
    }
}

/// Whether `path` is a planning path: a record under `project-management/`
/// (not the record templates, which are schema) or a plan under `docs/plan/`.
/// A storage key of a name that is not text is never one (OS text rule,
/// issue 79).
#[must_use]
pub fn is_planning_path(path: &str) -> bool {
    crate::git::key_is_text(path)
        && ((path.starts_with("project-management/")
            && !path.starts_with("project-management/templates/"))
            || path.starts_with("docs/plan/"))
}

/// What a path is to a planning amendment (ADR-0078, SPC-013 R-70).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmendmentPath {
    /// A record or a plan ([`is_planning_path`]).
    Record,
    /// A documentation file under `docs/` that is not adopter-facing.
    Doc,
    /// The root `AGENTS.md`, admitted only while its managed block is
    /// byte-identical to the target's ([`instructions_block_unchanged`]).
    Instructions,
}

/// The stems of the files a harness reads as instructions wherever they
/// sit (`AGENTS.md`, `AGENTS.override.md`, `CLAUDE.md`, `CLAUDE.local.md`,
/// `GEMINI.md`), matched without case so a case-insensitive file system
/// cannot slip one into a records or documentation folder.
const INSTRUCTION_STEMS: &[&str] = &["agents.", "claude.", "gemini."];

/// What a planning amendment may carry at `path`, or `None` when the path
/// keeps the range out of the planning class. Outside the root `AGENTS.md`,
/// whose managed block the caller compares, a path is refused first when
/// any folder or the file on the way is hidden (a `.gitkeep` excepted), its
/// file name is a harness instruction file, or an adopter-facing member
/// matches it in any letter case (so a shipped template such as
/// `docs/decisions/template.md`, or a product glob such as `**/*.py`, keeps
/// it out even under `docs/plan/`). What is left is admitted as a record or
/// plan ([`is_planning_path`]) or a file under `docs/`. `CLAUDE.md`,
/// `.claude/`, `.agents/`, `.codeflow/`, record templates, policy, hooks and
/// CI all stay out.
#[must_use]
pub fn amendment_path(path: &str, project: &ProjectPaths) -> Option<AmendmentPath> {
    if path == "AGENTS.md" {
        return Some(AmendmentPath::Instructions);
    }
    // A name that is not text is outside every planning class (issue 79).
    if !crate::git::key_is_text(path) {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    let hidden = parts
        .iter()
        .any(|part| part.starts_with('.') && *part != ".gitkeep");
    let instructions = parts
        .last()
        .map(|name| name.to_ascii_lowercase())
        .is_some_and(|name| INSTRUCTION_STEMS.iter().any(|stem| name.starts_with(stem)));
    if hidden || instructions || path_sets().adopter_facing_any_case(path, project) {
        return None;
    }
    if is_planning_path(path) {
        Some(AmendmentPath::Record)
    } else {
        path.starts_with("docs/").then_some(AmendmentPath::Doc)
    }
}

/// Whether `AGENTS.md` keeps the target's managed block byte for byte:
/// `target` and `head` are the file's bytes on each side, `None` where it is
/// absent. Two files without a block, or no file on either side, keep it;
/// adding, removing or editing the block, its markers included, does not.
/// A side that is not UTF-8 keeps it only when both files are identical.
#[must_use]
pub fn instructions_block_unchanged(target: Option<&[u8]>, head: Option<&[u8]>) -> bool {
    fn text(bytes: Option<&[u8]>) -> Result<Option<&str>, std::str::Utf8Error> {
        bytes.map(std::str::from_utf8).transpose()
    }
    match (text(target), text(head)) {
        (Ok(target), Ok(head)) => {
            let block = crate::scaffold::region::managed_span;
            target.and_then(block) == head.and_then(block)
        }
        _ => target == head,
    }
}

/// Whether a spike may land `path`: its findings under `docs/research/` or
/// its own task record (R-64).
#[must_use]
pub fn is_spike_path(path: &str, task_id: &str) -> bool {
    path.starts_with("docs/research/")
        || (path.starts_with("project-management/")
            && path
                .rsplit('/')
                .next()
                .is_some_and(|file| file == format!("{task_id}.md")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_parses_and_every_fixed_member_has_an_example() {
        let sets = path_sets();
        assert!(!sets.adopter_facing.is_empty());
        for member in &sets.adopter_facing {
            if member.policy_key.is_none() {
                assert!(member.example.is_some(), "{} has no example", member.member);
                assert!(
                    !member.patterns.is_empty(),
                    "{} has no globs",
                    member.member
                );
            }
            for pattern in &member.patterns {
                assert!(glob::Pattern::new(pattern).is_ok(), "bad glob {pattern}");
            }
        }
    }

    #[test]
    fn r22_project_paths_reject_invalid_globs_and_keep_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ProjectPaths::load(dir.path()).is_ok());
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        for key in ["product_paths", "breaking_watch_paths"] {
            let policy = serde_json::json!({"schema_version": 1, "git": {key: ["src/["]}});
            std::fs::write(dir.path().join(".codeflow/policy.json"), policy.to_string()).unwrap();
            assert!(ProjectPaths::load(dir.path()).is_err(), "{key}");
        }
    }

    fn codeflow_paths() -> ProjectPaths {
        let text = include_str!("../../../../.codeflow/policy.json");
        let policy: serde_json::Value = serde_json::from_str(text).unwrap();
        let list = |key: &str| -> Vec<String> {
            policy["git"][key]
                .as_array()
                .unwrap_or_else(|| panic!("CodeFlow policy sets git.{key}"))
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect()
        };
        ProjectPaths {
            product: list("product_paths"),
            watched: list("breaking_watch_paths"),
        }
    }

    /// AC-8: one path of each member classifies as that member, with the
    /// project-owned members read from this repository's own policy.
    #[test]
    fn one_path_of_each_member_classifies_as_that_member() {
        let sets = path_sets();
        let project = codeflow_paths();
        assert_eq!(
            sets.adopter_facing_member("crates/codeflow-present/src/lib.rs", &project),
            Some("product_paths")
        );
        let adopter = ProjectPaths {
            product: vec!["src/**".to_string()],
            watched: vec!["api/schema.json".to_string()],
        };
        assert_eq!(
            sets.adopter_facing_member("api/schema.json", &adopter),
            Some("watched_contract_paths")
        );
        for member in &sets.adopter_facing {
            if let Some(example) = &member.example {
                assert_eq!(
                    sets.adopter_facing_member(example, &adopter),
                    Some(member.member.as_str()),
                    "{example}"
                );
            }
        }
    }

    /// The fixed set of R-114, for this repository: the scaffold, hook
    /// sources, the CLI command tree and every runtime adopters run are
    /// adopter-facing through its own `git.product_paths`.
    #[test]
    fn codeflow_names_its_product_through_its_own_policy() {
        let sets = path_sets();
        let project = codeflow_paths();
        for path in [
            "assets/base/pm/task.md.tmpl",
            "crates/codeflow-core/src/hooks/git_guard.rs",
            "crates/codeflow-cli/src/cmd/work.rs",
            "crates/codeflow-present/src/render.rs",
        ] {
            assert_eq!(
                sets.adopter_facing_member(path, &project),
                Some("product_paths"),
                "{path}"
            );
        }
        for (path, member) in [
            ("CLAUDE.md", "managed_instructions"),
            (".claude/skills/cf-plan/SKILL.md", "managed_instructions"),
            (".codeflow/rules/writing.md", "managed_instructions"),
            (".github/workflows/release.yml", "ci_workflows"),
            ("crates/codeflow-core/src/hooks/policy.rs", "product_paths"),
        ] {
            assert_eq!(
                sets.adopter_facing_member(path, &project),
                Some(member),
                "{path}"
            );
        }
    }

    /// Each release contract member requires a journey criterion and is
    /// watched by the release configuration.
    #[test]
    fn release_contract_members_are_adopter_facing_and_watched() {
        let sets = path_sets();
        let project = codeflow_paths();
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../../../.release/config.json")).unwrap();
        let watched: Vec<&str> = config["watched_contract_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        let members: Vec<&PathMember> = sets
            .adopter_facing
            .iter()
            .filter(|member| member.release_contract)
            .collect();
        let names: Vec<&str> = members.iter().map(|m| m.member.as_str()).collect();
        assert_eq!(names, ["policy", "record_schema"]);
        for member in members {
            let example = member.example.as_deref().unwrap();
            for path in member
                .patterns
                .iter()
                .map(|glob| glob.replace("**", "x/y.md"))
                .chain([example.to_string()])
            {
                assert_eq!(
                    sets.adopter_facing_member(&path, &project),
                    Some(member.member.as_str()),
                    "{path}"
                );
                assert!(
                    sets.adopter_facing_member(&path, &project).is_some(),
                    "{path}"
                );
                assert!(
                    watched.iter().any(|glob| glob_matches(glob, &path)),
                    "{path} is not in .release/config.json watched_contract_paths"
                );
            }
        }
    }

    /// TSK-106 AC-10: the behaviour paths of the local preflight, checked on
    /// every member's example (the fixture `scripts/test_release.py` reads
    /// too) and on the paths R-114 names.
    #[test]
    fn behaviour_paths_follow_the_table() {
        let sets = path_sets();
        assert!(!sets.not_behaviour.is_empty() && !sets.behaviour_always.is_empty());
        for member in &sets.not_behaviour {
            let example = member.example.as_deref().unwrap();
            assert!(!sets.is_behaviour(example), "{example}");
        }
        for member in &sets.behaviour_always {
            let example = member.example.as_deref().unwrap();
            assert!(sets.is_behaviour(example), "{example}");
        }
        for (path, behaviour) in [
            ("crates/codeflow-core/src/lib.rs", true),
            ("scripts/release.py", true),
            ("README.md", true),
            ("assets/base/ci/codeflow-ci.yml", true),
            ("assets/base/agents/skills/cf-ship/SKILL.md", true),
            (".claude/skills/cf-plan/SKILL.md", true),
            ("docs/releasing.md", false),
            ("docs/verification/evidence/tsk-106/journey.txt", false),
            ("project-management/tasks/TSK-106.md", false),
            ("project-management/templates/task.md", false),
            ("assets/base/pm/spec.md.tmpl", false),
        ] {
            assert_eq!(sets.is_behaviour(path), behaviour, "{path}");
        }
    }

    #[test]
    fn every_stack_default_covers_its_source_root() {
        let sets = path_sets();
        for (stack, path) in [
            ("rust", "src/main.rs"),
            ("rust", "crates/engine/src/lib.rs"),
            ("node", "src/index.ts"),
            ("python", "mypkg/core.py"),
            ("unset", "cmd/tool/main.go"),
        ] {
            let project = ProjectPaths {
                product: stack_product_paths(stack)
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                watched: Vec::new(),
            };
            assert_eq!(
                sets.adopter_facing_member(path, &project),
                Some("product_paths"),
                "{stack} {path}"
            );
            assert_eq!(sets.adopter_facing_member("docs/guide.md", &project), None);
        }
    }

    #[test]
    fn ordinary_docs_and_records_are_outside_adopter_paths() {
        let sets = path_sets();
        let project = codeflow_paths();
        for path in [
            "docs/guide/start.md",
            "README.md",
            "project-management/tasks/TSK-001.md",
            "scripts/release.py",
        ] {
            assert_eq!(sets.adopter_facing_member(path, &project), None, "{path}");
        }
    }

    #[test]
    fn planning_paths_are_records_and_plans_not_templates() {
        assert!(is_planning_path("project-management/tasks/TSK-001.md"));
        assert!(is_planning_path("docs/plan/v2/00-charter.md"));
        assert!(!is_planning_path("project-management/templates/task.md"));
        assert!(!is_planning_path("docs/guide.md"));
        assert!(!is_planning_path("src/lib.rs"));
    }

    /// Issue 79: the storage key of a name that is not text is no planning
    /// record and no amendment document, whatever its prefix says.
    #[test]
    fn a_name_that_is_not_text_is_outside_the_planning_class() {
        let project = ProjectPaths {
            product: vec![],
            watched: vec![],
        };
        let record = crate::git::GitName::from_bytes(b"project-management/tasks/TSK-002\xff.md")
            .storage_key();
        let doc = crate::git::GitName::from_bytes(b"docs/caf\xe9.md").storage_key();
        assert!(!is_planning_path(&record));
        assert_eq!(amendment_path(&record, &project), None);
        assert_eq!(amendment_path(&doc, &project), None);
        assert_eq!(
            amendment_path("docs/cafe.md", &project),
            Some(AmendmentPath::Doc)
        );
    }

    /// TSK-229 AC-2, AC-4: a planning amendment carries records, plans,
    /// non-adopter docs and `AGENTS.md`; instruction and enforcement paths
    /// stay out, inside `docs/` too.
    #[test]
    fn an_amendment_carries_records_docs_and_the_project_section() {
        let project = ProjectPaths {
            product: vec!["src/**".to_string(), "**/*.py".to_string()],
            watched: vec!["docs/api/schema.json".to_string()],
        };
        for (path, expected) in [
            (
                "project-management/tasks/TSK-001.md",
                Some(AmendmentPath::Record),
            ),
            ("docs/plan/plan.md", Some(AmendmentPath::Record)),
            ("docs/reading.md", Some(AmendmentPath::Doc)),
            ("docs/decisions/ADR-0078-x.md", Some(AmendmentPath::Doc)),
            ("AGENTS.md", Some(AmendmentPath::Instructions)),
            ("docs/decisions/template.md", None),
            ("docs/api/schema.json", None),
            ("docs/tool.py", None),
            ("docs/AGENTS.md", None),
            ("docs/guide/CLAUDE.md", None),
            ("docs/guide/claude.local.md", None),
            ("docs/AGENTS.override.md", None),
            ("docs/Gemini.md", None),
            ("docs/agents-guide.md", Some(AmendmentPath::Doc)),
            ("docs/.claude/settings.json", None),
            ("docs/.github/workflows/x.yml", None),
            ("CLAUDE.md", None),
            (".claude/settings.json", None),
            (".agents/skills/cf-plan/SKILL.md", None),
            (".codeflow/policy.json", None),
            (".codeflow/rules/git-rules.md", None),
            (".github/workflows/codeflow-ci.yml", None),
            ("project-management/templates/task.md", None),
            ("src/lib.rs", None),
            ("README.md", None),
            ("crates/AGENTS.md", None),
            ("docs/plan/AGENTS.md", None),
            ("project-management/AGENTS.md", None),
            ("docs/plan/.claude/settings.json", None),
            ("docs/plan/code.py", None),
            ("docs/decisions/TEMPLATE.md", None),
            ("docs/.envrc", None),
            (
                "project-management/tasks/.gitkeep",
                Some(AmendmentPath::Record),
            ),
        ] {
            assert_eq!(amendment_path(path, &project), expected, "{path}");
        }
    }

    #[test]
    fn the_instructions_block_is_compared_byte_for_byte() {
        let block =
            "<!-- codeflow:managed:begin scaffold=3.1.0 -->\nrules\n<!-- codeflow:managed:end -->";
        let target = format!("# p\n\n{block}\n\n## Project\n\nmine\n");
        let below = format!("# p\n\n{block}\n\n## Project\n\nmine, amended\n");
        let inside = target.replace("rules", "rules!");
        let same = |a: Option<&str>, b: Option<&str>| {
            instructions_block_unchanged(a.map(str::as_bytes), b.map(str::as_bytes))
        };
        assert!(same(Some(&target), Some(&below)));
        assert!(!same(Some(&target), Some(&inside)));
        assert!(!same(Some(&target), Some("# p\n")));
        assert!(!same(None, Some(&target)));
        assert!(!same(Some(&target), None));
        assert!(same(Some("# p\n"), Some("# q\n")));
        assert!(same(None, Some("# p\n")));
        // Bytes that are not UTF-8 never decode to the target's text.
        let replacement = target.replace("rules", "rules\u{fffd}");
        let mut raw = target.clone().into_bytes();
        let at = raw.windows(5).position(|w| w == b"rules").unwrap() + 5;
        raw.insert(at, 0xFF);
        assert!(!instructions_block_unchanged(
            Some(replacement.as_bytes()),
            Some(&raw)
        ));
        assert!(instructions_block_unchanged(Some(&raw), Some(&raw)));
    }

    #[test]
    fn spike_paths_are_findings_and_the_own_record() {
        assert!(is_spike_path("docs/research/cache.md", "TSK-007"));
        assert!(is_spike_path(
            "project-management/tasks/TSK-007.md",
            "TSK-007"
        ));
        assert!(!is_spike_path(
            "project-management/tasks/TSK-008.md",
            "TSK-007"
        ));
        assert!(!is_spike_path("src/cache.rs", "TSK-007"));
    }
}
