//! The logger's output targets.

use std::fmt::Debug;
use std::fs::File;
use std::io::{self, Stderr, Write};
use std::path::Path;
use std::sync::Mutex;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

#[cfg(windows)]
mod windbg;

/// The environment variable for controlling the logging behavior.
const NICE_LOG_ENV: &str = "NICE_LOG";

/// Convenience method for [`NiceLogWriter::from_environment()`] that wraps the
/// writer in a [`Mutex`].
///
/// This custom writer reads from the `NICE_LOG` environment variable to set the
/// output target.
///
/// - A value of `stderr` causes the log to be printed to STDERR.
/// - A value of `windbg` causes the log to be output to the Windows debugger.
/// - Anything else is interpreted as a file name, which causes the log to be
///   written to that file instead.
///
/// If `NICE_LOG` is not set, then a dynamic logging output target is used instead.
/// On Windows this causes log messages to be sent to the Windows debugger when
/// one is attached, then falls back to STDERR. All other platforms use STDERR.
pub fn writer_from_env() -> Mutex<NiceLogWriter> {
    Mutex::new(NiceLogWriter::from_environment())
}

/// A custom struct that implements [`Write`] that can be attached to a logging subscriber
/// (such as `tracing-subscriber`)
pub enum NiceLogWriter {
    /// The default logging target on Windows. This checks whether a Windows debugger is attached
    /// before logging. If there is a debugger, then the message is written using
    /// `OutputDebugString()`. Otherwise the message is written to STDERR instead.
    #[cfg(windows)]
    StderrOrWinDbg(Stderr, windbg::WinDbgWriter),
    /// Writes directly to STDERR. The default logging target on non-Windows platforms. May use
    /// colors colors depending on the environment.
    Stderr(Stderr),
    /// Outputs to the Windows debugger using `OutputDebugString()`.
    #[cfg(windows)]
    WinDbg(windbg::WinDbgWriter),
    /// Writes to the file.
    File(NonBlocking, WorkerGuard),
}

impl Debug for NiceLogWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(_, windbg) => f
                .debug_tuple("StderrOrWinDbg")
                .field(&"<stderr stream>")
                .field(windbg)
                .finish(),
            NiceLogWriter::Stderr(_) => f.debug_tuple("Stderr").field(&"<stderr stream>").finish(),
            #[cfg(windows)]
            NiceLogWriter::WinDbg(windbg) => f.debug_tuple("WinDbg").field(windbg).finish(),
            NiceLogWriter::File(file, _) => f.debug_tuple("File").field(file).finish(),
        }
    }
}

impl NiceLogWriter {
    /// This custom writer reads from the `NICE_LOG` environment variable to set the
    /// output target.
    ///
    /// - A value of `stderr` causes the log to be printed to STDERR.
    /// - A value of `windbg` causes the log to be output to the Windows debugger.
    /// - Anything else is interpreted as a file name, which causes the log to be
    ///   written to that file instead.
    ///
    /// If `NICE_LOG` is not set, then a dynamic logging output target is used instead.
    /// On Windows this causes log messages to be sent to the Windows debugger when
    /// one is attached, then falls back to STDERR. All other platforms use STDERR.
    pub fn from_environment() -> Self {
        let nice_log_env = std::env::var(NICE_LOG_ENV);
        let nice_log_env_str = nice_log_env.as_deref().unwrap_or("");
        if nice_log_env_str.eq_ignore_ascii_case("stderr") {
            return Self::new_stderr();
        }
        #[cfg(windows)]
        if nice_log_env_str.eq_ignore_ascii_case("windbg") {
            return Self::new_windbg();
        }
        if !nice_log_env_str.is_empty() {
            match Self::new_file_path(nice_log_env_str) {
                Ok(target) => return target,
                // TODO: Print this using the actual logger
                Err(err) => eprintln!(
                    "Could not open '{nice_log_env_str}' from NICE_LOG for logging, falling back \
                     to STDERR: {err}"
                ),
            }
        }

        #[cfg(windows)]
        return Self::new_stderr_or_windbg();
        #[cfg(not(windows))]
        return Self::new_stderr();
    }

    /// Construct an [`NiceLogWriter`] that writes to STDERR with optional color support
    /// determined by the environment. If a Windows debugger is attached when writing debug output,
    /// then the output is sent to the Windows debugger instead.
    #[cfg(windows)]
    pub fn new_stderr_or_windbg() -> Self {
        NiceLogWriter::StderrOrWinDbg(io::stderr(), windbg::WinDbgWriter::default())
    }

    /// Construct an [`NiceLogWriter`] that writes to STDERR with optional color support
    /// determined by the environment.
    pub fn new_stderr() -> Self {
        NiceLogWriter::Stderr(io::stderr())
    }

    /// Construct an [`NiceLogWriter`] that writes to the Windows debugger.
    #[cfg(windows)]
    pub fn new_windbg() -> Self {
        NiceLogWriter::WinDbg(windbg::WinDbgWriter::default())
    }

    /// Construct an [`NiceLogWriter`] for doing buffered writes to a file.
    pub fn new_file_path<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let file = File::options().create(true).append(true).open(path)?;
        let (writer, guard) = NonBlocking::new(file);

        Ok(Self::File(writer, guard))
    }

    /// Returns a writer that can be written to using the [`write!()`] and [`writeln!()`] macros.
    /// This writer can also be used to color the STDERR stream when outputting to an STDERR stream
    /// that supports colors. May perform a syscall to check whether the Windows debugger is
    /// attached so this should be reused for multiple `write!()` calls.
    ///
    /// Needs to be a single function since otherwise you'd need to borrow from this struct twice.
    pub fn writer(&mut self) -> &mut dyn Write {
        match self {
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(_, windbg) if windbg::attached() => windbg,
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(stderr, _) => stderr,
            NiceLogWriter::Stderr(stderr) => stderr,
            #[cfg(windows)]
            NiceLogWriter::WinDbg(windbg) => windbg,
            NiceLogWriter::File(file, _) => file,
        }
    }
}

impl Write for NiceLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(_, windbg) if windbg::attached() => windbg.write(buf),
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(stderr, _) => stderr.write(buf),
            NiceLogWriter::Stderr(stderr) => stderr.write(buf),
            #[cfg(windows)]
            NiceLogWriter::WinDbg(windbg) => windbg.write(buf),
            NiceLogWriter::File(file, _) => file.write(buf),
        }
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        match self {
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(_, windbg) if windbg::attached() => {
                windbg.write_vectored(bufs)
            }
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(stderr, _) => stderr.write_vectored(bufs),
            NiceLogWriter::Stderr(stderr) => stderr.write_vectored(bufs),
            #[cfg(windows)]
            NiceLogWriter::WinDbg(windbg) => windbg.write_vectored(bufs),
            NiceLogWriter::File(file, _) => file.write_vectored(bufs),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(_, windbg) if windbg::attached() => windbg.flush(),
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(stderr, _) => stderr.flush(),
            NiceLogWriter::Stderr(stderr) => stderr.flush(),
            #[cfg(windows)]
            NiceLogWriter::WinDbg(windbg) => windbg.flush(),
            NiceLogWriter::File(file, _) => file.flush(),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(_, windbg) if windbg::attached() => windbg.write_all(buf),
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(stderr, _) => stderr.write_all(buf),
            NiceLogWriter::Stderr(stderr) => stderr.write_all(buf),
            #[cfg(windows)]
            NiceLogWriter::WinDbg(windbg) => windbg.write_all(buf),
            NiceLogWriter::File(file, _) => file.write_all(buf),
        }
    }

    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> io::Result<()> {
        match self {
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(_, windbg) if windbg::attached() => {
                windbg.write_fmt(args)
            }
            #[cfg(windows)]
            NiceLogWriter::StderrOrWinDbg(stderr, _) => stderr.write_fmt(args),
            NiceLogWriter::Stderr(stderr) => stderr.write_fmt(args),
            #[cfg(windows)]
            NiceLogWriter::WinDbg(windbg) => windbg.write_fmt(args),
            NiceLogWriter::File(file, _) => file.write_fmt(args),
        }
    }
}
