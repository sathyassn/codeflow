//! A bounded controller for execution, process-tree termination and pipe collection.
use super::deadline::{Deadline, TargetStatus};
use super::{CommandOutcome, TestingError, POLL_INTERVAL};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Scoped CPU observation. Exit-sensitive anomalies are re-checked before
/// they invalidate termination evidence from the original group.
pub(super) struct CpuObservation {
    pub cpu: Result<u64, String>,
    pub membership_uncertain: bool,
}

pub(super) fn run(
    cmd: Command,
    target_name: &str,
    deadline: Deadline,
    emit_progress: bool,
    supervised: bool,
    hand_over: bool,
) -> Result<CommandOutcome, TestingError> {
    run_observed(
        cmd,
        target_name,
        deadline,
        emit_progress,
        supervised,
        hand_over,
        None::<fn(Duration) -> Option<CpuObservation>>,
    )
}

#[allow(clippy::too_many_lines)]
pub(super) fn run_observed(
    mut cmd: Command,
    target_name: &str,
    mut deadline: Deadline,
    emit_progress: bool,
    supervised: bool,
    hand_over: bool,
    mut observe: Option<impl FnMut(Duration) -> Option<CpuObservation>>,
) -> Result<CommandOutcome, TestingError> {
    let spawn_err = |error: std::io::Error| TestingError::CommandSpawnError {
        target: target_name.into(),
        message: error.to_string(),
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(super::target_job::CREATE_SUSPENDED);
    }
    #[cfg(unix)]
    let mut watcher = if supervised {
        Some(super::group_watch::GroupWatch::prepare(&mut cmd, hand_over).map_err(spawn_err)?)
    } else {
        None
    };

    #[cfg(target_os = "linux")]
    let mut cgroup = if deadline.info.heavy && observe.is_none() {
        super::cgroup::prepare(&mut cmd, &mut deadline).map_err(spawn_err)?
    } else {
        None
    };
    #[cfg(target_os = "macos")]
    if deadline.info.heavy {
        deadline.unavailable("macOS complete descendant accounting unavailable".into());
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    if deadline.info.heavy {
        deadline.unavailable("platform CPU accounting unavailable".into());
    }

    let grace = deadline.cleanup_grace;
    let start = Instant::now();
    let mut child = super::spawn_inheriting_fences(&mut cmd, hand_over).map_err(spawn_err)?;
    let mut membership_uncertain: Option<String> = None;
    let mut status = None;
    #[cfg(unix)]
    if let Some(watcher) = &mut watcher {
        watcher.spawned();
    }
    #[cfg(target_os = "linux")]
    if let Some(group) = &mut cgroup {
        if let Err(error) = group.mark_launched(child.id()) {
            let definite = (!error.may_be_exit).then(|| error.to_string());
            let observation = CpuObservation {
                cpu: Err(error.to_string()),
                membership_uncertain: true,
            };
            let observation = recheck_observation(&mut child, &mut status, observation);
            if let Some(reason) = definite {
                membership_uncertain.get_or_insert(reason);
            }
            if let Some(observation) = observation {
                let reason = observation
                    .cpu
                    .expect_err("a failed launch observation carries a reason");
                membership_uncertain = Some(reason.clone());
                deadline.unavailable(reason);
            }
        }
    }
    deadline.info.target_pid = child.id();
    #[cfg(unix)]
    {
        let pid = child.id();
        // Metadata must not postpone observing this already-running child.
        let record_by = start + deadline.ceiling.min(grace);
        if let Err(error) = complete_bookkeeping_before(record_by, move |until| {
            crate::testing::gate_guard::target_group_started_before(pid, until)
        }) {
            deadline.info.cleanup_details.push(error);
        }
    }
    // On Windows the target's tree ends with this process: the job is
    // closed when the gate exits or is killed. The OS gives no order between
    // that and freeing the gate's own locks, so while this process holds a
    // fence a watcher outside the job holds it too and frees it only once
    // the job is empty (TSK-1318).
    // A job failure ends the still-suspended child before its command runs
    // and fails the spawn (TSK-142 AC-3), as it did before supervision.
    #[cfg(windows)]
    let mut job = Some(super::adopt_or_stop(
        super::target_job::TargetJob::adopt(&child),
        target_name,
        || {
            let _ = child.kill();
            let _ = child.wait();
        },
    )?);
    #[cfg(windows)]
    if deadline.info.heavy {
        deadline.info.cpu_source = Some("Windows job user plus kernel CPU".into());
    }
    let safe_name = crate::git::display_text(target_name);
    if supervised {
        if target_name == "reserve" {
            progress!("[codeflow reserve] started process {}", child.id());
        } else {
            progress!(
                "[codeflow test] supervising heavy target '{safe_name}' in process group {}",
                child.id()
            );
        }
    }
    if let Some(reason) = &deadline.info.early_stall_unavailable {
        progress!("[codeflow test] {safe_name}: early stall unavailable: {reason}");
    }
    let mut stdout = super::output::Output::new(child.stdout.take());
    let mut stderr = super::output::Output::new(child.stderr.take());
    let mut stop = None;
    let mut next_sample = Duration::ZERO;
    let mut termination_confirmed;
    #[cfg(unix)]
    let mut watcher_finished = watcher.is_none();
    let mut cleanup_errors = Vec::new();
    let mut termination_requested = false;
    #[cfg(unix)]
    let mut group_signalled = false;

    loop {
        let now = start.elapsed();
        // The deadline wins over a successful exit first observed too late.
        if now >= deadline.ceiling {
            if matches!(
                deadline.info.status,
                TargetStatus::Passed | TargetStatus::Failed
            ) {
                deadline.info.status = TargetStatus::TimedOut;
            }
            stop.get_or_insert(now);
        }
        let name = emit_progress.then_some(safe_name.as_str());
        let activity = stdout.poll(name) | stderr.poll(name);
        if activity {
            deadline.output(start.elapsed());
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(error) => {
                    cleanup_errors.push(format!("cannot observe child exit: {error}"));
                    stop.get_or_insert(start.elapsed());
                }
            }
        }
        let now = start.elapsed();
        if stop.is_none()
            && deadline.info.heavy
            && status.is_none()
            && (now >= next_sample || deadline.decision(now) == Some(TargetStatus::Stalled))
        {
            #[cfg(target_os = "linux")]
            let mut definite_membership = None;
            let observation = observe
                .as_mut()
                .and_then(|sample| sample(now))
                .unwrap_or_else(|| {
                    #[cfg(windows)]
                    let cpu = job
                        .as_ref()
                        .ok_or_else(|| "job accounting unavailable".into())
                        .and_then(|job| job.accounting().map(|v| v.0).map_err(|e| e.to_string()));
                    #[cfg(target_os = "linux")]
                    let (cpu, lost_membership) = cgroup.as_ref().map_or_else(
                        || (Err("no delegated cgroup".into()), false),
                        |group| match group.sample(child.id()) {
                            Ok(cpu) => (Ok(cpu), false),
                            Err(error) => {
                                if error.membership_uncertain && !error.may_be_exit {
                                    definite_membership = Some(error.to_string());
                                }
                                (Err(error.to_string()), error.membership_uncertain)
                            }
                        },
                    );
                    #[cfg(not(any(windows, target_os = "linux")))]
                    let cpu = Err("platform complete accounting unavailable".into());
                    #[cfg(not(target_os = "linux"))]
                    let lost_membership = false;
                    CpuObservation {
                        cpu,
                        membership_uncertain: lost_membership,
                    }
                });
            let observation = recheck_observation(&mut child, &mut status, observation);
            #[cfg(target_os = "linux")]
            if let Some(reason) = definite_membership {
                membership_uncertain.get_or_insert(reason);
            }
            if let Some(observation) = observation {
                let old_reason = deadline.info.early_stall_unavailable.is_some();
                if observation.membership_uncertain {
                    membership_uncertain.get_or_insert_with(|| {
                        observation
                            .cpu
                            .as_ref()
                            .err()
                            .cloned()
                            .unwrap_or_else(|| "target cgroup membership became uncertain".into())
                    });
                }
                deadline.cpu(now, observation.cpu);
                if !old_reason {
                    if let Some(reason) = &deadline.info.early_stall_unavailable {
                        progress!("[codeflow test] {safe_name}: early stall unavailable: {reason}");
                    }
                }
            }
            next_sample = now.saturating_add(Duration::from_secs(1));
        }
        if let Some(cause) = deadline
            .decision(now)
            .filter(|cause| *cause != TargetStatus::Stalled || status.is_none())
        {
            if matches!(
                deadline.info.status,
                TargetStatus::Passed | TargetStatus::Failed
            ) {
                deadline.info.status = cause;
            }
            stop.get_or_insert(now);
        }
        if status.is_some() {
            stop.get_or_insert(now);
        }

        let tests = stderr
            .tests_done
            .as_deref()
            .or(stdout.tests_done.as_deref())
            .unwrap_or("unknown");
        if let Some(line) = deadline.heartbeat(now, tests) {
            progress!("[codeflow test] {safe_name}: {line}");
        }
        if let Some(stopped) = stop {
            // Issue termination once, without spawning a blocking kill helper.
            if !termination_requested {
                termination_requested = true;
                #[cfg(unix)]
                if let Some(watcher) = &mut watcher {
                    if let Err(error) = watcher.signal() {
                        cleanup_errors.push(format!("watcher signal: {error}"));
                    }
                }
                #[cfg(unix)]
                if status.is_none() {
                    // SAFETY: a live owned child reserves the process-group identity.
                    if unsafe {
                        libc::kill(
                            -i32::try_from(child.id()).unwrap_or(i32::MAX),
                            libc::SIGKILL,
                        )
                    } != 0
                    {
                        cleanup_errors
                            .push(format!("group kill: {}", std::io::Error::last_os_error()));
                    }
                }
                #[cfg(windows)]
                if let Some(job) = &job {
                    job.terminate();
                }
                #[cfg(target_os = "linux")]
                if let Some(group) = &cgroup {
                    if let Err(error) = group.kill() {
                        if error.membership_uncertain {
                            membership_uncertain.get_or_insert_with(|| error.to_string());
                        }
                        cleanup_errors.push(format!("cgroup kill unavailable: {error}"));
                    }
                }
                if status.is_none() {
                    let _ = child.kill();
                }
            }
            #[cfg(unix)]
            if let Some(watcher) = &mut watcher {
                match watcher.finished() {
                    Ok(value) => watcher_finished = value,
                    Err(error) => {
                        cleanup_errors.push(format!("watcher confirmation: {error}"));
                    }
                }
                // A watcher closes every descriptor up to the limit before it
                // reads the lifetime pipe. At a large limit on a loaded host
                // that can outlast the grace. Halfway through, end the group
                // here instead. While the watcher has not finished, the group
                // id is reserved: by the watcher until it leaves the group,
                // then by its sentinel until the watcher has killed the group
                // (TSK-1318). Past that kill this signal is redundant.
                if !watcher_finished && !group_signalled && now.saturating_sub(stopped) >= grace / 2
                {
                    group_signalled = true;
                    // SAFETY: the unfinished watcher keeps the group id reserved.
                    if unsafe {
                        libc::kill(
                            -i32::try_from(child.id()).unwrap_or(i32::MAX),
                            libc::SIGKILL,
                        )
                    } != 0
                    {
                        let error = std::io::Error::last_os_error();
                        // The group already ended: the watcher's pipe confirms it.
                        if error.raw_os_error() != Some(libc::ESRCH) {
                            cleanup_errors.push(format!("group kill at half grace: {error}"));
                        }
                    }
                }
            }
            #[cfg(unix)]
            {
                termination_confirmed = status.is_some() && watcher_finished;
            }
            // The watcher's exit counts too, so the clean path leaves no fence
            // behind. A watcher still running at the grace keeps its fences and
            // the target is reported unconfirmed (TSK-1318).
            #[cfg(windows)]
            {
                termination_confirmed = status.is_some()
                    && job.as_mut().is_none_or(super::target_job::TargetJob::ended);
            }
            #[cfg(target_os = "linux")]
            if let Some(group) = &cgroup {
                match group.populated() {
                    Ok(populated) => termination_confirmed &= !populated,
                    Err(error) => {
                        membership_uncertain.get_or_insert_with(|| {
                            format!("cannot confirm target cgroup empty: {error}")
                        });
                        termination_confirmed = false;
                    }
                }
            }
            let scoped_processes_ended = termination_confirmed;
            termination_confirmed &= membership_uncertain.is_none();
            // An empty original scope cannot account for observed escapes.
            // Once that scope and its pipes end there is no more proof to wait
            // for here; return unconfirmed and retain the group evidence.
            if scoped_processes_ended && stdout.finished() && stderr.finished() {
                if start.elapsed() >= deadline.ceiling
                    && deadline.info.status == TargetStatus::Passed
                {
                    deadline.info.status = TargetStatus::TimedOut;
                }
                break;
            }
            let grace_elapsed = now.saturating_sub(stopped) >= grace;
            let absolute = deadline.ceiling.saturating_add(grace);
            if grace_elapsed || now >= absolute {
                if !stdout.finished() || !stderr.finished() {
                    cleanup_errors.push("output pipe remained open at drain deadline".into());
                    termination_confirmed = false;
                }
                break;
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    stdout.cancel();
    stderr.cancel();
    for error in [stdout.error.as_ref(), stderr.error.as_ref()]
        .into_iter()
        .flatten()
    {
        cleanup_errors.push(format!("output collection: {error}"));
    }
    if let Some(reason) = &membership_uncertain {
        cleanup_errors.push(format!("termination membership uncertain: {reason}"));
    }
    if !termination_confirmed {
        cleanup_errors.push("process-tree termination unconfirmed".into());
    }
    #[cfg(unix)]
    if termination_confirmed {
        let finish_by = start
            .checked_add(
                stop.expect("cleanup follows a stop")
                    .min(deadline.ceiling)
                    .saturating_add(grace),
            )
            .unwrap_or_else(Instant::now);
        let pid = child.id();
        #[cfg(target_os = "linux")]
        let group = cgroup.take();
        if let Err(error) = complete_bookkeeping_before(finish_by, move |finish_by| {
            let mut failures = Vec::new();
            if Instant::now() >= finish_by {
                return Err("target group bookkeeping reached the shared deadline".into());
            }
            #[cfg(target_os = "linux")]
            if let Some(group) = group {
                if let Err(error) = group.remove_empty() {
                    failures.push(format!("cgroup cleanup: {error}"));
                }
            }
            if let Err(error) =
                crate::testing::gate_guard::target_group_finished_before(pid, finish_by)
            {
                failures.push(error);
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(failures.join("; "))
            }
        }) {
            cleanup_errors.push(error);
        }
    }
    let ordinary_exit = status
        .as_ref()
        .and_then(std::process::ExitStatus::code)
        .unwrap_or(-1);
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.as_ref().and_then(std::process::ExitStatus::signal)
    };
    #[cfg(not(unix))]
    let signal = None;
    if deadline.info.status == TargetStatus::Passed && ordinary_exit != 0 {
        deadline.info.status = TargetStatus::Failed;
    }
    if deadline.info.status == TargetStatus::Passed
        && (!cleanup_errors.is_empty() || !deadline.info.cleanup_details.is_empty())
    {
        deadline.info.status = TargetStatus::CleanupIncomplete;
    }
    deadline.info.cleanup_details.extend(cleanup_errors);
    let timed_out = matches!(
        deadline.info.status,
        TargetStatus::Stalled | TargetStatus::TimedOut
    );
    let exit_code = if timed_out {
        super::TIMEOUT_EXIT_CODE
    } else if deadline.info.status == TargetStatus::CleanupIncomplete {
        -1
    } else {
        ordinary_exit
    };
    if status.is_none() {
        // Preserve reaping ownership after bounded return. The controller never
        // joins this helper; termination remains explicitly unconfirmed.
        std::thread::spawn(move || loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
            }
        });
    }
    Ok(CommandOutcome {
        exit_code,
        signal,
        stdout: stdout.rendered(),
        stderr: stderr.rendered(),
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
        supervision: deadline.info,
        termination_unconfirmed: !termination_confirmed,
    })
}

/// A child can exit after the loop's first wait and before its membership
/// observation. Discard only that new observation; previously confirmed
/// uncertainty belongs to the controller and is never cleared here.
fn recheck_observation(
    child: &mut std::process::Child,
    status: &mut Option<std::process::ExitStatus>,
    mut observation: CpuObservation,
) -> Option<CpuObservation> {
    if status.is_some() {
        return None;
    }
    if observation.membership_uncertain {
        match child.try_wait() {
            Ok(Some(exit)) => {
                *status = Some(exit);
                return None;
            }
            Ok(None) => {}
            Err(error) => {
                let cause = observation
                    .cpu
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "membership uncertain".into());
                observation.cpu = Err(format!("cannot recheck child exit after membership observation: {error}; observation: {cause}"));
            }
        }
    }
    Some(observation)
}

/// One absolute wait budget covers bookkeeping, every record and its mutex.
#[cfg(unix)]
pub(crate) fn complete_bookkeeping_before(
    finish_by: Instant,
    bookkeeping: impl FnOnce(Instant) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    if Instant::now() >= finish_by {
        return Err("target group bookkeeping reached the shared deadline".into());
    }
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    // Filesystem I/O cannot be interrupted safely. Never wait for it beyond
    // the supervisor's remaining budget, or join its worker on return.
    std::thread::Builder::new()
        .name("codeflow-group-records".into())
        .spawn(move || {
            let _ = send.send(bookkeeping(finish_by));
        })
        .map_err(|error| format!("cannot start target group bookkeeping: {error}"))?;
    let result = receive
        .recv_timeout(finish_by.saturating_duration_since(Instant::now()))
        .map_err(|error| match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => {
                "target group bookkeeping incomplete at shared deadline".to_string()
            }
            std::sync::mpsc::RecvTimeoutError::Disconnected => {
                "target group bookkeeping worker ended without confirmation".to_string()
            }
        })?;
    if Instant::now() >= finish_by {
        return Err("target group bookkeeping completed after the shared deadline".into());
    }
    result
}
