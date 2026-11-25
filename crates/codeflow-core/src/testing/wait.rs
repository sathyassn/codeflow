//! Structured evidence for contended gate locks. Each run owns its log;
//! worker threads explicitly inherit it, so concurrent embedded gates stay separate.
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

thread_local! {
    static CURRENT: RefCell<Option<WaitLog>> = const { RefCell::new(None) };
}

/// Wait evidence shared only by the threads of one gate.
#[derive(Clone, Default)]
pub struct WaitLog(Arc<Mutex<Vec<serde_json::Value>>>);

/// Restores the caller's wait log on scope exit.
pub struct WaitScope {
    previous: Option<WaitLog>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl WaitLog {
    /// The current gate's log, or a new empty log for an embedded caller.
    #[must_use]
    pub fn current() -> Self {
        CURRENT
            .with(|cell| cell.borrow().clone())
            .unwrap_or_default()
    }

    /// Attach this log to the current thread until the returned scope drops.
    #[must_use]
    pub fn enter(&self) -> WaitScope {
        WaitScope {
            previous: CURRENT.with(|cell| cell.replace(Some(self.clone()))),
            _thread: std::marker::PhantomData,
        }
    }

    /// Add optional evidence without changing the version-2 fields readers use.
    /// No wait key is emitted for an uncontended run.
    pub fn apply(&self, artifact: &mut serde_json::Value) {
        let records = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(first) = records.first() {
            let mut first = first.clone();
            if records.len() > 1 {
                first["events"] = serde_json::Value::Array(records.clone());
            }
            artifact["wait"] = first;
        }
    }
}

impl Drop for WaitScope {
    fn drop(&mut self) {
        CURRENT.with(|cell| {
            cell.replace(self.previous.take());
        });
    }
}

/// One contended acquisition. Drop records a refusal; success records acquisition.
pub struct Attempt {
    log: WaitLog,
    started: Instant,
    asked: f64,
    resource: String,
    holder: serde_json::Value,
    finished: bool,
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

impl Attempt {
    #[must_use]
    pub fn new(resource: impl Into<String>, record: &str) -> Self {
        let field = |key: &str| record.lines().find_map(|line| line.strip_prefix(key));
        let holder = field("pid=")
            .and_then(|pid| pid.parse::<u32>().ok())
            .map_or_else(
                || serde_json::Value::String("holder unknown".into()),
                |pid| {
                    serde_json::json!({
                        "pid": pid,
                        "worktree": field("dir="),
                        "mode": field("mode="),
                    })
                },
            );
        Self {
            log: WaitLog::current(),
            started: Instant::now(),
            asked: now(),
            resource: resource.into(),
            holder,
            finished: false,
        }
    }

    /// Holder and request time for a refusal or waiting message.
    #[must_use]
    pub fn description(&self) -> String {
        format!(
            "{}; holder {}; asked at {:.6}",
            self.resource, self.holder, self.asked
        )
    }

    /// Complete a successful acquisition.
    pub fn acquired(mut self) {
        self.record(Some(now()));
    }

    fn record(&mut self, acquired: Option<f64>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.log
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(serde_json::json!({
                "resource": self.resource, "wait_started": self.asked,
                "acquired": acquired, "seconds_waited": self.started.elapsed().as_secs_f64(),
                "holder": self.holder,
            }));
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        self.record(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncontended_runs_have_no_wait_block() {
        let log = WaitLog::default();
        let mut record = serde_json::json!({"schema_version":2});
        log.apply(&mut record);
        assert_eq!(record, serde_json::json!({"schema_version":2}));
    }

    #[test]
    fn acquired_and_refused_waits_keep_holder_and_request_time() {
        let log = WaitLog::default();
        let _scope = log.enter();
        Attempt::new("resource a", "pid=23\ndir=/fixture\nmode=quick\n").acquired();
        drop(Attempt::new("resource b", ""));
        let mut record = serde_json::json!({"schema_version":2});
        log.apply(&mut record);
        assert_eq!(record["wait"]["holder"]["pid"], 23);
        assert_eq!(record["wait"]["holder"]["worktree"], "/fixture");
        assert_eq!(record["wait"]["holder"]["mode"], "quick");
        assert!(record["wait"]["acquired"].is_number());
        assert!(record["wait"]["wait_started"].is_number());
        assert!(record["wait"]["events"][1]["acquired"].is_null());
        assert_eq!(record["wait"]["events"][1]["holder"], "holder unknown");
        assert_eq!(record["schema_version"], 2);
    }
}
