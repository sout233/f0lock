//! Adapters for logging to a windows debugger. Split off into a module to avoid littering `#[cfg]`
//! attributes all over the place.

use std::io::Write;

use windows::Win32::System::Diagnostics::Debug::{IsDebuggerPresent, OutputDebugStringW};
use windows::core::PCWSTR;

/// A shim to provide a writes `write!()` implementation that writes to the Windows debugger using
/// `OutputDebugStringW()`. Provides line-based buffering since `OutputDebugString` normally
/// immediately flushes. Since this needs to convert the bytes input from UTF-8 to UTF-16, this is
/// not going to be particularly efficient.
///
/// # Notes
///
/// This provides a general [`Write`] interface, but this only supports writing valid UTF-8 text.
#[derive(Debug)]
pub struct WinDbgWriter {
    /// Unwritten output. Will be flushed either when `flush` is called, or when a carriege return
    /// is printed.
    buffer: Vec<u8>,
    /// An intermediary buffer used to convert UTF-8 text from `buffer` into UTF-16 so it can be
    /// output using `OutputDebugStringW()`. `OutputDebugStringA()` can be used with UTF-8 text, but
    /// only in very recent Windows versions.
    utf16_buffer: Vec<u16>,
}

impl Default for WinDbgWriter {
    fn default() -> Self {
        Self {
            // This is the default capacity used for `BufWriter`:
            // https://github.com/rust-lang/rust/blob/5423745db8b434fcde54888b35f518f00cce00e4/library/std/src/sys_common/io.rs#L1-L3
            buffer: Vec::with_capacity(8 * 1024),
            utf16_buffer: Vec::with_capacity(8 * 1024),
        }
    }
}

impl Drop for WinDbgWriter {
    fn drop(&mut self) {
        // Make sure to write any remaining partial lines to the debugger console when the object is
        // dropped
        let _ = self.flush();
    }
}

impl Write for WinDbgWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        const LINE_FEED: u8 = b'\n';

        // We'll buffer writes to only flush on newlines because `IsDebuggerPresent()` is unbuffered
        // and the way the logs are written assumes buffered writes
        // TODO: This can be optimized a bit by only flushing at the last line feed in `buf`, if
        //       `buf` contains multiple line feeds
        for line in buf.split_inclusive(|c| c == &LINE_FEED) {
            self.buffer.extend_from_slice(line);
            if line.last() == Some(&LINE_FEED) {
                self.flush()?;
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        // As explained above, we'll use `OutputDebugStringW()` instead of `OutputDebugStringA()` to
        // better support legacy platforms. This requires us to convert the UTF-8 buffer into UTF-16
        // first.
        self.utf16_buffer.clear();
        match std::str::from_utf8(&self.buffer) {
            Ok(buffer_str) => self.utf16_buffer.extend(buffer_str.encode_utf16()),
            Err(err) => self
                .utf16_buffer
                .extend(format!("ERROR: Invalid UTF-8 in input: {err}").encode_utf16()),
        }
        self.buffer.clear();

        // The UTF-16 buffer is treated as a null terminated string
        self.utf16_buffer.push(0);
        unsafe { OutputDebugStringW(PCWSTR::from_raw(self.utf16_buffer.as_ptr())) };

        Ok(())
    }
}

/// Whether the windows debugger is currently attached.
pub fn attached() -> bool {
    unsafe { IsDebuggerPresent().as_bool() }
}
