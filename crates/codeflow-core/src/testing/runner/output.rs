//! Nonblocking, bounded pipe collection. No EOF or reader join can hold the gate open.
use std::io::{self, Read};

#[cfg(unix)]
pub(super) trait Pipe: Read + std::os::fd::AsRawFd {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsRawFd> Pipe for T {}
#[cfg(windows)]
pub(super) trait Pipe: Read + std::os::windows::io::AsRawHandle {}
#[cfg(windows)]
impl<T: Read + std::os::windows::io::AsRawHandle> Pipe for T {}

pub(super) struct Output<R> {
    pipe: Option<R>,
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub error: Option<String>,
    partial: Vec<u8>,
    pub tests_done: Option<String>,
}

impl<R: Pipe> Output<R> {
    pub fn new(pipe: Option<R>) -> Self {
        let this = Self {
            pipe,
            bytes: Vec::new(),
            truncated: false,
            error: None,
            partial: Vec::new(),
            tests_done: None,
        };
        #[cfg(unix)]
        let this = {
            let mut this = this;
            if let Some(pipe) = &this.pipe {
                // SAFETY: the descriptor belongs to this reader and remains open.
                let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
                if flags < 0
                    || unsafe {
                        libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                    } < 0
                {
                    this.error = Some(io::Error::last_os_error().to_string());
                    this.pipe = None;
                }
            }
            this
        };
        this
    }

    pub fn finished(&self) -> bool {
        self.pipe.is_none()
    }

    pub fn cancel(&mut self) {
        self.pipe = None;
    }

    /// A finite read batch lets even an endlessly chatty child reach its deadline.
    pub fn poll(&mut self, name: Option<&str>) -> bool {
        let mut received = false;
        let mut chunk = [0; 8192];
        for _ in 0..64 {
            let Some(pipe) = self.pipe.as_mut() else {
                break;
            };
            #[cfg(windows)]
            let limit = match available(pipe) {
                Ok(0) => break,
                Ok(n) => n.min(chunk.len()),
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                    self.pipe = None;
                    break;
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.pipe = None;
                    break;
                }
            };
            #[cfg(unix)]
            let limit = chunk.len();
            match pipe.read(&mut chunk[..limit]) {
                Ok(0) => {
                    self.pipe = None;
                    break;
                }
                Ok(n) => {
                    received = true;
                    self.ingest(&chunk[..n], name);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.pipe = None;
                    break;
                }
            }
        }
        received
    }

    fn ingest(&mut self, bytes: &[u8], name: Option<&str>) {
        self.bytes.extend_from_slice(bytes);
        if self.bytes.len() > super::OUTPUT_CAPTURE_LIMIT {
            self.bytes
                .drain(..self.bytes.len() - super::OUTPUT_CAPTURE_LIMIT);
            self.truncated = true;
        }
        for &byte in bytes {
            if byte == b'\n' || self.partial.len() == 8192 {
                let line = String::from_utf8_lossy(&self.partial);
                if let Some(count) = completed_count(&line) {
                    self.tests_done = Some(count);
                }
                if let Some(name) = name {
                    if line.contains("Starting")
                        || line.contains("Summary")
                        || line.starts_with("journey gate:")
                        || line.contains(" PASS ")
                        || line.contains(" FAIL ")
                    {
                        progress!("[codeflow test] {name}: {line}");
                    }
                }
                self.partial.clear();
            }
            if byte != b'\n' {
                self.partial.push(byte);
            }
        }
    }

    pub fn rendered(&self) -> String {
        let text = String::from_utf8_lossy(&self.bytes);
        if self.truncated {
            format!(
                "[codeflow test] output truncated; retained last {} bytes\n{text}",
                super::OUTPUT_CAPTURE_LIMIT
            )
        } else {
            text.into_owned()
        }
    }
}

fn completed_count(line: &str) -> Option<String> {
    let (_, tail) = line.rsplit_once('(')?;
    let (numbers, _) = tail.split_once(')')?;
    let (done, total) = numbers.split_once('/')?;
    let done = done.trim_ascii().parse::<u64>().ok()?;
    let total = total.trim_ascii().parse::<u64>().ok()?;
    (done <= total).then(|| format!("{done} of {total}"))
}

#[cfg(windows)]
fn available(pipe: &impl Pipe) -> io::Result<usize> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    let mut count = 0;
    // SAFETY: only queries our owned pipe; no blocking read occurs until bytes exist.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle() as HANDLE,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &raw mut count,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(count as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_counts_are_explicit_and_bounded() {
        assert_eq!(
            completed_count("PASS (1189/6827) name"),
            Some("1189 of 6827".into())
        );
        for text in ["busy", "PASS (8/2)", "PASS (x/2)"] {
            assert_eq!(completed_count(text), None);
        }
    }
}
