# nice-log

[![crates.io](https://img.shields.io/crates/v/nice-log)](https://crates.io/crates/nice-log)
[![docs.rs](https://img.shields.io/docsrs/nice-log)](https://docs.rs/nice-log)

A logger for the [log](https://crates.io/crates/log) crate made specifically to
cater to the needs of the [nice-plug](https://codeberg.org/BillyDM/nice-plug)
plugin framework.

## Features

- Log messages are formatted similarly to
  [simplelog](https://crates.io/crates/simplelog). Because simplelog is great.
- By default the log output goes to STDERR, unless a Windows debugger is
  attached. In that case the output is sent to the Windows debugger using the
  [`OutputDebugString()`](https://learn.microsoft.com/en-us/windows/win32/api/debugapi/nf-debugapi-outputdebugstringw)
  function. This check is performed at runtime to accommodate debuggers being
  attached to already running processes.
- The log's output target can be changed by setting the `NICE_LOG` environment
  variable:

  - A value of `stderr` causes the log to be printed to STDERR.
  - A value of `windbg` causes the log to be output to the Windows debugger.
  - Anything else is interpreted as a file name, which causes the log to be
    written to that file instead.

  The latter two options are useful on Windows where accessing the standard IO
  streams may be difficult.

- If `NICE_LOG` is not set, then a dynamic logging output target is used instead.
  On Windows this causes log messages to be sent to the Windows debugger when
  one is attached. This check is done just before printing the message to make
  it possible to attach a debugger to a running process. When the debugger is
  not attached the output goes directly to STDERR. On non-Windows platforms
  STDERR is always used.
- _(not yet implemented)_ The logger's output can be changed to output to a custom function after the
  logger has been created. This makes it possible to integrate with external
  logging APIs that are not yet available when the logger is first initialized,
  like the CLAP plugin API's [logging
  extension](https://github.com/free-audio/clap/blob/main/include/clap/ext/log.h).
  - This API can be used multiple times, for instance by multiple CLAP plugin
    instances that have all received their own logger instance from the host. In
    that case all registered loggers are kept track of in a queue, and the first
    still active one is used.
  - If `NICE_LOG` was set explicitly, then this is honored and the regular
    behavior won't be overridden.
- Because nice-log is opinionated for use with nice-plug, it only exposes the bare
  minimum of settings by design. The only configuration options are to control
  the output target and to filter out log messages based on the crate and module
  they're sent from.
- The logger itself does not try to be realtime-safe. It does however try to be
  as performant as reasonably possible.
  - The logger also tries to detect reentrant logging calls from the same
    thread, in which case it will log using a new logger instance based on the
    `NICE_LOG` environment variable. This is very specific behavior is needed to
    use [`assert_no_alloc`](https://crates.io/crates/assert_no_alloc/1.1.2)'s
    log feature as an allocation while writing to the logger (for instance
    because the buffers aren't large enough to format all of the logger's text)
    should also be logged using the logger, but in that case the log target's
    mutex will already be locked.
