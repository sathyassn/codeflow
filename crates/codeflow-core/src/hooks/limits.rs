//! The registry of every limit, under the name the hook plane and
//! the git helpers already use. The declarations live in the
//! `codeflow-limits` crate (TSK-1299).

pub use codeflow_limits::*;

#[cfg(test)]
mod tests {
    /// Everything that sets or spends a configured wait stays out of the
    /// code a guard runs: `QueryLimits::guard` is the only limits a guard
    /// can build. A text scan, so it pins these names and does not trace
    /// calls; `no_configured_wait_reaches_a_guard` in the CLI's
    /// `policy_authority_journey` proves the behaviour.
    #[test]
    fn guard_code_names_no_configured_wait() {
        const CONFIGURED: [&str; 7] = [
            "QueryLimits::from",
            "QueryLimits::default",
            "remote_query::ls_remote",
            "remote_query::fetch_objects",
            "release_line::advertisement",
            "release_line::ask_destination",
            "release_line::checkout_scope",
        ];
        fn scan(path: &std::path::Path, this_file: &std::path::Path, found: &mut Vec<String>) {
            if path.is_dir() {
                for entry in std::fs::read_dir(path).unwrap() {
                    scan(&entry.unwrap().path(), this_file, found);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") && path != this_file {
                let text = std::fs::read_to_string(path).unwrap();
                for name in CONFIGURED {
                    if text.contains(name) {
                        found.push(format!("{}: {name}", path.display()));
                    }
                }
            }
        }
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let this_file = core.join("src/hooks/limits.rs");
        let mut found = Vec::new();
        for root in [
            core.join("src/hooks"),
            core.join("src/security"),
            core.join("../codeflow-cli/src/cmd/hook.rs"),
        ] {
            assert!(root.exists(), "{}", root.display());
            scan(&root, &this_file, &mut found);
        }
        assert!(found.is_empty(), "{found:#?}");
    }
}
