//! Draining a piped child stream (issue 100, TSK-282).
//!
//! A pipe holds a few hundred bytes on a host under pipe memory pressure and
//! 64 KiB on most others. A child whose output outgrows it blocks in `write`
//! until someone reads, so a test that waits for or polls the child before
//! reading hangs. `drain_pipe` and `drain_lines` are the helpers the source
//! check (`crates/codeflow-core/tests/pipe_drain_rule.rs`) accepts: hand one
//! `child.stdout.take()` or `child.stderr.take()` before the first wait.

use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{channel, Receiver};
use std::thread::JoinHandle;

/// Read `pipe` to the end on its own thread; `join` returns the bytes once the
/// child has closed it.
pub fn drain_pipe<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// Read `pipe` line by line on its own thread, sending each line as it
/// arrives, until the child closes it.
pub fn drain_lines<R: Read + Send + 'static>(pipe: Option<R>) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let Some(pipe) = pipe else { return };
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}
