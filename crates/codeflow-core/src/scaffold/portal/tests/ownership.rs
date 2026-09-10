use super::*;

fn cleanup_fixture(root: &PortalIo) -> PortalMutationPlan {
    root.write("guide/a", b"before-a").unwrap();
    root.write("guide/b", b"before-b").unwrap();
    let mut plan = PortalMutationPlan::default();
    plan.write(root, "guide/a", b"after-a".to_vec(), 100, "fixture")
        .unwrap();
    plan.write(root, "guide/b", b"after-b".to_vec(), 100, "fixture")
        .unwrap();
    prepare_portal_transaction(
        root,
        Path::new("guide"),
        &ordered_mutations(&plan.mutations),
    )
    .unwrap();
    plan
}

#[test]
fn cleanup_crash_prefixes_preserve_completed_application_and_rollback() {
    for applied in [false, true] {
        for prefix in 0..=4 {
            let temp = initialized_root();
            let io = PortalIo::open(temp.path()).unwrap();
            let plan = cleanup_fixture(&io);
            if applied {
                for mutation in plan.mutations.values() {
                    io.write(&mutation.path, mutation.after.as_ref().unwrap())
                        .unwrap();
                }
            }
            let journal = read_transaction_journal(&io, TRANSACTION_PATH).unwrap();
            let tombstone = format!(
                ".codeflow/{TRANSACTION_CLEANUP_PREFIX}{}",
                journal.transaction_id
            );
            io.rename(TRANSACTION_PATH, &tombstone).unwrap();
            if prefix >= 1 {
                io.remove(&format!("{tombstone}/after/0000")).unwrap();
            }
            if prefix >= 2 {
                io.remove(&format!("{tombstone}/after/0001")).unwrap();
            }
            if prefix >= 3 {
                io.remove_empty(&format!("{tombstone}/after")).unwrap();
            }
            if prefix >= 4 {
                io.remove(&format!("{tombstone}/manifest.json")).unwrap();
            }
            recover_portal_transaction(&io, Path::new("guide")).unwrap();
            assert!(io.inspect(&tombstone).unwrap().is_none());
            for mutation in plan.mutations.values() {
                let expected = if applied {
                    &mutation.after
                } else {
                    &mutation.before
                };
                assert_eq!(
                    io.read(&mutation.path, 100).unwrap(),
                    *expected.as_ref().unwrap()
                );
            }
        }
    }
}

#[test]
fn missing_active_output_cannot_complete_unfinished_write_or_mixed_cleanup() {
    for cleanup in [false, true] {
        let temp = initialized_root();
        let io = PortalIo::open(temp.path()).unwrap();
        cleanup_fixture(&io);
        io.write("guide/a", b"after-a").unwrap();
        let directory = if cleanup {
            let journal = read_transaction_journal(&io, TRANSACTION_PATH).unwrap();
            let tombstone = format!(
                ".codeflow/{TRANSACTION_CLEANUP_PREFIX}{}",
                journal.transaction_id
            );
            io.rename(TRANSACTION_PATH, &tombstone).unwrap();
            tombstone
        } else {
            TRANSACTION_PATH.into()
        };
        io.remove(&format!("{directory}/after/0001")).unwrap();
        assert!(recover_portal_transaction(&io, Path::new("guide")).is_err());
        assert_eq!(io.read("guide/a", 100).unwrap(), b"after-a");
        assert_eq!(io.read("guide/b", 100).unwrap(), b"before-b");
        assert!(io
            .inspect(&format!("{directory}/manifest.json"))
            .unwrap()
            .is_some());
    }
}

#[test]
fn unpublished_manifest_first_stage_allows_partial_outputs_but_preserves_unknown_bytes() {
    for written in 0..=2 {
        let temp = initialized_root();
        let io = PortalIo::open(temp.path()).unwrap();
        cleanup_fixture(&io);
        let stage = ".codeflow/.docs-portal-transaction-stage-123-456";
        io.rename(TRANSACTION_PATH, stage).unwrap();
        for index in written..2 {
            io.remove(&format!("{stage}/after/{index:04}")).unwrap();
        }
        recover_portal_transaction(&io, Path::new("guide")).unwrap();
        assert!(io.inspect(stage).unwrap().is_none());
        assert_eq!(io.read("guide/a", 100).unwrap(), b"before-a");
        assert_eq!(io.read("guide/b", 100).unwrap(), b"before-b");
    }
    for manifest in [false, true] {
        let temp = initialized_root();
        let io = PortalIo::open(temp.path()).unwrap();
        cleanup_fixture(&io);
        let stage = ".codeflow/.docs-portal-transaction-stage-123-456";
        io.rename(TRANSACTION_PATH, stage).unwrap();
        if !manifest {
            io.remove(&format!("{stage}/manifest.json")).unwrap();
        }
        io.write(&format!("{stage}/after/unknown"), b"preserve")
            .unwrap();
        assert!(recover_portal_transaction(&io, Path::new("guide")).is_err());
        assert_eq!(
            io.read(&format!("{stage}/after/unknown"), 100).unwrap(),
            b"preserve"
        );
        assert_eq!(
            io.read(&format!("{stage}/after/0000"), 100).unwrap(),
            b"after-a"
        );
    }
}

#[test]
fn successful_migration_removes_only_empty_reserved_baseline_directory() {
    let temp = initialized_root();
    setup_portal(&source("1", "before"), temp.path(), Path::new("guide")).unwrap();
    make_legacy(temp.path());
    setup_portal(&source("2", "after"), temp.path(), Path::new("guide")).unwrap();
    assert!(!temp.path().join(BASELINE_ROOT).exists());
}

#[test]
fn managed_release_identity_agrees_across_installed_state_and_generators() {
    let root = initialized_root();
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let source = DirSource::new(&assets);
    setup_portal(&source, root.path(), Path::new("guide")).unwrap();
    let state = load_state(&PortalIo::open(root.path()).unwrap())
        .unwrap()
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&source.read(MANIFEST_ASSET).unwrap()).unwrap();
    assert_eq!(state.starter_version, manifest["version"].as_str().unwrap());
    assert_eq!(state.generator, Generator::managed(&state.starter_version));
    let generator = String::from_utf8(
        source
            .read(&format!("{ASSET_PREFIX}scripts/generator.mjs"))
            .unwrap(),
    )
    .unwrap();
    assert!(generator.contains(&format!(
        "Object.freeze({{ name: \"{}\", version: \"{}\" }})",
        state.generator.name, state.generator.version
    )));
    let package: serde_json::Value =
        serde_json::from_slice(&source.read(&format!("{ASSET_PREFIX}package.json")).unwrap())
            .unwrap();
    assert_eq!(package["name"], state.generator.name);
    assert_eq!(package["version"], state.generator.version);
    let lock: serde_json::Value = serde_json::from_slice(
        &source
            .read(&format!("{ASSET_PREFIX}package-lock.json"))
            .unwrap(),
    )
    .unwrap();
    for entry in [&lock, &lock["packages"][""]] {
        assert_eq!(entry["name"], state.generator.name);
        assert_eq!(entry["version"], state.generator.version);
    }
    for consumer in ["adapter.mjs", "evidence.mjs", "browser-verify.mjs"] {
        let code = String::from_utf8(
            source
                .read(&format!("{ASSET_PREFIX}scripts/{consumer}"))
                .unwrap(),
        )
        .unwrap();
        assert!(code.contains("from \"./generator.mjs\""));
    }
}

#[test]
fn absent_legacy_baselines_allow_transfer_without_repairing_runtime() {
    for preserve_directory in [false, true] {
        let temp = initialized_root();
        let source = bundle(
            "1",
            &[
                ("kept.txt", "managed", "before"),
                ("removed.txt", "managed", "remove me"),
                ("portal.config.json", "user-owned", "{}"),
            ],
        );
        setup_portal(&source, temp.path(), Path::new("guide")).unwrap();
        make_legacy(temp.path());
        let preserved = temp.path().join("reviewed-baseline-copy");
        std::fs::rename(temp.path().join(BASELINE_ROOT), &preserved).unwrap();
        if preserve_directory {
            std::fs::create_dir(temp.path().join(BASELINE_ROOT)).unwrap();
        }
        std::fs::write(temp.path().join("guide/kept.txt"), b"project edit").unwrap();
        std::fs::remove_file(temp.path().join("guide/removed.txt")).unwrap();
        std::fs::remove_file(temp.path().join("guide/portal.config.json")).unwrap();
        let preserved_before: Vec<_> = std::fs::read_dir(&preserved)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect();
        transfer_portal(temp.path(), true).unwrap();
        assert_eq!(
            std::fs::read(temp.path().join("guide/kept.txt")).unwrap(),
            b"project edit"
        );
        assert!(!temp.path().join("guide/removed.txt").exists());
        assert!(!temp.path().join("guide/portal.config.json").exists());
        let state = load_state(&PortalIo::open(temp.path()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(state.runtime_ownership, RuntimeOwnership::Transferred);
        assert_eq!(state.starter_version, "1");
        for (name, bytes) in preserved_before {
            assert_eq!(std::fs::read(preserved.join(name)).unwrap(), bytes);
        }
    }
}

fn bundle(version: &str, files: &[(&str, &str, &str)]) -> MapSource {
    let entries: Vec<_> = files
        .iter()
        .map(|(path, ownership, _)| serde_json::json!({"path": path, "ownership": ownership}))
        .collect();
    let mut source = MapSource(BTreeMap::from([(
        MANIFEST_ASSET.into(),
        serde_json::to_vec(
            &serde_json::json!({"schema_version": 1, "version": version, "files": entries}),
        )
        .unwrap(),
    )]));
    for (path, _, content) in files {
        source
            .0
            .insert(format!("{ASSET_PREFIX}{path}"), content.as_bytes().to_vec());
    }
    source
}

fn bytes(root: &Path, relative: &str) -> Vec<u8> {
    std::fs::read(root.join(relative)).unwrap()
}

#[test]
fn lockfiles_never_line_merge_even_when_edits_are_disjoint() {
    let root = initialized_root();
    let first = bundle(
        "1",
        &[(
            "package-lock.json",
            "managed",
            "{\n\"a\": 1,\n\"b\": 1\n}\n",
        )],
    );
    setup_portal(&first, root.path(), Path::new("guide")).unwrap();
    make_legacy(root.path());
    let local = b"{\n\"a\": 2,\n\"b\": 1\n}\n";
    std::fs::write(root.path().join("guide/package-lock.json"), local).unwrap();
    let before = bytes(root.path(), STATE_PATH);
    let next = bundle(
        "2",
        &[(
            "package-lock.json",
            "managed",
            "{\n\"a\": 1,\n\"b\": 2\n}\n",
        )],
    );
    let report = setup_portal(&next, root.path(), Path::new("guide")).unwrap();
    assert!(report.has_conflicts());
    assert_eq!(report.count(Action::Merged), 0);
    assert_eq!(bytes(root.path(), "guide/package-lock.json"), local);
    assert_eq!(bytes(root.path(), STATE_PATH), before);
}

#[test]
fn a_late_conflict_discards_earlier_repairs_upgrades_and_state() {
    let root = initialized_root();
    let first = bundle(
        "1",
        &[
            ("a.txt", "managed", "old"),
            ("b.txt", "managed", "old"),
            ("z.txt", "managed", "old"),
        ],
    );
    setup_portal(&first, root.path(), Path::new("guide")).unwrap();
    std::fs::remove_file(root.path().join("guide/a.txt")).unwrap();
    std::fs::write(root.path().join("guide/z.txt"), "custom").unwrap();
    let state = bytes(root.path(), STATE_PATH);
    let next = bundle(
        "2",
        &[
            ("a.txt", "managed", "new"),
            ("b.txt", "managed", "new"),
            ("z.txt", "managed", "new"),
        ],
    );
    for _ in 0..2 {
        let report = setup_portal(&next, root.path(), Path::new("guide")).unwrap();
        assert!(report.has_conflicts());
        assert!(!report.files.iter().any(|file| matches!(
            file.action,
            Action::Added | Action::Changed | Action::Created | Action::Removed
        )));
        assert!(!root.path().join("guide/a.txt").exists());
        assert_eq!(bytes(root.path(), "guide/b.txt"), b"old");
        assert_eq!(bytes(root.path(), "guide/z.txt"), b"custom");
        assert_eq!(bytes(root.path(), STATE_PATH), state);
        assert!(!root.path().join(TRANSACTION_PATH).exists());
    }
}

#[test]
fn missing_managed_repairs_but_project_owned_paths_never_reseed_or_revert() {
    let root = initialized_root();
    let first = bundle(
        "1",
        &[
            ("runtime", "managed", "old"),
            ("config", "user-owned", "mine"),
        ],
    );
    setup_portal(&first, root.path(), Path::new("guide")).unwrap();
    std::fs::remove_file(root.path().join("guide/runtime")).unwrap();
    std::fs::remove_file(root.path().join("guide/config")).unwrap();
    let next = bundle(
        "2",
        &[
            ("runtime", "managed", "new"),
            ("new-config", "user-owned", "default"),
        ],
    );
    assert!(!setup_portal(&next, root.path(), Path::new("guide"))
        .unwrap()
        .has_conflicts());
    assert_eq!(bytes(root.path(), "guide/runtime"), b"new");
    assert!(!root.path().join("guide/config").exists());
    assert!(!root.path().join("guide/new-config").exists());
    let reclaim = bundle(
        "3",
        &[
            ("runtime", "managed", "new"),
            ("config", "managed", "upstream"),
        ],
    );
    assert!(!setup_portal(&reclaim, root.path(), Path::new("guide"))
        .unwrap()
        .has_conflicts());
    assert!(!root.path().join("guide/config").exists());
    assert_eq!(
        load_state(&PortalIo::open(root.path()).unwrap())
            .unwrap()
            .unwrap()
            .files["config"]
            .ownership,
        "user-owned"
    );
}

#[test]
fn incoming_identical_bytes_are_accepted_but_unknown_collisions_stop_everything() {
    let root = initialized_root();
    setup_portal(&source("1", "old"), root.path(), Path::new("guide")).unwrap();
    std::fs::write(root.path().join("guide/managed.txt"), "new").unwrap();
    assert!(
        !setup_portal(&source("2", "new"), root.path(), Path::new("guide"))
            .unwrap()
            .has_conflicts()
    );
    std::fs::write(root.path().join("guide/new-file"), "mine").unwrap();
    let state = bytes(root.path(), STATE_PATH);
    let next = bundle(
        "3",
        &[
            ("managed.txt", "managed", "third"),
            ("new-file", "managed", "upstream"),
        ],
    );
    assert!(setup_portal(&next, root.path(), Path::new("guide"))
        .unwrap()
        .has_conflicts());
    assert_eq!(bytes(root.path(), "guide/managed.txt"), b"new");
    assert_eq!(bytes(root.path(), "guide/new-file"), b"mine");
    assert_eq!(bytes(root.path(), STATE_PATH), state);
}

#[test]
fn edited_retirement_blocks_then_clean_retirement_removes() {
    let root = initialized_root();
    setup_portal(&source("1", "old"), root.path(), Path::new("guide")).unwrap();
    std::fs::write(root.path().join("guide/managed.txt"), "mine").unwrap();
    let state = bytes(root.path(), STATE_PATH);
    let next = bundle("2", &[("replacement", "managed", "next")]);
    assert!(setup_portal(&next, root.path(), Path::new("guide"))
        .unwrap()
        .has_conflicts());
    assert!(!root.path().join("guide/replacement").exists());
    assert_eq!(bytes(root.path(), STATE_PATH), state);
    std::fs::write(root.path().join("guide/managed.txt"), "old").unwrap();
    assert!(!setup_portal(&next, root.path(), Path::new("guide"))
        .unwrap()
        .has_conflicts());
    assert!(!root.path().join("guide/managed.txt").exists());
    assert_eq!(bytes(root.path(), "guide/replacement"), b"next");
}

#[test]
fn transfer_v1_and_v2_preserves_edits_deletions_and_older_release_provenance() {
    for legacy in [false, true] {
        let root = initialized_root();
        let first = bundle(
            "1",
            &[
                ("edited", "managed", "old"),
                ("deleted", "managed", "old"),
                ("config", "user-owned", "mine"),
            ],
        );
        setup_portal(&first, root.path(), Path::new("guide")).unwrap();
        if legacy {
            make_legacy(root.path());
        }
        let old = load_state(&PortalIo::open(root.path()).unwrap())
            .unwrap()
            .unwrap();
        std::fs::write(root.path().join("guide/edited"), "custom").unwrap();
        std::fs::remove_file(root.path().join("guide/deleted")).unwrap();
        std::fs::remove_file(root.path().join("guide/config")).unwrap();
        let before = bytes(root.path(), STATE_PATH);
        assert!(transfer_portal(root.path(), false).is_err());
        assert_eq!(bytes(root.path(), STATE_PATH), before);
        let report = transfer_portal(root.path(), true).unwrap();
        assert!(!report.has_conflicts());
        let transferred = load_state(&PortalIo::open(root.path()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(transferred.schema_version, 2);
        assert_eq!(transferred.runtime_ownership, RuntimeOwnership::Transferred);
        assert_eq!(transferred.starter_version, "1");
        assert_eq!(
            serde_json::to_value(&transferred.files).unwrap(),
            serde_json::to_value(&old.files).unwrap()
        );
        let frozen = bytes(root.path(), STATE_PATH);
        std::fs::write(root.path().join("guide/edited"), "more edits").unwrap();
        let next = bundle(
            "99",
            &[
                ("edited", "managed", "new"),
                ("deleted", "managed", "new"),
                ("config", "user-owned", "new"),
                ("new-runtime", "managed", "new"),
            ],
        );
        setup_portal(&next, root.path(), Path::new("guide")).unwrap();
        update_adopted_portal(&next, root.path()).unwrap();
        transfer_portal(root.path(), true).unwrap();
        assert_eq!(bytes(root.path(), STATE_PATH), frozen);
        assert_eq!(bytes(root.path(), "guide/edited"), b"more edits");
        for absent in ["deleted", "config", "new-runtime"] {
            assert!(!root.path().join("guide").join(absent).exists());
        }
        assert_eq!(
            report
                .files
                .iter()
                .filter(|file| file.action == Action::Removed)
                .count(),
            usize::from(legacy)
        );
    }
}

#[test]
fn transferred_update_does_not_consult_incoming_assets() {
    struct NoAssets;
    impl AssetSource for NoAssets {
        fn read(&self, _: &str) -> Option<Vec<u8>> {
            panic!("transferred runtime must not read incoming assets");
        }
    }
    let root = initialized_root();
    setup_portal(&source("1", "old"), root.path(), Path::new("guide")).unwrap();
    transfer_portal(root.path(), true).unwrap();
    assert!(update_adopted_portal(&NoAssets, root.path())
        .unwrap()
        .is_some());
    setup_portal(&NoAssets, root.path(), Path::new("guide")).unwrap();
}

#[test]
fn absent_legacy_baselines_are_benign_but_unknown_hash_named_entries_are_not() {
    for missing_directory in [false, true] {
        let root = initialized_root();
        setup_portal(&source("1", "old"), root.path(), Path::new("guide")).unwrap();
        make_legacy(root.path());
        std::fs::remove_file(root.path().join(baseline_path(&sha256_hex(b"old")))).unwrap();
        if missing_directory {
            std::fs::remove_dir(root.path().join(BASELINE_ROOT)).unwrap();
        }
        assert!(
            !setup_portal(&source("2", "new"), root.path(), Path::new("guide"))
                .unwrap()
                .has_conflicts()
        );
    }
    let root = initialized_root();
    setup_portal(&source("1", "old"), root.path(), Path::new("guide")).unwrap();
    make_legacy(root.path());
    let unknown = baseline_path(&sha256_hex(b"unrecorded"));
    std::fs::write(root.path().join(&unknown), b"unrecorded").unwrap();
    let before = bytes(root.path(), STATE_PATH);
    assert!(transfer_portal(root.path(), true).is_err());
    assert!(setup_portal(&source("2", "new"), root.path(), Path::new("guide")).is_err());
    assert_eq!(bytes(root.path(), STATE_PATH), before);
    assert_eq!(bytes(root.path(), &unknown), b"unrecorded");
}

#[test]
fn migration_checks_runtime_when_legacy_metadata_already_claims_new_release() {
    let root = initialized_root();
    setup_portal(&source("2", "new"), root.path(), Path::new("guide")).unwrap();
    make_legacy(root.path());
    std::fs::write(root.path().join("guide/managed.txt"), "older custom bytes").unwrap();
    let state = bytes(root.path(), STATE_PATH);
    assert!(
        setup_portal(&source("2", "new"), root.path(), Path::new("guide"))
            .unwrap()
            .has_conflicts()
    );
    assert_eq!(bytes(root.path(), STATE_PATH), state);
    assert_eq!(
        bytes(root.path(), "guide/managed.txt"),
        b"older custom bytes"
    );
}

#[test]
fn legacy_recovery_precedes_full_state_parsing_on_setup_and_update() {
    for setup in [false, true] {
        let root = initialized_root();
        setup_portal(&source("1", "old"), root.path(), Path::new("guide")).unwrap();
        make_legacy(root.path());
        let valid = bytes(root.path(), STATE_PATH);
        let mut before: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        before["future_field"] = true.into();
        std::fs::write(
            root.path().join(STATE_PATH),
            serde_json::to_vec(&before).unwrap(),
        )
        .unwrap();
        let mut plan = PortalMutationPlan::default();
        plan.write(
            &PortalIo::open(root.path()).unwrap(),
            "guide/managed.txt",
            b"old".to_vec(),
            100,
            "fixture",
        )
        .unwrap();
        plan.write(
            &PortalIo::open(root.path()).unwrap(),
            STATE_PATH,
            valid,
            MAX_STATE_BYTES,
            "fixture",
        )
        .unwrap();
        assert!(plan
            .commit_with_abrupt_fault_after(
                &PortalIo::open(root.path()).unwrap(),
                Path::new("guide"),
                0
            )
            .is_err());
        let report = if setup {
            setup_portal(&source("2", "new"), root.path(), Path::new("guide")).unwrap()
        } else {
            update_adopted_portal(&source("2", "new"), root.path())
                .unwrap()
                .unwrap()
        };
        assert!(!report.has_conflicts());
        assert_eq!(
            load_state(&PortalIo::open(root.path()).unwrap())
                .unwrap()
                .unwrap()
                .schema_version,
            2
        );
        assert_eq!(bytes(root.path(), "guide/managed.txt"), b"new");
        assert!(!root.path().join(TRANSACTION_PATH).exists());
    }
}

#[test]
fn baseline_removal_live_failure_rolls_back_but_abrupt_failure_recovers_forward() {
    for abrupt in [false, true] {
        let root = initialized_root();
        setup_portal(&source("1", "old"), root.path(), Path::new("guide")).unwrap();
        make_legacy(root.path());
        let before = bytes(root.path(), STATE_PATH);
        let mut state = load_state(&PortalIo::open(root.path()).unwrap())
            .unwrap()
            .unwrap();
        state.schema_version = 2;
        let mut plan = PortalMutationPlan::default();
        plan_state(&PortalIo::open(root.path()).unwrap(), &state, &mut plan).unwrap();
        let baseline = baseline_path(&sha256_hex(b"old"));
        plan.remove(
            &PortalIo::open(root.path()).unwrap(),
            &baseline,
            MAX_MANAGED_FILE_BYTES,
            "fixture",
        )
        .unwrap();
        if abrupt {
            assert!(plan
                .commit_with_abrupt_fault_after(
                    &PortalIo::open(root.path()).unwrap(),
                    Path::new("guide"),
                    1
                )
                .is_err());
            recover_portal_transaction(&PortalIo::open(root.path()).unwrap(), Path::new("guide"))
                .unwrap();
            assert!(!root.path().join(&baseline).exists());
            assert_eq!(
                load_state(&PortalIo::open(root.path()).unwrap())
                    .unwrap()
                    .unwrap()
                    .schema_version,
                2
            );
        } else {
            assert!(plan
                .commit_with_fault_after(
                    &PortalIo::open(root.path()).unwrap(),
                    Path::new("guide"),
                    1
                )
                .is_err());
            assert_eq!(bytes(root.path(), STATE_PATH), before);
            assert_eq!(bytes(root.path(), &baseline), b"old");
        }
    }
}
