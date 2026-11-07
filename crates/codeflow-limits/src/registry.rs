//! Every limit, declared once. See the crate documentation for the
//! classes and `docs/limits.md`, generated from these entries, for the page
//! a project maintainer reads.
//!
//! Adding a limit is one entry here and one read at the consumer. A policy
//! key needs nothing else: the schema, the validation and the page follow.

use super::{
    elsewhere, fixed, guidance, operational, operational_u32, security, Limit, LimitClass, Route,
    BYTES, MILLISECONDS, SECONDS,
};

/// Declares each entry as a constant and lists them all in [`LIMITS`].
macro_rules! limits {
    ($($(#[$doc:meta])* $name:ident = $value:expr;)*) => {
        $($(#[$doc])* pub const $name: Limit = $value;)*
        /// Every declared limit.
        pub const LIMITS: &[Limit] = &[$($name),*];
    };
}

/// The most a target's deadline arithmetic can hold after the heavy
/// multiplier and the cleanup grace, in seconds.
const MOST_TARGET_SECONDS: u64 = (u64::MAX - 5 * 1_000) / (4 * 1_000);

/// Why a value of the present service is not a project setting.
const SERVICE: &str =
    "one present service serves every project on the host, so no single project policy can set it";

/// The reason shown for a security bound a project does not set.
const BOUND: &str = "security bound, not project-settable";

limits! {
    // The destination probe (TSK-1301).
    /// How long one `git ls-remote` attempt may take on the pre-push and
    /// `codeflow ci` paths.
    REMOTE_QUERY_TIMEOUT = operational("git.remote_query_timeout_seconds", "remote query wait per attempt", SECONDS, 30, 1, "GitHub answered the pre-push default-branch lookup in 11.3 s and 11.6 s on 2026-11-03, over the earlier fixed 10 s; 30 s leaves room above that while a silent destination still refuses");
    /// How many attempts a timed-out `git ls-remote` gets, the first included.
    REMOTE_QUERY_ATTEMPTS = operational("git.remote_query_attempts", "remote query attempts", "attempts", 3, 1, "the earlier lookup made one attempt and two retries; only a timeout is retried, so a refusal stays final");
    /// How long a fetch of the objects a judge needs may take.
    REMOTE_FETCH_TIMEOUT = operational("git.remote_fetch_timeout_seconds", "remote fetch wait", SECONDS, 120, 1, "the earlier fixed fetch wait; a fetch carries objects, so it gets longer than a lookup");

    // The session guards' time budget (TSK-1082). The ceilings nest: remote
    // lookup + pull request lookup + 5 s of other work fit the deadline, and
    // the deadline + refusal write + 5 s fit the hook timeout.
    /// Seconds a harness waits for a guard hook: the `timeout` every shipped
    /// guard hook entry carries in the harness hook files.
    GUARD_HOOK_TIMEOUT = fixed(LimitClass::Security, "guard hook timeout", SECONDS, 30, "written into each harness's hook file and read by the harness, not by CodeFlow; a guard answers in about 0.1 to 0.5 s on a loaded host, so 30 s leaves room for a cold start and a slow disk while bounding how long a stalled guard holds a session (TSK-1082)");
    /// Seconds after which a guard refuses on its own.
    GUARD_DEADLINE = security("limits.guard_deadline_seconds", "guard deadline", SECONDS, 20, 5, 24, "a 10 s buffer under the 30 s guard hook timeout, so the guard's own refusal reaches the harness before the harness stops waiting and lets the call through in Codex and Grok (TSK-1082); the ceiling keeps 6 s for a slow host's refusal write and process exit, and the floor stays above a loaded host's slowest guard");
    /// Seconds a guard waits for one `ls-remote` while a repository's remote
    /// tracking refs are missing. It is per lookup: a command naming several
    /// repositories can make several, and [`GUARD_DEADLINE`] bounds them all.
    GUARD_REMOTE_QUERY_WAIT = fixed(LimitClass::Security, "guard remote lookup wait", SECONDS, 10, "a guard waits this long only while it is still working out which policy applies, so no policy can set it; a 10 s buffer under the 20 s guard deadline for the guard's other work; `git fetch` removes the need to ask");
    /// Seconds the git guard waits for `gh pr view` to name a pull request's
    /// base branch, the guard's other network wait.
    GUARD_PR_LOOKUP_WAIT = security("limits.guard_pr_lookup_wait_seconds", "guard pull request lookup wait", SECONDS, 5, 1, 7, "fits after one guard remote lookup with a 5 s buffer under the 20 s guard deadline; no answer in time is doubt, and the guard blocks on doubt");
    /// Seconds the deadline's refusal waits for its reason to reach stderr
    /// before the guard exits 2 regardless.
    GUARD_REFUSAL_WRITE_WAIT = fixed(LimitClass::Security, "guard refusal write wait", SECONDS, 1, "a stderr pipe nobody reads must not hold the exit; the guard deadline's ceiling is sized around this exact second, so moving it would break the nesting under the hook timeout");

    /// How often the deadline thread checks the clock.
    GUARD_DEADLINE_POLL = fixed(LimitClass::Security, "guard deadline poll", MILLISECONDS, 100, "the deadline's own clock: a short tick so a refusal lands within a tenth of a second of the deadline, costing nothing a project would tune");

    // Coordination timing (TSK-1297).
    /// How long a coordination check waits before it is due again.
    COORD_CHECK_INTERVAL = operational("coord.check_interval_seconds", "coordination check interval", SECONDS, 10 * 60, 1, "the check cadence the coordination loop ships with; ten minutes keeps a watcher current without spending the forge rate limit");
    /// How long a session may stay silent before it counts as gone.
    COORD_HEARTBEAT_TIMEOUT = operational("coord.heartbeat_timeout_seconds", "coordination heartbeat timeout", SECONDS, 30 * 60, 1, "three missed check intervals: a session that is working reports well inside it");
    /// The latest a recheck can be scheduled.
    COORD_MAX_RECHECK = operational("limits.coord_max_recheck_seconds", "longest coordination recheck", SECONDS, 24 * 60 * 60, 1, "a recheck further out than a day would hide a waiting item");

    // Policy keys that predate the registry.
    GIT_LINE_SYNC_INTERVAL = operational_u32("git.line_sync_interval", "line sync reminder interval", "days", 0, 0, "reminders are off until a project chooses an interval; zero disables them");
    GIT_CANDIDATE_SEARCH_DEPTH = operational_u32("git.candidate_search_depth", "candidate search depth", "commits", 200, 1, "covers a line's recent first-parent history; a line with longer history raises it");
    GIT_COMMIT_DESC_MAX_LEN = operational_u32("git.commit_desc_max_len", "commit description length", "characters", 50, 0, "the conventional limit for a commit description");
    GIT_COMMIT_SUBJECT_MAX_LEN = operational_u32("git.commit_subject_max_len", "commit subject length", "characters", 72, 0, "git's 72-column wrap");
    GIT_COMMIT_BODY_MAX_BULLETS = operational_u32("git.commit_body_max_bullets", "commit body bullets", "bullets", 3, 0, "a commit that needs more bullets is usually more than one commit");
    GIT_COMMIT_BODY_BULLET_MAX_LEN = operational_u32("git.commit_body_bullet_max_len", "commit body bullet length", "characters", 72, 0, "git's 72-column wrap");
    GIT_PR_BODY_MAX_WORDS = operational_u32("git.pr_body_max_words", "pull request body words", "words", 1000, 0, "a body past this warns and never blocks (TSK-228); zero turns the warning off");
    /// Files above this size are reported, never refused.
    PATH_WARN_BYTES = guidance("limits.path_warn_bytes", Route::Policy, "large file warning", BYTES, 1 << 20, "a file this size is worth a look before it is committed", "a large file is staged; check that it belongs in the repository");
    /// The size rule's default when it names no limit of its own.
    PATH_DEFAULT_LIMIT_BYTES = operational("limits.path_default_limit_bytes", "default file size rule", BYTES, 10 << 20, 1, "used only when a project's size rule names no limit");

    // The test runner: per-target keys of the test configuration.
    TARGET_TIMEOUT = elsewhere("timeout_seconds", Route::TestConfig, "target timeout", SECONDS, 600, (1, Some(MOST_TARGET_SECONDS)), "ten minutes suits a typical target; the ceiling is only what the deadline arithmetic can hold");
    TARGET_STALL_TIMEOUT = elsewhere("stall_timeout_seconds", Route::TestConfig, "target stall timeout", SECONDS, 300, (1, Some(MOST_TARGET_SECONDS)), "five quiet minutes with no output or CPU is a stall (TSK-1278); the ceiling is only what the deadline arithmetic can hold");
    TARGET_CLEANUP_GRACE = elsewhere("cleanup_grace_seconds", Route::TestConfig, "target cleanup grace", SECONDS, 5, (1, Some(3600)), "the bound on termination, reaping, watcher shutdown and pipe draining after termination starts (TSK-1278); an hour keeps every deadline sum far inside `Instant` and `Duration` arithmetic");
    /// The factor a heavy target's timeout is raised by.
    HEAVY_TIMEOUT_MULTIPLIER = fixed(LimitClass::Operational, "heavy target timeout multiplier", "times", 4, "about four times the supplied quiet coverage run (1713 s); a project sets the target's own `timeout_seconds` instead, and the largest accepted timeout is computed from this factor, so it cannot move on its own");

    // The hosted CI wait: `.codeflow/ci-wait.json`.
    CI_WAIT_TIMEOUT = elsewhere("timeout_minutes", Route::CiWait, "hosted CI wait", "minutes", 30, (1, Some(u64::MAX / 60)), "a typical hosted run finishes inside half an hour; zero would fail the gate at once, and the ceiling is only what a duration in seconds can hold");
    CI_WAIT_POLL_INTERVAL = elsewhere("poll_interval_seconds", Route::CiWait, "hosted CI poll interval", SECONDS, 30, (5, None), "a shorter interval risks the forge rate limit, so the floor is what correctness needs; a longer one only reports later");

    // Guidance: reported, never enforced (AC-10, ADR-0083).
    /// Jobs all heavy runs on this host share; 0 means computed.
    HOST_JOBS = guidance("execution.host_jobs", Route::TestConfig, "host job budget", "jobs", 0, "omitted, the budget is computed from the logical cores and available memory", "the run's jobs exceed what available memory suggests; expect slower builds if the host starts to swap");
    /// Available memory one compile or test job is assumed to need.
    MEMORY_PER_JOB = guidance("limits.memory_per_job_bytes", Route::Policy, "memory assumed per job", BYTES, 1 << 30, "one compile or test job's working set, used only to size the computed host budget", "the computed host budget uses this size");
    /// Workers one run shares among its heavy targets; 0 means the host budget.
    WORKER_BUDGET = guidance("execution.worker_budget", Route::TestConfig, "run worker budget", "jobs", 0, "omitted, a run uses the host budget; set, it is used as set, even above the computed budget", "worker_budget is above the memory-derived budget; it is used as set");
    /// The wait for a host-exclusive target; 0 refuses at once.
    HEAVY_WAIT = guidance("execution.heavy_wait_seconds", Route::CommandLine, "wait for a host-exclusive target", SECONDS, 0, "heavy runs never wait for each other by default; set today by `codeflow test --wait-seconds`, and TSK-1279 brings back the configuration key", "waiting for a host-exclusive target");
    /// A push set past this names its slowest step and still runs to its verdict.
    PUSH_SET_BUDGET_SECONDS = guidance("limits.push_set_budget_seconds", Route::Policy, "pre-push report threshold", SECONDS, 60, "a push set that takes longer still runs to its verdict (TSK-132)", "the pre-push set took longer than this; the hook names its slowest step");
    /// Lines a skill body is expected to stay under.
    READING_BODY_LINES = guidance("limits.reading_body_lines", Route::Policy, "skill body guideline", "lines", 500, "a reading budget, reported and never enforced (ADR-0083)", "this skill body is past its reading guideline");
    /// Lines an instruction file is expected to stay under.
    READING_INSTRUCTIONS_LINES = guidance("limits.reading_instructions_lines", Route::Policy, "instruction file guideline", "lines", 200, "a reading budget, reported and never enforced (ADR-0083)", "this instruction file is past its reading guideline");
    /// Bytes an instruction chain is expected to stay under.
    READING_CHAIN_BYTES = guidance("limits.reading_chain_bytes", Route::Policy, "reading chain guideline", BYTES, 160 * 1024, "a reading budget, reported and never enforced (ADR-0083)", "this reading chain is past its guideline");
    /// Bytes a reminder line is expected to stay under.
    REMINDER_LINE_BYTES = guidance("limits.reminder_line_bytes", Route::Policy, "reminder line guideline", BYTES, 300, "a reading budget, reported and never enforced (ADR-0083)", "this reminder line is past its guideline");

    // Facts of other systems: fixed because CodeFlow does not own them.
    GIT_BINARY_SNIFF_BYTES = fixed(LimitClass::Operational, "binary file sniff", BYTES, 8000, "git's own rule for telling a binary file: the first 8000 bytes hold a NUL; matching it keeps CodeFlow's answer the same as git's");
    CONFLICT_MARKER_SIZE = fixed(LimitClass::Operational, "conflict marker size", "characters", 7, "git's default `conflict-marker-size`");
    GITHUB_PAGE_SIZE = fixed(LimitClass::Operational, "forge page size", "entries", 100, "GitHub's largest page size");
    HARNESS_DESCRIPTION_CHARS = fixed(LimitClass::Operational, "skill description limit", "characters", 1024, "the harness truncates a skill description past this length");
    HARNESS_DESCRIPTION_LISTING_CHARS = fixed(LimitClass::Operational, "skill listing limit", "characters", 1536, "the harness truncates a skill listing past this length");
    HARNESS_IMPORT_DEPTH = fixed(LimitClass::Operational, "instruction import depth", "levels", 4, "the harness follows an `@path` import this many hops deep");
    CODEX_INSTRUCTION_BYTES = fixed(LimitClass::Operational, "Codex instruction limit", BYTES, 32 * 1024, "Codex stops reading an instruction file at this size");
    SHARED_PROJECT_SECTION_ROOM_BYTES = fixed(LimitClass::Operational, "project section room", BYTES, 16 * 1024, "the room the managed block leaves in the Codex limit for a project's own section, so `AGENTS.md` stays readable to Codex");
    SHARED_HEADER_ROOM_BYTES = fixed(LimitClass::Operational, "file header room", BYTES, 1024, "the room the managed block leaves in the Codex limit for the file's header");
    GATE_MOST_RESERVED_LOCKS = fixed(LimitClass::Operational, "reserved lock files", "entries", 6, "the most lock folders one reservation can touch, a fact of the lock layout");

    // The present service.
    PRESENT_WINDOW_HANDOFF = fixed(LimitClass::Operational, "present window handoff wait", SECONDS, 10, SERVICE);
    PRESENT_BROWSER_POLL = fixed(LimitClass::Operational, "present browser poll", MILLISECONDS, 50, SERVICE);
    PRESENT_BROWSER_READ_TIMEOUT = fixed(LimitClass::Operational, "present browser read wait", SECONDS, 10, SERVICE);
    PRESENT_BROWSER_EXIT = fixed(LimitClass::Operational, "present browser exit wait", SECONDS, 3, SERVICE);
    PRESENT_BROWSER_KILL = fixed(LimitClass::Operational, "present browser kill wait", SECONDS, 1, SERVICE);
    PRESENT_BROWSER_KILL_POLL = fixed(LimitClass::Operational, "present browser kill poll", MILLISECONDS, 25, SERVICE);
    PRESENT_LEADERLESS_GROUP_EXIT = fixed(LimitClass::Operational, "present process group exit wait", SECONDS, 2, SERVICE);
    PRESENT_SERVICE_POLL = fixed(LimitClass::Operational, "present service poll", MILLISECONDS, 250, SERVICE);
    PRESENT_SERVICE_RETRY = fixed(LimitClass::Operational, "present service retry pause", SECONDS, 1, SERVICE);
    PRESENT_STATE_POLL = fixed(LimitClass::Operational, "present state poll", MILLISECONDS, 50, SERVICE);
    PRESENT_COMMAND_LINE_BYTES = fixed(LimitClass::Operational, "present process command line read", BYTES, 64 * 1024, SERVICE);
    PRESENT_COMMAND_LINE_SCAN_BYTES = fixed(LimitClass::Operational, "present process command line scan", BYTES, 8 * 1024 * 1024, SERVICE);
    PRESENT_VISITED_PROCESSES = fixed(LimitClass::Operational, "present process tree walk", "entries", 64, SERVICE);
    PRESENT_CONFIG_BYTES = fixed(LimitClass::Operational, "present configuration read", BYTES, 64 * 1024, SERVICE);
    PRESENT_TOKEN_BYTES = fixed(LimitClass::Operational, "present token file read", BYTES, 256 * 1024, SERVICE);
    PRESENT_IDENTITY_BYTES = fixed(LimitClass::Operational, "present identity image", BYTES, 128 * 1024, SERVICE);
    PRESENT_REANCHOR_CACHE_ENTRIES = fixed(LimitClass::Operational, "present reanchor cache", "entries", 8192, SERVICE);
    PRESENT_STATE_LINE_BYTES = fixed(LimitClass::Operational, "present state line", BYTES, 136, "the fixed width of one state line in the responses ledger, a file format fact");

    // Shared poll cadences: how often a bounded wait checks again. Each is
    // operational: a slower value spends less CPU and answers later.
    POLL_GATE_RECORD_MILLIS = operational("limits.poll_gate_record_millis", "gate record lock poll", MILLISECONDS, 2, 1, "the gate's record lock is held for microseconds, so its waiters check often");
    POLL_CHILD_OUTPUT_MILLIS = operational("limits.poll_child_output_millis", "child output poll", MILLISECONDS, 5, 1, "a child that has finished should be read at once");
    POLL_LEDGER_LOCK_MILLIS = operational("limits.poll_ledger_lock_millis", "ledger lock poll", MILLISECONDS, 10, 1, "the ledger lock is held for one append");
    POLL_LOCK_MILLIS = operational("limits.poll_lock_millis", "lock and process poll", MILLISECONDS, 20, 1, "short enough to feel immediate, long enough to cost nothing");
    POLL_PROCESS_MILLIS = operational("limits.poll_process_millis", "child process poll", MILLISECONDS, 25, 1, "a child process usually ends within a few polls");
    POLL_WAIT_MILLIS = operational("limits.poll_wait_millis", "bounded wait poll", MILLISECONDS, 50, 1, "the wait is for work that takes seconds");
    REMOTE_RETRY_PAUSE_MILLIS = operational("limits.remote_retry_pause_millis", "pause between remote query attempts", MILLISECONDS, 100, 0, "lets a brief network failure pass before the next attempt");
    RECALL_BUSY_TIMEOUT_SECONDS = operational("limits.recall_busy_timeout_seconds", "recall index busy wait", SECONDS, 5, 1, "the wait for another process to finish writing the recall index");
    GATE_LOCK_RETAKE_SECONDS = operational("limits.gate_lock_retake_seconds", "gate lock retake wait", SECONDS, 5, 1, "a run that gave up its lock for an exclusive step gets it back within this wait");
    RUNNER_CPU_SAMPLE_SECONDS = operational("limits.runner_cpu_sample_seconds", "target CPU sample interval", SECONDS, 1, 1, "how often a quiet target's CPU is sampled as activity");
    PRESENT_SERVICE_START_SECONDS = operational("limits.present_service_start_seconds", "present service start wait", SECONDS, 10, 1, "the wait for the local service to report ready");
    PRESENT_CONNECT_SECONDS = operational("limits.present_connect_seconds", "present service connect wait", SECONDS, 2, 1, "the wait for one local connection to the service");
    PRESENT_FOLLOW_POLL_MILLIS = operational("limits.present_follow_poll_millis", "present wait poll", MILLISECONDS, 250, 1, "how often `codeflow present wait` checks for a reply");

    FOLLOW_URGENT_SHOWN = operational("limits.follow_urgent_shown", "follow urgent shown", "entries", 20, 1, "The most urgent items `list` shows before it points at the rest.");
    CEREMONY_BATCH = operational("limits.ceremony_batch", "ceremony batch", "entries", 50, 1, "Pull requests asked for in one query.");
    COORD_OUTPUT_LIMIT = operational("limits.coord_output_limit", "coord output limit", "entries", 1 << 20, 1, "The most output one observation keeps from each stream.");
    COORD_TEXT_SHORT = operational("limits.coord_text_short", "coord text short", "entries", 200, 1, "The bound for a short field: a title, a reason, a reference.");
    COORD_TEXT_MEDIUM = operational("limits.coord_text_medium", "coord text medium", "entries", 1000, 1, "The bound for a condition or an observation.");
    COORD_TEXT_LONG = operational("limits.coord_text_long", "coord text long", "entries", 4000, 1, "The bound for a description.");
    DELEGATE_MAX_PROMPT_BYTES = operational("limits.delegate_max_prompt_bytes", "delegate max prompt", BYTES, 1024 * 1024, 1, "Maximum exact prompt size accepted by the lifecycle protocol.");
    DELEGATE_MAX_MESSAGE_BYTES = operational("limits.delegate_max_message_bytes", "delegate max message", BYTES, 4 * 1024 * 1024, 1, "bounds one delegate message so a runaway seat cannot exhaust memory");
    DELEGATE_MAX_ERROR_BYTES = operational("limits.delegate_max_error_bytes", "delegate max error", BYTES, 1024 * 1024, 1, "bounds one delegate message so a runaway seat cannot exhaust memory");
    DELEGATE_MAX_TRANSCRIPT_BYTES = operational("limits.delegate_max_transcript_bytes", "delegate max transcript", BYTES, 512 * 1024 * 1024, 1, "Largest session transcript read to verify a launch.");
    DOCTOR_MAX_SETTINGS_BYTES = operational("limits.doctor_max_settings_bytes", "doctor max settings", BYTES, 16 * 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    DOCTOR_VERSION_PROBE_OUTPUT_BYTES = operational("limits.doctor_version_probe_output_bytes", "doctor version probe output", BYTES, 64 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    DOCTOR_INSTRUCTION_WALK_DEPTH = operational("limits.doctor_instruction_walk_depth", "doctor instruction walk depth", "levels", 16, 1, "Directory depth past which the instruction walk stops.");
    DOCTOR_CODEX_PROBED_FOLDERS = operational("limits.doctor_codex_probed_folders", "doctor codex probed folders", "entries", 4, 1, "How many folders the probe reports: two lock folders, `objects` and `refs`.");
    DOCTOR_STARTUP_READ_LIMIT = operational("limits.doctor_startup_read_limit", "doctor startup read limit", "entries", 256 * 1024, 1, "The most bytes read from one startup file.");
    ESTIMATE_MAX_ADOPTION_BYTES = operational("limits.estimate_max_adoption_bytes", "estimate max adoption", BYTES, 64 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    ESTIMATE_MAX_FORECAST_FILES = operational("limits.estimate_max_forecast_files", "estimate max forecast files", "entries", 256, 1, "At most this many forecast files are joined; more is a finding.");
    ESTIMATE_MAX_INPUT_BYTES = operational("limits.estimate_max_input_bytes", "estimate max input", BYTES, 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    ESTIMATE_MAX_SOURCE_BYTES = operational("limits.estimate_max_source_bytes", "estimate max source", BYTES, 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    ESTIMATE_MAX_TOTAL_SOURCE_BYTES = operational("limits.estimate_max_total_source_bytes", "estimate max total source", BYTES, 16 * 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    ESTIMATE_MAX_HORIZON = operational("limits.estimate_max_horizon", "estimate max horizon", SECONDS, 366 * 24 * 60 * 60, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    FEEDBACK_TEMPLATE_MAX_BYTES = operational("limits.feedback_template_max_bytes", "feedback template max", BYTES, 64 * 1024, 1, "The largest project template `read_project_template` reads.");
    UNREADABLE_SHOWN_NAMES = operational("limits.unreadable_shown_names", "unreadable shown names", "entries", 5, 1, "How many names one report line lists before it counts the rest.");
    WORKTREE_INVENTORY_SHOWN = operational("limits.worktree_inventory_shown", "worktree inventory shown", "entries", 5, 1, "The most entries shown per list.");
    ORIENT_MAX_LINES = operational("limits.orient_max_lines", "orient max lines", "lines", 30, 1, "Base digest guideline from the charter; line and epic inventories are complete.");
    IDS_PUSH_ATTEMPTS = operational("limits.ids_push_attempts", "ids push attempts", "attempts", 6, 1, "Attempts at one push before giving up: the first and five retries (SPC-013 planning resolution 4).");
    QUALIFICATION_MAX_RECORD_BYTES = operational("limits.qualification_max_record_bytes", "qualification max record", BYTES, 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    RECALL_MAX_DEPTH = operational("limits.recall_max_depth", "recall max depth", "levels", 64, 1, "bounds the walk of the ledger folders recall indexes");
    RECALL_MAX_ENTRIES = operational("limits.recall_max_entries", "recall max entries", "entries", 10_000, 1, "bounds the walk of the ledger folders recall indexes");
    SCAFFOLD_PORTAL_MAX_IDENTITY_BYTES = operational("limits.scaffold_portal_max_identity_bytes", "scaffold portal max identity", BYTES, 128, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    SCAFFOLD_PORTAL_MAX_MANIFEST_BYTES = operational("limits.scaffold_portal_max_manifest_bytes", "scaffold portal max manifest", BYTES, 2 * 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    SCAFFOLD_PORTAL_MAX_BUNDLE_FILES = operational("limits.scaffold_portal_max_bundle_files", "scaffold portal max bundle files", "entries", 256, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    SCAFFOLD_PORTAL_MAX_STATE_BYTES = operational("limits.scaffold_portal_max_state_bytes", "scaffold portal max state", BYTES, 64 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    SCAFFOLD_PORTAL_MAX_BASELINE_ENTRIES = operational("limits.scaffold_portal_max_baseline_entries", "scaffold portal max baseline entries", "entries", 1_024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    SCAFFOLD_PORTAL_MAX_MANAGED_FILE_BYTES = operational("limits.scaffold_portal_max_managed_file_bytes", "scaffold portal max managed file", BYTES, 4 * 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    SCAFFOLD_PORTAL_MAX_RECONCILIATION_READ_BYTES = operational("limits.scaffold_portal_max_reconciliation_read_bytes", "scaffold portal max reconciliation read", BYTES, 64 * 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    TEST_CONFIG_NAMED_CONFIG_MAX_BYTES = operational("limits.test_config_named_config_max_bytes", "test config named config max", BYTES, 4 * 1024 * 1024, 1, "The largest named test config `codeflow test --config` reads.");
    GATE_EVIDENCE_FILE_LIMIT = operational("limits.gate_evidence_file_limit", "gate evidence file limit", "entries", 1 << 30, 1, "The largest file and the most names in one folder that the durable copy takes; a run folder beyond either is refused, never truncated.");
    GATE_EVIDENCE_ENTRY_LIMIT = operational("limits.gate_evidence_entry_limit", "gate evidence entry limit", "entries", 100_000, 1, "bounds what a gate reads as evidence");
    RUNNER_OUTPUT_CAPTURE_LIMIT = operational("limits.runner_output_capture_limit", "runner output capture limit", "entries", 1024 * 1024, 1, "Maximum retained bytes for each child output stream. Readers keep the tail so the most recent failure context survives noisy commands.");
    FIGURES_MAX_PAGE_CLASSES = operational("limits.figures_max_page_classes", "figures max page classes", "entries", 256, 1, "bounds what the figure check reads from a portal build");
    FIGURES_MAX_FIGURE_BINDINGS = operational("limits.figures_max_figure_bindings", "figures max figure bindings", "entries", 512, 1, "bounds what the figure check reads from a portal build");
    FIGURES_MAX_FACTS = operational("limits.figures_max_facts", "figures max facts", "entries", 32, 1, "bounds what the figure check reads from a portal build");
    FIGURES_MAX_DECLARATION_BYTES = operational("limits.figures_max_declaration_bytes", "figures max declaration", BYTES, 256 * 1024, 1, "bounds what the figure check reads from a portal build");
    FIGURES_MAX_TOTAL_DECLARATION_BYTES = operational("limits.figures_max_total_declaration_bytes", "figures max total declaration", BYTES, 8 * 1024 * 1024, 1, "bounds what the figure check reads from a portal build");
    FIGURES_MAX_FACT_SOURCE_BYTES = operational("limits.figures_max_fact_source_bytes", "figures max fact source", BYTES, 4 * 1024 * 1024, 1, "bounds what the figure check reads from a portal build");
    FIGURES_MAX_TOTAL_FACT_SOURCE_BYTES = operational("limits.figures_max_total_fact_source_bytes", "figures max total fact source", BYTES, 64 * 1024 * 1024, 1, "bounds what the figure check reads from a portal build");
    PORTAL_MAX_MANIFEST_BYTES = operational("limits.portal_max_manifest_bytes", "portal max manifest", BYTES, 8 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_PAGEFIND_ENTRY_BYTES = operational("limits.portal_max_pagefind_entry_bytes", "portal max pagefind entry", BYTES, 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_PAGES = operational("limits.portal_max_pages", "portal max pages", "entries", 10_000, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_REPOSITORY_FILES = operational("limits.portal_max_repository_files", "portal max repository files", "entries", 100_000, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_GIT_TREE_BYTES = operational("limits.portal_max_git_tree_bytes", "portal max git tree", BYTES, 64 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_ARTIFACTS = operational("limits.portal_max_artifacts", "portal max artifacts", "entries", 100_000, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_TOTAL_TRAVERSAL_ENTRIES = operational("limits.portal_max_total_traversal_entries", "portal max total traversal entries", "entries", 100_000, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_CLAIMED_FILE_BYTES = operational("limits.portal_max_claimed_file_bytes", "portal max claimed file", BYTES, 64 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_SOURCE_BYTES = operational("limits.portal_max_source_bytes", "portal max source", BYTES, 4 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_TOTAL_SOURCE_BYTES = operational("limits.portal_max_total_source_bytes", "portal max total source", BYTES, 64 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_MEDIA_BYTES = operational("limits.portal_max_media_bytes", "portal max media", BYTES, 8 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_TOTAL_MEDIA_BYTES = operational("limits.portal_max_total_media_bytes", "portal max total media", BYTES, 64 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_RASTER_DIMENSION = operational("limits.portal_max_raster_dimension", "portal max raster dimension", "pixels", 8_192, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_RASTER_PIXELS = operational("limits.portal_max_raster_pixels", "portal max raster pixels", "pixels", 33_554_432, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_TOTAL_ARTIFACT_BYTES = operational("limits.portal_max_total_artifact_bytes", "portal max total artifact", BYTES, 512 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_STALE_REASON_BYTES = operational("limits.portal_max_stale_reason_bytes", "portal max stale reason", BYTES, 512, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_STALE_STUB_BYTES = operational("limits.portal_max_stale_stub_bytes", "portal max stale stub", BYTES, 4 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_RENDERED_PAGE_BYTES = operational("limits.portal_max_rendered_page_bytes", "portal max rendered page", BYTES, 8 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_TOTAL_RENDERED_BYTES = operational("limits.portal_max_total_rendered_bytes", "portal max total rendered", BYTES, 256 * 1024 * 1024, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_IDS_PER_PAGE = operational("limits.portal_max_ids_per_page", "portal max ids per page", "entries", 256, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_RELATIONSHIPS_PER_PAGE = operational("limits.portal_max_relationships_per_page", "portal max relationships per page", "entries", 512, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_BACKLINKS_PER_PAGE = operational("limits.portal_max_backlinks_per_page", "portal max backlinks per page", "entries", 512, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_SNIPPETS_PER_PAGE = operational("limits.portal_max_snippets_per_page", "portal max snippets per page", "entries", 64, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_FRAGMENT_LINKS = operational("limits.portal_max_fragment_links", "portal max fragment links", "entries", 16_384, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    PORTAL_MAX_PORTAL_PAGE_CARRIERS = operational("limits.portal_max_portal_page_carriers", "portal max portal page carriers", "entries", 64, 1, "bounds what the portal check reads from a portal build; a larger site raises it");
    WORKGRAPH_LANDED_WALK_LIMIT = operational("limits.workgraph_landed_walk_limit", "workgraph landed walk limit", "entries", 100_000, 1, "The most commits walked from a landed tip to the line it left.");
    RECORD_TEXT_LIMIT = operational("limits.record_text_limit", "record text limit", "entries", 60, 1, "the length a record line is cut to in a message");
    WORK_START_MAX_ACTIVATION_ENTRIES = operational("limits.work_start_max_activation_entries", "work start max activation entries", "entries", 16_384, 1, "Maximum task-home entries inspected by the activation probe. An exhausted inventory is an error, never evidence that durable work is absent.");
    WORK_START_MAX_BYTES = operational("limits.work_start_max_bytes", "work start max", BYTES, 1024 * 1024, 1, "bounds one read of a project file, so a stray huge file cannot exhaust memory; a project with larger files raises it");
    WORK_START_MAX_RECORD_BYTES = operational("limits.work_start_max_record_bytes", "work start max record", BYTES, 4 * 1024 * 1024, 1, "The largest work record a tree read accepts. The largest real record is about 100 KiB; the bound matches the portal's per-file source limit.");
    CEREMONY_HOST_TIMEOUT_SECONDS = operational("limits.ceremony_host_timeout_seconds", "ceremony host timeout", SECONDS, 60, 1, "How long one `gh` call may take before it counts as failed.");
    COORD_RECORDS_TIMEOUT_SECONDS = operational("limits.coord_records_timeout_seconds", "coord records timeout", SECONDS, 10, 1, "Records: 10 seconds.");
    COORD_GIT_TIMEOUT_SECONDS = operational("limits.coord_git_timeout_seconds", "coord git timeout", SECONDS, 10, 1, "One local Git observation: 10 seconds.");
    COORD_NETWORK_TIMEOUT_SECONDS = operational("limits.coord_network_timeout_seconds", "coord network timeout", SECONDS, 20, 1, "One network observation: 20 seconds.");
    COORD_OBSERVE_HEARTBEAT_TIMEOUT_SECONDS = operational("limits.coord_observe_heartbeat_timeout_seconds", "coord observe heartbeat timeout", SECONDS, 5, 1, "One heartbeat write: 5 seconds.");
    COORD_FRESHNESS_SECONDS = operational("limits.coord_freshness_seconds", "coord freshness", SECONDS, 60, 1, "Freshness of a cached observation: 60 seconds.");
    COORD_COMMAND_WAIT_SECONDS = operational("limits.coord_command_wait_seconds", "coord command wait", SECONDS, 10, 1, "How long a command waits for the lock.");
    DELEGATE_RUN_LOCK_TIMEOUT_SECONDS = operational("limits.delegate_run_lock_timeout_seconds", "delegate run lock timeout", SECONDS, 1, 1, "the wait for a delegate run lock");
    LEDGER_LOCK_SETTLE_SECONDS = operational("limits.ledger_lock_settle_seconds", "ledger lock settle", SECONDS, 1, 1, "How long a compaction waits for a contended lock before it treats the holder as a real compaction or writer. A child process spawned by any thread while a lock descriptor was open keeps that lock until it execs, so a lock just released can read as held for a moment.");
    LEDGER_REFUSAL_LOCK_WAIT_SECONDS = operational("limits.ledger_refusal_lock_wait_seconds", "ledger refusal lock wait", SECONDS, 1, 1, "How long a hook or guard waits for another writer's lock on this ledger before it gives up on the record and keeps its verdict.");
    DOCTOR_VERSION_PROBE_TIMEOUT_SECONDS = operational("limits.doctor_version_probe_timeout_seconds", "doctor version probe timeout", SECONDS, 5, 1, "the wait for a tool to print its version");
    DOCTOR_CODEX_LOAD_TIMEOUT_SECONDS = operational("limits.doctor_codex_load_timeout_seconds", "doctor codex load timeout", SECONDS, 20, 1, "How long one Codex command may take before it counts as not run.");
    DOCTOR_GH_TIMEOUT_SECONDS = operational("limits.doctor_gh_timeout_seconds", "doctor gh timeout", SECONDS, 15, 1, "Each `gh` call is bounded, so an unreachable host cannot hang doctor.");
    RUNNER_HEARTBEAT_INTERVAL_SECONDS = operational("limits.runner_heartbeat_interval_seconds", "runner heartbeat interval", SECONDS, 300, 1, "how often a quiet target reports that it is still running");
    PORTAL_GIT_TIMEOUT_SECONDS = operational("limits.portal_git_timeout_seconds", "portal git timeout", SECONDS, 30, 1, "the wait for one git read in the portal check");
    WORK_START_REVIEW_LOOKUP_TIMEOUT_SECONDS = operational("limits.work_start_review_lookup_timeout_seconds", "work start review lookup timeout", SECONDS, 15, 1, "Maximum provider wait for the review evidence of one predecessor pin.");

    // The present document format and runtime.
    PRESENT_MAX_DOCUMENT_BYTES = fixed(LimitClass::Operational, "present document bytes", BYTES, 8 * 1024 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_BLOCKS = fixed(LimitClass::Operational, "present blocks", "entries", 512, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_NESTING = fixed(LimitClass::Operational, "present nesting", "entries", 12, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_TITLE_BYTES = fixed(LimitClass::Operational, "present title bytes", BYTES, 512, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_PROSE_BYTES = fixed(LimitClass::Operational, "present prose bytes", BYTES, 512 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_CODE_BYTES = fixed(LimitClass::Operational, "present code bytes", BYTES, 1024 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FIGURE_BLOCKS = fixed(LimitClass::Operational, "present figure blocks", "entries", 24, "The drawn block budget: figure blocks across the whole nested tree; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FIGURE_DECLARATION_BYTES = fixed(LimitClass::Operational, "present figure declaration bytes", BYTES, 64 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_DIAGRAM_BYTES = fixed(LimitClass::Operational, "present diagram bytes", BYTES, 64 * 1024, "The removed diagram block's bounds, kept to judge a stored pre-release revision: one outside them was never admitted (TSK-114); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_DIAGRAM_BLOCKS = fixed(LimitClass::Operational, "present diagram blocks", "entries", 24, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_HTML_BYTES = fixed(LimitClass::Operational, "present html bytes", BYTES, 512 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_MEDIA_BYTES = fixed(LimitClass::Operational, "present media bytes", BYTES, 4 * 1024 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_RASTER_DIMENSION = fixed(LimitClass::Operational, "present raster dimension", "pixels", 16_384, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_RASTER_PIXELS = fixed(LimitClass::Operational, "present raster pixels", "pixels", 40_000_000, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_LABEL_BYTES = fixed(LimitClass::Operational, "present label bytes", BYTES, 4 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_LANGUAGE_BYTES = fixed(LimitClass::Operational, "present language bytes", BYTES, 64, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_TABLE_ROWS = fixed(LimitClass::Operational, "present table rows", "entries", 2_000, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_TABLE_COLUMNS = fixed(LimitClass::Operational, "present table columns", "entries", 64, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_COLLECTION_ITEMS_PER_BLOCK = fixed(LimitClass::Operational, "present collection items per block", "entries", 2_048, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_DOCUMENT_COLLECTION_ITEMS = fixed(LimitClass::Operational, "present document collection items", "entries", 16_384, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_REPLY_TEXT_BYTES = fixed(LimitClass::Operational, "present reply text bytes", BYTES, 16 * 1024, "Agent reply text is bounded in UTF-8 bytes, independently of review-note UTF-16 limits; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FEEDBACK_NOTES = fixed(LimitClass::Operational, "present feedback notes", "entries", 100, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FEEDBACK_TEXT_UTF16 = fixed(LimitClass::Operational, "present feedback text utf16", "UTF-16 units", 16 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_SELECTOR_EXACT_UTF16 = fixed(LimitClass::Operational, "present selector exact utf16", "UTF-16 units", 4 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_VISUAL_ANCHOR_BYTES = fixed(LimitClass::Operational, "present visual anchor bytes", BYTES, 2 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_REGION_COORDINATE_SCALE = fixed(LimitClass::Operational, "present region coordinate scale", "entries", 1_000_000, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_VISIBLE_FEEDBACK = fixed(LimitClass::Operational, "present visible feedback", "entries", 256, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FEEDBACK_BYTES = fixed(LimitClass::Operational, "present feedback bytes", BYTES, 256 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_EXCERPT_TEXT_BYTES = fixed(LimitClass::Operational, "present excerpt text bytes", BYTES, 4 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_EXCERPT_IMAGE_BYTES = fixed(LimitClass::Operational, "present excerpt image bytes", BYTES, 24 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_EXCERPT_IMAGE_B64_BYTES = fixed(LimitClass::Operational, "present excerpt image b64 bytes", BYTES, 32 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_CROP_WIDTH = fixed(LimitClass::Operational, "present crop width", "pixels", 480, "The largest crop the page captures (`web/src/excerpt.ts` `MAX_CROP_*`); a PNG crop over either bound is refused before anything decodes it; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_CROP_HEIGHT = fixed(LimitClass::Operational, "present crop height", "pixels", 360, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_EVENTS_PER_RESPONSE = fixed(LimitClass::Operational, "present events per response", "entries", 100, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_EVENT_RESPONSE_BYTES = fixed(LimitClass::Operational, "present event response bytes", BYTES, 1024 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_EVENT_RECORD_BYTES = fixed(LimitClass::Operational, "present event record bytes", BYTES, 256 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_EVENT_LOG_BYTES = fixed(LimitClass::Operational, "present event log bytes", BYTES, 64 * 1024 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_RUNTIME_CONTROL_BYTES = fixed(LimitClass::Operational, "present runtime control bytes", BYTES, 64 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FEEDBACK_EVENTS = fixed(LimitClass::Operational, "present feedback events", "entries", 100_000, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_REVISIONS = fixed(LimitClass::Operational, "present revisions", "entries", 10_000, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_STATE_ENTRIES = fixed(LimitClass::Operational, "present state entries", "entries", 200_000, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_STATE_DEPTH = fixed(LimitClass::Operational, "present state depth", "entries", 24, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_EVENT_POLL_SECONDS = fixed(LimitClass::Operational, "present event poll seconds", SECONDS, 25, "one present service serves every project on the host, so no single project policy can set it");
    PRESENT_BOOTSTRAP_TTL_SECONDS = fixed(LimitClass::Operational, "present bootstrap ttl seconds", SECONDS, 120, "one present service serves every project on the host, so no single project policy can set it");
    PRESENT_SESSION_IDLE_SECONDS = fixed(LimitClass::Operational, "present session idle seconds", SECONDS, 4 * 60 * 60, "one present service serves every project on the host, so no single project policy can set it");
    PRESENT_WINDOW_CLOSE_GRACE_SECONDS = fixed(LimitClass::Operational, "present window close grace seconds", SECONDS, 15, "How long the isolated browser stays running once its last review window has closed: a window reopened within it, by `present show` or a reload, keeps the browser; after it the browser quits and the session stays open; one present service serves every project on the host, so no single project policy can set it");
    PRESENT_CLOSED_RETENTION_DAYS = elsewhere("closed_days", Route::PresentConfig, "present closed retention days", "days", 30, (1, None), "the shipped present retention default");
    PRESENT_MAX_CLOSED_SESSIONS = elsewhere("max_closed_sessions", Route::PresentConfig, "present closed sessions", "entries", 100, (1, None), "the shipped present retention default");
    PRESENT_MAX_PROJECT_STATE_BYTES = elsewhere("max_project_bytes", Route::PresentConfig, "present project state bytes", BYTES, 500 * 1024 * 1024, (1, None), "the shipped present retention default");
    PRESENT_MAX_SESSION_STATE_BYTES = fixed(LimitClass::Operational, "present session state bytes", BYTES, 64 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_HISTORY_READ_BYTES = fixed(LimitClass::Operational, "present history read bytes", BYTES, 32 * 1024 * 1024, "part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_RAW_ASSET_BYTES = fixed(LimitClass::Operational, "present raw asset bytes", BYTES, 5_000_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_RAW_CHUNK_BYTES = fixed(LimitClass::Operational, "present raw chunk bytes", BYTES, 850_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_BROTLI_ASSET_BYTES = fixed(LimitClass::Operational, "present brotli asset bytes", BYTES, 1_150_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_GZIP_ASSET_BYTES = fixed(LimitClass::Operational, "present gzip asset bytes", BYTES, 340_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_GZIP_CHUNK_BYTES = fixed(LimitClass::Operational, "present gzip chunk bytes", BYTES, 150_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_BROTLI_CHUNK_BYTES = fixed(LimitClass::Operational, "present brotli chunk bytes", BYTES, 150_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_SERVICE_BINARY_DELTA_BYTES = fixed(LimitClass::Operational, "present service binary delta bytes", BYTES, 1_250_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_EXPORT_PAYLOAD_BYTES = fixed(LimitClass::Operational, "present export payload bytes", BYTES, 1_300_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_EXPORT_SHELL_BYTES = fixed(LimitClass::Operational, "present export shell bytes", BYTES, 1_750_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_COMBINED_BINARY_DELTA_BYTES = fixed(LimitClass::Operational, "present combined binary delta bytes", BYTES, 2_600_000, "the size budget of the shipped present runtime, a build contract rather than a run-time setting");
    PRESENT_MAX_ENTITY_LABEL_CHARS = fixed(LimitClass::Operational, "present entity label chars", "characters", 120, "Entity labels are collapsed and cut to this many characters (SPC-014 B2); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_ENTITY_FOR_IDS = fixed(LimitClass::Operational, "present entity for ids", "entries", 8, "A `data-cf-for` names at most this many entities; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_LEGEND_ENTRIES = fixed(LimitClass::Operational, "present legend entries", "entries", 12, "A v2 `html` legend lists 1 to this many entries; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_DESCRIPTION_CHARS = fixed(LimitClass::Operational, "present description chars", "characters", 2_000, "A v2 `html` description is at most this many characters; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_SUMMARY_CHARS = fixed(LimitClass::Operational, "present summary chars", "characters", 200, "A v2 document summary is 1 to this many characters; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FUZZY_QUOTE_UTF16 = fixed(LimitClass::Operational, "present fuzzy quote utf16", "UTF-16 units", 512, "The fuzzy quote step runs for quotes up to this many UTF-16 units ; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FUZZY_TEXT_UTF16 = fixed(LimitClass::Operational, "present fuzzy text utf16", "UTF-16 units", 65_536, "... inside block texts up to this many (SPC-014 B1); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_FUZZY_WORK_BUDGET = fixed(LimitClass::Operational, "present fuzzy work budget", "entries", 32_000_000, "The edit-distance cells one fuzzy quote search may compute. A search that needs more stops before scoring and the note shows on its block with a notice (SPC-014 B1, TSK-118 review round 1); one present service serves every project on the host, so no single project policy can set it");
    PRESENT_MAX_FORMS_PER_DOCUMENT = fixed(LimitClass::Operational, "present forms per document", "entries", 32, "At most this many `form` and v2 `decision` blocks in one document (SPC-014 I1); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FORM_FIELDS = fixed(LimitClass::Operational, "present form fields", "entries", 32, "At most this many fields in one form (I1); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FIELD_OPTIONS = fixed(LimitClass::Operational, "present field options", "entries", 24, "A choice field lists 2 to this many options (B6, I1); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_DECISION_OPTIONS = fixed(LimitClass::Operational, "present decision options", "entries", 8, "A v2 decision lists 2 to this many options (B6); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FORM_LABEL_CHARS = fixed(LimitClass::Operational, "present form label chars", "characters", 200, "A field or option label is 1 to this many characters (B6); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_FORM_TEXT_UTF16 = fixed(LimitClass::Operational, "present form text utf16", "UTF-16 units", 16_384, "The largest `max_length` a text field may declare, in UTF-16 units (B6); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_DECLINE_REASON_BYTES = fixed(LimitClass::Operational, "present decline reason bytes", BYTES, 4 * 1024, "A decline reason is at most this many bytes (B6); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_ANSWER_REQUEST_BYTES = fixed(LimitClass::Operational, "present answer request bytes", BYTES, 64 * 1024, "An answer request body is at most this many bytes; the bound is checked before anything else in the body (I3 `answer_too_large`); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_JSON_CHAR_BYTES = fixed(LimitClass::Operational, "present json char bytes", BYTES, 6, "The most JSON bytes one character of question text takes in a record: a `\\uXXXX` escape (6) is longer than any UTF-8 character (4); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_JSON_NUMBER_GROWTH = fixed(LimitClass::Operational, "present json number growth", "entries", 23, "The most bytes a number grows when the record writes it again: at most 24 bytes out (`-2.2250738585072014e-308`) for at least 1 byte in; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_RECORD_FRAME_BYTES = fixed(LimitClass::Operational, "present record frame bytes", BYTES, 20_715, "The rest of a record at its largest: its JSON structure for 32 fields of 24 options, the member names, the longest event and outcome names, numbers at `u64::MAX`, the UUIDs and digests. Pinned by the test that writes that record (`the_answer_record_cap_is_the_largest_record`); part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_RESPONSE_LOG_BYTES = fixed(LimitClass::Operational, "present response log bytes", BYTES, 64 * 1024 * 1024, "The whole `responses.jsonl` ledger; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_MAX_RESPONSE_EVENTS = fixed(LimitClass::Operational, "present response events", "entries", 100_000, "Lines in `responses.jsonl`; part of the present document format: the public schemas, the browser runtime and saved sessions all carry this number, so every reader must agree on it");
    PRESENT_SERVICE_EXIT_WAIT_SECONDS = fixed(LimitClass::Operational, "present service exit wait seconds", SECONDS, 10, "How long `present close` waits for the session service to exit; one present service serves every project on the host, so no single project policy can set it");

    // Bounds of the guards.
    DELETION_READER_VALUES = fixed(LimitClass::Security, "deletion reader values", "entries", 64, BOUND);
    DELETION_READER_VARIANTS = fixed(LimitClass::Security, "deletion reader variants", "entries", 64, BOUND);
    DELETION_READER_DEPTH = fixed(LimitClass::Security, "deletion reader depth", "levels", 6, BOUND);
    DELETION_READER_LOOP_PASSES = fixed(LimitClass::Security, "deletion reader loop passes", "passes", 6, BOUND);
    STARTUP_LINK_LIMIT = fixed(LimitClass::Security, "startup link limit", "links", 40, BOUND);
    ARGUMENT_WALK_LIMIT = fixed(LimitClass::Security, "argument walk limit", "entries", 4096, BOUND);
    GUARD_NESTING_LIMIT = fixed(LimitClass::Security, "guard nesting limit", "levels", 8, BOUND);
    GUARD_ALIAS_DEPTH = fixed(LimitClass::Security, "guard alias depth", "levels", 8, BOUND);
    POLICY_SETTINGS_READ_BYTES = fixed(LimitClass::Security, "policy settings read", BYTES, 4 * 1024 * 1024, "the largest policy or settings file read; the policy file cannot set the bound on its own read (TSK-397)");
    HOOK_SCHEMA_INPUT_BYTES = fixed(LimitClass::Security, "hook schema input", BYTES, 32 * 1024 * 1024, "bounds the allocation before a hook parses its input, which comes before any policy read; it holds the largest terminal message with every byte escaped");
    GLOB_PATTERN_BYTES = fixed(LimitClass::Security, "glob pattern", BYTES, 1024, "the longest glob the matcher compiles; policy globs compile while policy is read, so policy cannot set it");
    GLOB_COMPILED_BYTES = fixed(LimitClass::Security, "glob compiled", BYTES, 256 * 1024, "the largest compiled glob, which is the step bound for compilation; policy globs compile while policy is read, so policy cannot set it");
    GLOB_CACHE_ENTRIES = fixed(LimitClass::Security, "glob cache entries", "entries", 128, "compiled globs kept at once; policy globs compile while policy is read, so policy cannot set it");
    GUARD_BRACE_WORD_LIMIT = security("limits.guard_brace_word_limit", "guard brace word limit", "words", 64, 16, 64, "the most words one brace expansion yields before the guard refuses; the ceiling is the default because the expansion cost multiplies");
    GUARD_BRACE_DEPTH_LIMIT = security("limits.guard_brace_depth_limit", "guard brace depth limit", "levels", 256, 8, 256, "brace groups one word may nest before the guard refuses; it bounds recursion, so the ceiling is the default");
    GUARD_BRACE_WORD_BYTES = security("limits.guard_brace_word_bytes", "guard brace word", BYTES, 4096, 1024, 4096, "the longest word brace expansion reads; the longest path the supported systems open, and the nested scan costs the square of the length, so the ceiling is the default");
    GUARD_BRACE_CHAR_LIMIT = security("limits.guard_brace_char_limit", "guard brace char limit", "characters", 10, 6, 12, "the `{`, `}` and `,` characters a word may hold for the guard to read its braces; each may be quoted, so readings double per character and the ceiling stays near the default");
    GUARD_GLOB_ENTRY_LIMIT = security("limits.guard_glob_entry_limit", "guard glob entry limit", "entries", 4096, 256, 65_536, "directory entries one glob expansion reads before it stops; a monorepo with very wide folders can raise it, and the ceiling matches the secret walk so a guard still answers before its deadline");
    GUARD_SECRET_ENTRY_LIMIT = security("limits.guard_secret_entry_limit", "guard secret entry limit", "entries", 65_536, 4096, 262_144, "directory entries a secret-path walk reads before it refuses; a large monorepo can raise it, and the ceiling keeps a cold walk on a slow disk inside the guard deadline");
    GUARD_RUN_DIR_LIMIT = security("limits.guard_run_dir_limit", "guard run dir limit", "entries", 64, 8, 256, "directories one line can move to before the guard reports the directory as unknown; a generated script with many `cd` steps can raise it");
    GUARD_SED_SCRIPT_BYTES = security("limits.guard_sed_script_bytes", "guard sed script", BYTES, 1 << 24, 65_536, 16 * 1024 * 1024, "the largest `sed -f` script the guard reads; a larger one is refused, and the ceiling is the default because the guard holds the script in memory");
    GUARD_TARGET_CANDIDATES = security("limits.guard_target_candidates", "guard target candidates", "entries", 8, 2, 16, "directory candidates for one git target before it counts as unknown; each candidate costs a repository read on a slow host");
    EDIT_GUARD_WALK_LIMIT = security("limits.edit_guard_walk_limit", "edit guard walk limit", "entries", 4096, 64, 4096, "entries of a protected directory walked before the walk stands for the whole directory; past it the guard treats the directory as protected, and the ceiling is the default");
    GUARD_SCRIPT_READINGS = fixed(LimitClass::Security, "guard script readings", "readings", 16, BOUND);
    DELETION_GLOB_LIMIT = fixed(LimitClass::Security, "deletion reader glob limit", "entries", 4096, BOUND);
    GUARD_GIT_CONFIG_BYTES = security("limits.guard_git_config_bytes", "guard git config", BYTES, 4 * 1024 * 1024, 1024 * 1024, 64 * 1024 * 1024, "the most git configuration the guard reads before it refuses; a host with very large includes can raise it, and the ceiling bounds what one guard holds in memory");
    GUARD_DIR_SET_LIMIT = security("limits.guard_dir_set_limit", "guard dir set limit", "entries", 64, 8, 64, "directories a directory set lists before it reads as unknown; the ceiling is the default because every later step is read against each one");
    GUARD_ROOT_READ_LIMIT = security("limits.guard_root_read_limit", "guard root read limit", "entries", 4096, 64, 16_384, "directories read below one root before the root counts as reaching a class file; a monorepo can raise it, and the ceiling keeps a slow disk inside the guard deadline");
    REMOTE_ANSWER_BYTES = security("limits.remote_answer_bytes", "remote answer", BYTES, 16 << 20, 1024 * 1024, 64 * 1024 * 1024, "how much a remote may say; a repository with very many refs can raise it, and the ceiling bounds what a hostile remote can make the process hold");
    GLOB_PATH_BYTES = security("limits.glob_path_bytes", "glob path", BYTES, 32_000, 4096, 32_000, "the longest path the matcher scans against one pattern; it sits under the Windows path limit so every host gives the same refusal past it (TSK-1258), so the ceiling is the default");
    SANDBOX_SETTINGS_READ_BYTES = operational("limits.sandbox_settings_read_bytes", "sandbox settings read", BYTES, 16 * 1024 * 1024, 1024, "the most bytes read from one settings file by `codeflow doctor`, which only reports");
    DOCTOR_PROBE_TIMEOUT_SECONDS = operational("limits.doctor_probe_timeout_seconds", "doctor probe timeout", SECONDS, 5, 1, "how long `codeflow doctor` waits for one runner probe; a probe past it is reported and blocks nothing");
    GUARD_SCRIPT_REPARSE_DEPTH = fixed(LimitClass::Security, "guard script reparse depth", "levels", 16, BOUND);
    GUARD_GIT_READ_SECONDS = security("limits.guard_git_read_seconds", "guard git read wait", SECONDS, 3, 1, 5, "one local git read inside a guard; no answer in time is doubt, and the guard blocks on doubt; a slow disk can raise it, and the ceiling keeps several reads inside the guard deadline");
}
