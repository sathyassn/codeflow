//! Run one unit test again in a child process whose limits are set to their
//! largest accepted values.
//!
//! A process installs its project limits once (`codeflow_limits::install`),
//! so a test that needs a project value runs that value in a child of the
//! test binary and leaves every other test on the defaults.

use std::time::{Duration, Instant};

/// The environment variable that marks the child run.
const CHILD: &str = "CODEFLOW_LARGEST_LIMIT_CHILD";

/// Time allowed past a bound for process start, kill and scheduling on a
/// loaded host; a wait that ignores its bound overshoots it by far more.
pub(crate) const SCHEDULING: Duration = Duration::from_secs(2);

/// In the child run, install each of `limits` at its largest accepted value
/// and return true. In the parent run, return false.
pub(crate) fn in_child_with_largest(limits: &[&codeflow_limits::Limit]) -> bool {
    if std::env::var_os(CHILD).is_none() {
        return false;
    }
    let values = limits.iter().map(|limit| {
        (
            limit.key.expect("only a settable limit can be installed"),
            limit.most(),
        )
    });
    assert!(
        codeflow_limits::install(values),
        "the child installs its limits first"
    );
    for limit in limits {
        assert_eq!(limit.value(), limit.most(), "{} not installed", limit.name);
    }
    true
}

/// Run the unit test named `test` (its path inside the crate) alone in a
/// child of this test binary, and return whether it passed within `within`.
/// A child still running at `within` is killed and counts as a failure, and
/// so does a run that did not find the test.
pub(crate) fn child_passes_within(test: &str, within: Duration) -> bool {
    use std::io::Read as _;
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1"])
        .env(CHILD, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + within;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut summary = String::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut summary)
                .unwrap();
            eprintln!("{summary}");
            return status.success() && summary.contains("1 passed");
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
