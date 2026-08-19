use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use acelus_native_sys::ProcessGuard;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::invocation::Invocation;

pub const DEFAULT_LOG_CAPACITY: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not start {program}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("process supervision could not be established")]
    Supervision(#[from] acelus_native_sys::Error),

    #[error("waiting on the game process failed")]
    Wait {
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub stream: Stream,
    pub text: String,
}

#[derive(Debug)]
pub struct LogBuffer {
    lines: Mutex<VecDeque<LogLine>>,
    capacity: usize,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: Mutex::new(VecDeque::with_capacity(capacity.min(1024))),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&self, line: LogLine) {
        let mut lines = self.lines.lock().expect("the log buffer is never poisoned");
        if lines.len() == self.capacity {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    pub fn snapshot(&self) -> Vec<LogLine> {
        self.lines
            .lock()
            .expect("the log buffer is never poisoned")
            .iter()
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.lines
            .lock()
            .expect("the log buffer is never poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitReport {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub crashed: bool,
}

pub struct Session {
    id: String,
    pid: u32,
    child: Child,
    guard: ProcessGuard,
    log: Arc<LogBuffer>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("pid", &self.pid)
            .field("guarded", &self.guard.is_guarding())
            .finish()
    }
}

impl Session {
    pub async fn spawn(invocation: &Invocation, log_capacity: usize) -> Result<Self> {
        let mut command = Command::new(&invocation.program);
        command
            .args(&invocation.arguments)
            .current_dir(&invocation.working_directory)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(false);

        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                ProcessGuard::prepare_child().map_err(std::io::Error::other)?;
                Ok(())
            });
        }

        std::fs::create_dir_all(&invocation.working_directory).map_err(|source| Error::Spawn {
            program: invocation.program.display().to_string(),
            source,
        })?;

        let mut child = command.spawn().map_err(|source| Error::Spawn {
            program: invocation.program.display().to_string(),
            source,
        })?;

        let pid = child.id().unwrap_or_default();

        let mut guard = ProcessGuard::new()?;
        guard.adopt(pid)?;

        let log = Arc::new(LogBuffer::new(log_capacity));
        capture(child.stdout.take(), Stream::Stdout, log.clone());
        capture(child.stderr.take(), Stream::Stderr, log.clone());

        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            pid,
            child,
            guard,
            log,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn log(&self) -> &Arc<LogBuffer> {
        &self.log
    }

    pub fn is_guarded(&self) -> bool {
        self.guard.is_guarding()
    }

    pub async fn wait(&mut self) -> Result<ExitReport> {
        let status = self
            .child
            .wait()
            .await
            .map_err(|source| Error::Wait { source })?;

        Ok(report_from(status))
    }

    pub fn terminate(&self) -> Result<()> {
        Ok(self.guard.terminate()?)
    }
}

#[cfg(unix)]
fn report_from(status: std::process::ExitStatus) -> ExitReport {
    use std::os::unix::process::ExitStatusExt;

    let signal = status.signal();
    ExitReport {
        code: status.code(),
        signal,
        crashed: !status.success(),
    }
}

#[cfg(not(unix))]
fn report_from(status: std::process::ExitStatus) -> ExitReport {
    ExitReport {
        code: status.code(),
        signal: None,
        crashed: !status.success(),
    }
}

fn capture<R>(source: Option<R>, stream: Stream, log: Arc<LogBuffer>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let Some(source) = source else {
        return;
    };

    tokio::spawn(async move {
        let mut lines = BufReader::new(source).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            log.push(LogLine { stream, text });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_keeps_the_most_recent_lines_within_its_capacity() {
        let buffer = LogBuffer::new(3);
        assert!(buffer.is_empty());

        for n in 0..5 {
            buffer.push(LogLine {
                stream: Stream::Stdout,
                text: format!("line {n}"),
            });
        }

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].text, "line 2");
        assert_eq!(snapshot[2].text, "line 4");
    }

    #[test]
    fn a_zero_capacity_buffer_still_holds_a_line() {
        let buffer = LogBuffer::new(0);
        buffer.push(LogLine {
            stream: Stream::Stderr,
            text: "only".into(),
        });
        assert_eq!(buffer.len(), 1);
    }
}
