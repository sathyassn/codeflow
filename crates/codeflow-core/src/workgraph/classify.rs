//! Path classification for pull request ranges (SPC-013 R-70, R-71, R-114).
//!
//! One embedded table, `path_sets.toml`, defines the adopter-facing path set
//! and the rest of the direct-change floor. `codeflow ci` reads it to refuse a
//! direct change on a protected surface; later rules (the journey criterion,
//! the release contract) read the same table. The planning-only and spike
//! path rules live here too, so every caller judges a range the same way.

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
}

/// The path sets of `path_sets.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct PathSets {
    /// Surfaces that reach adopters (R-114).
    pub adopter_facing: Vec<PathMember>,
    /// The rest of the direct-change floor (R-71).
    pub direct_change_floor: Vec<PathMember>,
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
    #[must_use]
    pub fn load(repo_root: &Path) -> Self {
        let (policy, _) = crate::hooks::policy::Policy::load_effective(repo_root);
        let product = policy.git.product_paths.clone().unwrap_or_else(|| {
            let stack = crate::scaffold::state::ProjectState::load(repo_root)
                .map(|state| state.stack)
                .unwrap_or_default();
            stack_product_paths(&stack)
                .iter()
                .map(ToString::to_string)
                .collect()
        });
        Self {
            product,
            watched: policy.git.breaking_watch_paths,
        }
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
    /// The adopter-facing member `path` belongs to, if any.
    #[must_use]
    pub fn adopter_facing_member(&self, path: &str, project: &ProjectPaths) -> Option<&str> {
        self.adopter_facing
            .iter()
            .find(|member| member_matches(member, path, project))
            .map(|member| member.member.as_str())
    }

    /// The member of the direct-change floor (the adopter-facing set plus
    /// policy, manifests and the record schema) that refuses a direct change
    /// to `path`, if any.
    #[must_use]
    pub fn direct_change_refusal(&self, path: &str, project: &ProjectPaths) -> Option<&str> {
        self.adopter_facing_member(path, project).or_else(|| {
            self.direct_change_floor
                .iter()
                .find(|member| member_matches(member, path, project))
                .map(|member| member.member.as_str())
        })
    }
}

/// Whether `path` is a planning path: a record under `project-management/`
/// (not the record templates, which are schema) or a plan under `docs/plan/`.
#[must_use]
pub fn is_planning_path(path: &str) -> bool {
    (path.starts_with("project-management/") && !path.starts_with("project-management/templates/"))
        || path.starts_with("docs/plan/")
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
        for member in sets.adopter_facing.iter().chain(&sets.direct_change_floor) {
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
        for member in &sets.direct_change_floor {
            let example = member.example.as_deref().unwrap();
            assert_eq!(
                sets.adopter_facing_member(example, &adopter),
                None,
                "{example}"
            );
            assert_eq!(
                sets.direct_change_refusal(example, &adopter),
                Some(member.member.as_str()),
                "{example}"
            );
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
            assert_eq!(sets.direct_change_refusal("docs/guide.md", &project), None);
        }
    }

    #[test]
    fn manifests_match_at_the_root_and_nested() {
        let sets = path_sets();
        let none = ProjectPaths::default();
        for path in [
            "Cargo.toml",
            "crates/codeflow-cli/Cargo.toml",
            "web/package-lock.json",
        ] {
            assert_eq!(
                sets.direct_change_refusal(path, &none),
                Some("dependency_manifests"),
                "{path}"
            );
        }
    }

    #[test]
    fn ordinary_docs_and_records_are_outside_the_floor() {
        let sets = path_sets();
        let project = codeflow_paths();
        for path in [
            "docs/guide/start.md",
            "README.md",
            "project-management/tasks/TSK-001.md",
            "scripts/release.py",
        ] {
            assert_eq!(sets.direct_change_refusal(path, &project), None, "{path}");
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
