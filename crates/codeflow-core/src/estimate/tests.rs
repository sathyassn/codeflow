use super::{check_forecast, sources::sha256, ForecastReport, SourceAssurance};
use serde_json::{json, Value};
use std::path::Path;

#[test]
fn canonical_identity_claims_cannot_hide_under_an_alternate_filename() {
    let mut fixture = Fixture::new();
    fixture.canonical();
    let bytes = std::fs::read(
        fixture
            .dir
            .path()
            .join("project-management/tasks/TSK-001.md"),
    )
    .unwrap();
    write(
        fixture.dir.path(),
        "project-management/tasks/TSK-002.md",
        bytes,
    );
    assert!(
        !fixture.check().is_valid(),
        "alternate filename must not hide a duplicate identity"
    );
}

#[test]
fn canonical_identity_rejects_conflicting_alias_even_with_matching_digest() {
    let mut fixture = Fixture::new();
    fixture.canonical();
    let bytes = "---\nid: TSK-001\nformat_id: TSK-002\nepic_id: null\nspecs: []\ndepends_on: []\n---\nTask.";
    write(
        fixture.dir.path(),
        "project-management/tasks/TSK-001.md",
        bytes,
    );
    fixture.input["packages"][0]["source"]["sha256"] = json!(sha256(bytes.as_bytes()));
    assert!(
        !fixture.check().is_valid(),
        "conflicting canonical alias must fail"
    );
}

fn linked_identity_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let task = "---\nid: TSK-001\nepic_id: EPC-001\nspecs: [SPC-001]\ndepends_on: []\n---\nTask.";
    let epic = "---\nid: EPC-001\nspecs: [SPC-001]\n---\nEpic.";
    let spec = "---\nid: SPC-001\n---\nSpec.";
    write(
        fixture.dir.path(),
        "project-management/tasks/TSK-001.md",
        task,
    );
    write(
        fixture.dir.path(),
        "project-management/epics/EPC-001.md",
        epic,
    );
    write(
        fixture.dir.path(),
        "project-management/specs/SPC-001.md",
        spec,
    );
    fixture.input["packages"][0]["source"] =
        json!({"kind":"codeflow_task","task_id":"TSK-001","sha256":sha256(task.as_bytes())});
    fixture.input["packages"][0]["context_pins"] =
        json!([pin("project-management/specs/SPC-001.md", spec)]);
    fixture
}

#[test]
fn referenced_epic_and_spec_identity_claims_are_checked_across_filenames() {
    for (kind, prefix, code) in [
        ("epics", "EPC", "epic_resolution"),
        ("specs", "SPC", "spec_resolution"),
    ] {
        let fixture = linked_identity_fixture();
        assert!(fixture.check().is_valid());
        let bytes = std::fs::read(
            fixture
                .dir
                .path()
                .join(format!("project-management/{kind}/{prefix}-001.md")),
        )
        .unwrap();
        write(
            fixture.dir.path(),
            &format!("project-management/{kind}/{prefix}-002.md"),
            bytes,
        );
        has(&fixture.check(), code);
    }
}

#[test]
fn referenced_epic_and_spec_conflicting_aliases_fail_while_legacy_aliases_pass() {
    for (kind, prefix) in [("epics", "EPC"), ("specs", "SPC")] {
        let mut fixture = linked_identity_fixture();
        let path = format!("project-management/{kind}/{prefix}-001.md");
        let original = std::fs::read_to_string(fixture.dir.path().join(&path)).unwrap();
        let conflicting = original.replace(
            &format!("id: {prefix}-001"),
            &format!("id: {prefix}-001\nformat_id: {prefix}-002"),
        );
        write(fixture.dir.path(), &path, &conflicting);
        if kind == "specs" {
            fixture.input["packages"][0]["context_pins"] = json!([pin(&path, &conflicting)]);
        }
        has(&fixture.check(), "source_record");
        let legacy = original.replace(
            &format!("id: {prefix}-001"),
            &format!("id: historical-{prefix}\nformat_id: {prefix}-001"),
        );
        write(fixture.dir.path(), &path, &legacy);
        if kind == "specs" {
            fixture.input["packages"][0]["context_pins"] = json!([pin(&path, &legacy)]);
        }
        assert!(
            fixture.check().is_valid(),
            "valid legacy {prefix} alias must resolve"
        );
    }
}

#[test]
fn empty_allocation_has_zero_totals_and_no_canonical_delivery_claim() {
    let mut fixture = Fixture::new();
    fixture.input["packages"] = json!([]);
    fixture.input["resources"] = json!([]);
    for scenario in fixture.input["scenarios"].as_array_mut().unwrap() {
        scenario["activities"] = json!([]);
        scenario["milestones"] = json!([]);
    }
    let report = fixture.check();
    assert!(report.is_valid(), "{:?}", report.findings);
    assert_eq!(report.checked_package_count, 0);
    assert_eq!(report.source_assurance, SourceAssurance::Limited);
    assert!(report.scenarios.iter().all(|scenario| scenario.valid
        && scenario.elapsed_seconds == Some(0)
        && scenario.resources.is_empty()
        && scenario.milestones.is_empty()));
    assert!(report.limitation.contains("predictive accuracy"));
}

struct Fixture {
    dir: tempfile::TempDir,
    input: Value,
}

fn write(root: &Path, path: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn pin(path: &str, bytes: &str) -> Value {
    json!({"path":path,"sha256":sha256(bytes.as_bytes())})
}

fn activity(id: &str, stage: &str, start: u64, duration: u64, after: &[&str]) -> Value {
    json!({"id":id,"package_id":"p1","stage":stage,"start_seconds":start,"duration_seconds":duration,
        "demands":[{"resource_id":"agent","units":1}],"after":after,"after_boundaries":[],"basis":"Explicit judgment; fictional fixture."})
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "profile.md", "profile\n");
        write(dir.path(), "rubric.md", "rubric\n");
        write(dir.path(), "brief.md", "brief\n");
        let activities = json!([
            activity("implement", "implement", 0, 10, &[]),
            activity("review", "review", 10, 2, &["implement"]),
            activity("verify", "verify", 12, 3, &["review"]),
            activity("rework", "rework", 15, 1, &["verify"])
        ]);
        let scenarios=["favorable","planning","adverse"].iter().map(|name|json!({"name":name,"assumptions":["Explicit fixture allocation."],"activities":activities,"milestones":[{"id":"done","after":["rework"]}]})).collect::<Vec<_>>();
        let input = json!({"schema_version":1,"id":"test","anchor_epoch_seconds":0,"horizon_seconds":1000,
            "profile":pin("profile.md","profile\n"),"rubric":pin("rubric.md","rubric\n"),
            "packages":[{"id":"p1","source":{"kind":"declared","pin":pin("brief.md","brief\n"),"reference":"Fixture brief"},"context_pins":[],"grade":"easy","grade_evidence":["Explicit fixture scope."],"duration_basis":"judgment","stage_exclusions":[]}],
            "boundaries":[],"resources":[{"id":"agent","capacity":1,"windows":[{"start_seconds":0,"end_seconds":1000}]}],"scenarios":scenarios});
        Self { dir, input }
    }
    fn check(&self) -> ForecastReport {
        write(
            self.dir.path(),
            "forecast.json",
            serde_json::to_vec(&self.input).unwrap(),
        );
        check_forecast(self.dir.path(), Path::new("forecast.json"))
    }
    fn canonical(&mut self) {
        let bytes =
            "---\nid: TSK-001\nepic_id: null\nspecs: []\ndepends_on: []\n---\nAcceptance body.\n";
        write(
            self.dir.path(),
            "project-management/tasks/TSK-001.md",
            bytes,
        );
        self.input["packages"][0]["source"] =
            json!({"kind":"codeflow_task","task_id":"TSK-001","sha256":sha256(bytes.as_bytes())});
    }
    fn task(&mut self, id: &str, dependencies: &[&str], boundary: bool) {
        let bytes = format!(
            "---\nid: {id}\nspecs: []\ndepends_on: {}\n---\nRecord body.\n",
            serde_json::to_string(dependencies).unwrap()
        );
        write(
            self.dir.path(),
            &format!("project-management/tasks/{id}.md"),
            &bytes,
        );
        let source = json!({"kind":"codeflow_task","task_id":id,"sha256":sha256(bytes.as_bytes())});
        if boundary {
            self.input["boundaries"].as_array_mut().unwrap().push(json!({"id":"b1","source":source,"evidence":"Explicit availability, not status.","availability":[{"scenario":"favorable","available_at_seconds":0},{"scenario":"planning","available_at_seconds":0},{"scenario":"adverse","available_at_seconds":0}]}));
        } else {
            self.input["packages"][0]["source"] = source;
        }
    }
}

fn has(report: &ForecastReport, code: &str) {
    assert!(
        report.findings.iter().any(|f| f.code == code),
        "expected {code}: {report:?}"
    );
}

#[test]
fn valid_declared_forecast_separates_totals_and_reports_limited_assurance() {
    let f = Fixture::new();
    let r = f.check();
    assert!(r.is_valid(), "{r:?}");
    assert_eq!(r.source_assurance, SourceAssurance::Limited);
    assert_eq!(r.checked_package_count, 1);
    assert_eq!(r.source_digests.len(), 3);
    for s in r.scenarios {
        assert!(s.valid);
        assert_eq!(s.elapsed_seconds, Some(16));
        assert_eq!(s.resources[0].consumption_seconds, 16);
        assert_eq!(s.milestones[0].at_seconds, 16);
    }
    assert!(!f.dir.path().join("project-management").exists());
}

#[test]
fn strict_schema_rejects_unknown_and_missing_fields_at_nested_levels() {
    let mutations: [fn(&mut Value); 8] = [
        |v| v["unknown"] = json!(true),
        |v| v["profile"]["extra"] = json!(0),
        |v| v["packages"][0]["source"]["extra"] = json!(0),
        |v| v["resources"][0]["windows"][0]["extra"] = json!(0),
        |v| {
            v["scenarios"][0]["activities"][0]
                .as_object_mut()
                .unwrap()
                .remove("package_id");
        },
        |v| v["scenarios"][0]["activities"][0]["demands"][0]["extra"] = json!(0),
        |v| v["scenarios"][0]["milestones"][0]["extra"] = json!(0),
        |v| v["packages"][0]["source"]["kind"] = json!("remote_api"),
    ];
    for mutate in mutations {
        let mut f = Fixture::new();
        mutate(&mut f.input);
        has(&f.check(), "invalid_json");
    }
}

#[test]
fn duplicate_keys_are_rejected_even_in_tagged_sources() {
    let f = Fixture::new();
    let original = serde_json::to_string(&f.input).unwrap();
    for (old, new) in [
        (
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        ),
        (
            "\"kind\":\"declared\"",
            "\"kind\":\"declared\",\"kind\":\"declared\"",
        ),
        (
            "\"reference\":\"Fixture brief\"",
            "\"reference\":\"Fixture brief\",\"reference\":\"Fixture brief\"",
        ),
        ("\"capacity\":1", "\"capacity\":1,\"capacity\":1"),
    ] {
        let input = original.replace(old, new);
        assert_ne!(input, original);
        write(f.dir.path(), "forecast.json", input);
        has(
            &check_forecast(f.dir.path(), Path::new("forecast.json")),
            "invalid_json",
        );
    }
}

#[test]
fn numbers_are_unsigned_integral_and_version_is_closed() {
    for value in [json!(-1), json!(true), json!(1.5), json!("10")] {
        let mut f = Fixture::new();
        f.input["scenarios"][0]["activities"][0]["duration_seconds"] = value;
        has(&f.check(), "invalid_json");
    }
    let mut f = Fixture::new();
    f.input["schema_version"] = json!(2);
    has(&f.check(), "unsupported_version");
}

#[test]
fn shape_bounds_reject_before_source_reads() {
    let mutations: [fn(&mut Value); 8] = [
        |v| v["id"] = json!("x".repeat(129)),
        |v| v["horizon_seconds"] = json!(366 * 86400 + 1),
        |v| v["resources"][0]["capacity"] = json!(0),
        |v| v["scenarios"][1]["name"] = json!("favorable"),
        |v| v["packages"][0]["grade_evidence"] = json!([]),
        |v| v["scenarios"][0]["activities"][0]["demands"] = json!([]),
        |v| v["scenarios"][0]["activities"][0]["after"] = json!(["review", "review"]),
        |v| v["packages"][0]["context_pins"] = json!(vec![pin("brief.md", "brief\n"); 257]),
    ];
    for mutate in mutations {
        let mut f = Fixture::new();
        mutate(&mut f.input);
        let r = f.check();
        has(&r, "invalid_field");
        assert!(r.source_digests.is_empty());
    }
}

#[test]
fn stale_whole_file_pin_and_source_read_fail_closed_for_every_scenario() {
    let mut f = Fixture::new();
    f.canonical();
    write(
        f.dir.path(),
        "project-management/tasks/TSK-001.md",
        "---\nid: TSK-001\n---\nChanged body.",
    );
    let r = f.check();
    has(&r, "stale_pin");
    for s in r.scenarios {
        assert!(!s.valid);
        assert!(s.elapsed_seconds.is_none());
        assert!(s.resources.is_empty());
    }
    std::fs::remove_file(f.dir.path().join("profile.md")).unwrap();
    has(&f.check(), "source_read");
}

#[test]
fn canonical_task_and_legacy_duplicate_resolution() {
    let mut f = Fixture::new();
    f.canonical();
    assert_eq!(f.check().source_assurance, SourceAssurance::CanonicalLocal);
    let path = "project-management/tasks/TSK-001.md";
    let bytes = std::fs::read(f.dir.path().join(path)).unwrap();
    write(
        f.dir.path(),
        "project-management/epics/EPC-001/tasks/TSK-001.md",
        bytes,
    );
    has(&f.check(), "task_resolution");
}

#[test]
fn legacy_task_alias_and_nested_layout_are_read_only_compatible() {
    let mut f = Fixture::new();
    let bytes =
        "---\nid: task-old\nformat_id: TSK-001-002\nspecs: []\ndependencies: []\n---\nOld body.\n";
    write(
        f.dir.path(),
        "project-management/epics/EPC-001/tasks/TSK-001-002.md",
        bytes,
    );
    f.input["packages"][0]["source"] =
        json!({"kind":"codeflow_task","task_id":"TSK-001-002","sha256":sha256(bytes.as_bytes())});
    assert!(f.check().is_valid());
}

#[test]
fn inherited_and_direct_specs_require_context_pins() {
    let mut f = Fixture::new();
    let bytes = "---\nid: TSK-001\nepic_id: EPC-001\nspecs: [SPC-001]\ndepends_on: []\n---\nTask.";
    write(f.dir.path(), "project-management/tasks/TSK-001.md", bytes);
    f.input["packages"][0]["source"] =
        json!({"kind":"codeflow_task","task_id":"TSK-001","sha256":sha256(bytes.as_bytes())});
    write(
        f.dir.path(),
        "project-management/epics/EPC-001.md",
        "---\nid: EPC-001\nspecs: [SPC-002]\n---\nEpic.",
    );
    let one = "---\nid: SPC-001\n---\nDirect.";
    let two = "---\nid: SPC-002\n---\nInherited.";
    write(f.dir.path(), "project-management/specs/SPC-001.md", one);
    write(f.dir.path(), "project-management/specs/SPC-002.md", two);
    has(&f.check(), "missing_context_pin");
    f.input["packages"][0]["context_pins"] = json!([
        pin("project-management/specs/SPC-001.md", one),
        pin("project-management/specs/SPC-002.md", two)
    ]);
    assert!(f.check().is_valid());
}

#[test]
fn canonical_boundaries_are_explicit_and_never_derive_availability_from_status() {
    let mut f = Fixture::new();
    f.task("TSK-001", &["TSK-002"], false);
    has(&f.check(), "missing_predecessor");
    f.task("TSK-002", &[], true);
    assert!(f.check().is_valid());
    f.input["boundaries"][0]["availability"][0]["available_at_seconds"] = json!(10);
    has(&f.check(), "boundary_precedence");
    f.input["boundaries"] = json!([]);
    f.task("TSK-002", &["TSK-001"], true);
    has(&f.check(), "contradictory_boundary");
}

#[test]
fn external_boundary_references_constrain_allocations() {
    let mut f = Fixture::new();
    let source = f.input["packages"][0]["source"].clone();
    f.input["boundaries"] = json!([{"id":"b","source":source,"evidence":"Human decision slot.","availability":[{"scenario":"favorable","available_at_seconds":1},{"scenario":"planning","available_at_seconds":0},{"scenario":"adverse","available_at_seconds":0}]}]);
    f.input["scenarios"][0]["activities"][0]["after_boundaries"] = json!(["b"]);
    has(&f.check(), "boundary_precedence");
    f.input["scenarios"][0]["activities"][0]["after_boundaries"] = json!(["missing"]);
    has(&f.check(), "unknown_boundary");
}

#[test]
fn activity_cycles_unknown_references_and_milestone_refs_fail() {
    let mut f = Fixture::new();
    f.input["scenarios"][0]["activities"][0]["after"] = json!(["rework"]);
    has(&f.check(), "activity_cycle");
    f.input["scenarios"][0]["activities"][0]["after"] = json!(["missing"]);
    has(&f.check(), "unknown_activity");
    f.input["scenarios"][0]["milestones"][0]["after"] = json!(["missing"]);
    has(&f.check(), "unknown_milestone_activity");
}

#[test]
fn missing_stages_exclusions_and_unsized_discovery_are_distinct() {
    let mut f = Fixture::new();
    for s in f.input["scenarios"].as_array_mut().unwrap() {
        s["activities"].as_array_mut().unwrap().pop();
        s["milestones"] = json!([]);
    }
    has(&f.check(), "missing_stage");
    f.input["packages"][0]["stage_exclusions"] =
        json!([{"stage":"rework","reason":"Rework included in implementation range."}]);
    assert!(f.check().is_valid());
    assert_eq!(f.check().exclusions.len(), 1);
    f.input["packages"][0]["grade"] = json!("unsized");
    has(&f.check(), "unsized_implementation");
    f.input["packages"][0]["stage_exclusions"] = json!([{"stage":"implement","reason":"Discovery only."},{"stage":"review","reason":"Discovery only."},{"stage":"verify","reason":"Discovery only."},{"stage":"rework","reason":"Discovery only."}]);
    for s in f.input["scenarios"].as_array_mut().unwrap() {
        s["activities"] = json!([activity("probe", "discovery", 0, 10, &[])]);
    }
    assert!(f.check().is_valid());
}

#[test]
fn resource_seconds_are_not_elapsed_seconds_and_equal_endpoints_are_valid() {
    let mut f = Fixture::new();
    f.input["resources"][0]["capacity"] = json!(2);
    for s in f.input["scenarios"].as_array_mut().unwrap() {
        for a in s["activities"].as_array_mut().unwrap() {
            a["demands"][0]["units"] = json!(2);
        }
    }
    let r = f.check();
    assert!(r.is_valid());
    assert_eq!(r.scenarios[0].elapsed_seconds, Some(16));
    assert_eq!(r.scenarios[0].resources[0].consumption_seconds, 32);
}

#[test]
fn resource_calendar_accepts_abutting_windows_but_rejects_a_real_gap() {
    let mut fixture = Fixture::new();
    for scenario in fixture.input["scenarios"].as_array_mut().unwrap() {
        scenario["activities"][0]["start_seconds"] = json!(50);
        scenario["activities"][0]["duration_seconds"] = json!(100);
        for (index, start) in [(1, 150), (2, 152), (3, 155)] {
            scenario["activities"][index]["start_seconds"] = json!(start);
        }
    }
    for windows in [
        json!([{"start_seconds":0,"end_seconds":100},{"start_seconds":100,"end_seconds":200}]),
        json!([{"start_seconds":0,"end_seconds":60},{"start_seconds":60,"end_seconds":100},{"start_seconds":100,"end_seconds":200}]),
    ] {
        fixture.input["resources"][0]["windows"] = windows;
        let report = fixture.check();
        assert!(
            report.is_valid(),
            "abutting windows are continuous: {:?}",
            report.findings
        );
        assert_eq!(report.scenarios[0].elapsed_seconds, Some(156));
    }
    fixture.input["resources"][0]["windows"][1]["start_seconds"] = json!(61);
    has(&fixture.check(), "resource_calendar");
}

#[test]
fn resource_capacity_findings_are_bounded_per_resource_and_scenario() {
    let mut fixture = Fixture::new();
    let mut second = fixture.input["resources"][0].clone();
    second["id"] = json!("reviewer");
    fixture.input["resources"]
        .as_array_mut()
        .unwrap()
        .push(second);
    let activities = (0..256)
        .map(|i| {
            let stage = ["implement", "review", "verify", "rework"][i % 4];
            let mut value = activity(&format!("dense-{i}"), stage, 0, 10, &[]);
            value["demands"]
                .as_array_mut()
                .unwrap()
                .push(json!({"resource_id":"reviewer","units":1}));
            value
        })
        .collect::<Vec<_>>();
    for scenario in fixture.input["scenarios"].as_array_mut().unwrap() {
        scenario["activities"] = json!(activities);
        scenario["milestones"] = json!([]);
    }
    assert!(serde_json::to_vec(&fixture.input).unwrap().len() <= 1024 * 1024);
    let report = fixture.check();
    assert_eq!(
        report.findings.len(),
        6,
        "one violation per resource/scenario"
    );
    assert!(report
        .findings
        .iter()
        .all(|finding| finding.code == "resource_capacity"));
    assert!(report.scenarios.iter().all(|scenario| !scenario.valid
        && scenario.elapsed_seconds.is_none()
        && scenario.resources.is_empty()
        && scenario.milestones.is_empty()));
}

#[test]
fn shared_resources_and_calendar_gaps_are_enforced() {
    let mut f = Fixture::new();
    f.input["scenarios"][0]["activities"][1]["start_seconds"] = json!(9);
    has(&f.check(), "resource_capacity");
    has(&f.check(), "activity_precedence");
    f.input["resources"][0]["windows"] =
        json!([{"start_seconds":0,"end_seconds":5},{"start_seconds":6,"end_seconds":1000}]);
    has(&f.check(), "resource_calendar");
}

#[test]
fn arithmetic_and_horizon_overflows_never_produce_valid_totals() {
    let mut f = Fixture::new();
    f.input["anchor_epoch_seconds"] = json!(u64::MAX);
    has(&f.check(), "invalid_field");
    f.input["anchor_epoch_seconds"] = json!(0);
    f.input["scenarios"][0]["activities"][0]["duration_seconds"] = json!(u64::MAX);
    f.input["scenarios"][0]["activities"][0]["start_seconds"] = json!(1);
    has(&f.check(), "activity_interval");
    f.input["scenarios"][0]["activities"][0]["duration_seconds"] = json!(10);
    f.input["scenarios"][0]["activities"][0]["start_seconds"] = json!(0);
    f.input["scenarios"][0]["activities"][0]["demands"][0]["units"] = json!(u64::MAX);
    has(&f.check(), "resource_overflow");
}

#[test]
fn unsafe_paths_and_error_text_do_not_leak_untrusted_source_values() {
    for path in [
        "../outside",
        "/etc/passwd",
        ".env",
        ".aws/credentials",
        ".codex/auth.json",
        "private.pem",
        "docs\\brief.md",
        "C:/secret",
        "docs/CON",
        "docs/file:stream",
    ] {
        let mut f = Fixture::new();
        f.input["profile"]["path"] = json!(path);
        let r = f.check();
        has(&r, "unsafe_source_path");
        assert!(!serde_json::to_string(&r.findings).unwrap().contains(path));
    }
    let f = Fixture::new();
    write(
        f.dir.path(),
        "forecast.json",
        "{\"HIGHLY_PRIVATE_INPUT\":true}",
    );
    let r = check_forecast(f.dir.path(), Path::new("forecast.json"));
    assert!(!serde_json::to_string(&r)
        .unwrap()
        .contains("HIGHLY_PRIVATE_INPUT"));
}

#[test]
fn oversized_and_nonregular_inputs_and_sources_are_rejected() {
    let f = Fixture::new();
    write(f.dir.path(), "oversized.json", vec![b' '; 1024 * 1024 + 1]);
    has(
        &check_forecast(f.dir.path(), Path::new("oversized.json")),
        "input_read",
    );
    has(&check_forecast(f.dir.path(), Path::new(".")), "input_read");
    write(f.dir.path(), "profile.md", vec![b'x'; 1024 * 1024 + 1]);
    has(&f.check(), "source_read");
}

#[cfg(unix)]
#[test]
fn source_and_parent_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new();
    symlink("brief.md", f.dir.path().join("linked.md")).unwrap();
    f.input["profile"]["path"] = json!("linked.md");
    has(&f.check(), "unsafe_source_path");
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "brief.md", "brief\n");
    symlink(outside.path(), f.dir.path().join("docs")).unwrap();
    f.input["profile"]["path"] = json!("docs/brief.md");
    has(&f.check(), "unsafe_source_path");
}

#[test]
fn known_sha256_vector_and_report_order_are_stable() {
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let mut f = Fixture::new();
    let before = serde_json::to_value(f.check()).unwrap();
    f.input["scenarios"].as_array_mut().unwrap().reverse();
    let mut after = serde_json::to_value(f.check()).unwrap();
    after["input_sha256"] = before["input_sha256"].clone();
    assert_eq!(before, after);
}

fn second_package(f: &mut Fixture, canonical: bool) {
    let mut package = f.input["packages"][0].clone();
    package["id"] = json!("p2");
    if canonical {
        let bytes = "---\nid: TSK-002\ndepends_on: [TSK-001]\nspecs: []\n---\nSecond task.";
        write(f.dir.path(), "project-management/tasks/TSK-002.md", bytes);
        package["source"] =
            json!({"kind":"codeflow_task","task_id":"TSK-002","sha256":sha256(bytes.as_bytes())});
    }
    f.input["packages"].as_array_mut().unwrap().push(package);
    for s in f.input["scenarios"].as_array_mut().unwrap() {
        let extra = s["activities"]
            .as_array()
            .unwrap()
            .iter()
            .cloned()
            .map(|mut a| {
                a["id"] = json!(format!("second-{}", a["id"].as_str().unwrap()));
                a["package_id"] = json!("p2");
                a["start_seconds"] = json!(a["start_seconds"].as_u64().unwrap() + 16);
                a["after"] = json!(a["after"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| format!("second-{}", id.as_str().unwrap()))
                    .collect::<Vec<_>>());
                a
            })
            .collect::<Vec<_>>();
        s["activities"].as_array_mut().unwrap().extend(extra);
    }
}

#[test]
fn canonical_precedence_comes_from_records_not_activity_graph_copies() {
    let mut f = Fixture::new();
    f.canonical();
    second_package(&mut f, true);
    assert!(f.check().is_valid());
    f.input["scenarios"][0]["activities"][4]["after"] = json!(["rework"]);
    has(&f.check(), "copied_package_edge");
    f.input["scenarios"][0]["activities"][4]["after"] = json!([]);
    f.input["scenarios"][0]["activities"][4]["start_seconds"] = json!(0);
    has(&f.check(), "package_precedence");
    f.task("TSK-001", &["TSK-002"], false);
    has(&f.check(), "package_cycle");
}

#[test]
fn external_cross_package_edges_are_allowed_but_mixed_edges_fail() {
    let mut f = Fixture::new();
    second_package(&mut f, false);
    f.input["scenarios"][0]["activities"][4]["after"] = json!(["rework"]);
    assert!(f.check().is_valid());
    f.canonical();
    has(&f.check(), "copied_package_edge");
}

#[test]
fn whole_forecast_links_and_all_simultaneous_resources_are_checked() {
    let mut f = Fixture::new();
    f.input["resources"].as_array_mut().unwrap().push(json!({"id":"release-window","capacity":1,"windows":[{"start_seconds":20,"end_seconds":25}]}));
    for s in f.input["scenarios"].as_array_mut().unwrap() {
        let mut a = activity("release", "release", 20, 5, &["rework"]);
        a["package_id"] = Value::Null;
        a["demands"]
            .as_array_mut()
            .unwrap()
            .push(json!({"resource_id":"release-window","units":1}));
        s["activities"].as_array_mut().unwrap().push(a);
        s["milestones"] = json!([{"id":"release","after":["release"]}]);
    }
    let r = f.check();
    assert!(r.is_valid(), "{r:?}");
    assert_eq!(r.scenarios[0].elapsed_seconds, Some(25));
    f.input["resources"][1]["windows"][0]["end_seconds"] = json!(24);
    has(&f.check(), "resource_calendar");
}

#[test]
fn aggregate_source_budget_is_shared_and_repeated_pins_are_cached() {
    let mut f = Fixture::new();
    let bytes = "x".repeat(1024 * 1024);
    let mut pins = Vec::new();
    for i in 0..16 {
        let path = format!("context-{i}.md");
        write(f.dir.path(), &path, &bytes);
        pins.push(pin(&path, &bytes));
    }
    f.input["packages"][0]["context_pins"] = json!(pins);
    has(&f.check(), "source_read");
    f.input["packages"][0]["context_pins"] = json!([pin("brief.md", "brief\n")]);
    let r = f.check();
    assert!(r.is_valid());
    assert_eq!(r.source_digests.len(), 3);
}

#[test]
fn malformed_workgraph_fields_do_not_become_empty_dependencies() {
    for fields in [
        "depends_on: wrong",
        "depends_on: []\ndependencies: []",
        "specs: [1]",
        "epic_id: 7",
    ] {
        let mut f = Fixture::new();
        let bytes = format!("---\nid: TSK-001\n{fields}\n---\nBody.");
        write(f.dir.path(), "project-management/tasks/TSK-001.md", &bytes);
        f.input["packages"][0]["source"] =
            json!({"kind":"codeflow_task","task_id":"TSK-001","sha256":sha256(bytes.as_bytes())});
        has(&f.check(), "source_record");
    }
}

#[cfg(unix)]
#[test]
fn denied_source_reads_are_redacted() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let path = f.dir.path().join("profile.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
    let result = f.check();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    // Elevated CI can read mode-000 files; exercise the denial only when effective.
    if unsafe { libc::geteuid() } != 0 {
        has(&result, "source_read");
    }
}
