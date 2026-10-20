# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic
Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1] - 2023-04-24

### Fixed

- Fixed getting the local time offset on Linux. The `time` crate normally does
  not allow this.

## [0.3.0] - 2023-03-21

### Added

- Added an option to always show the module target, even for error, warning, and
  info messages. This is useful for debugging and for setting up module filters.

## [0.2.0] - 2023-03-01

### Added

- The logger now detects reentrant logging calls and creates a new logger target
  when that happens. This is a very specific solution to a very specific
  problem. The `assert_no_alloc` crate can print backtraces when an allocation
  fails, and it can also do so using the `log` crate. But when it was a `log`
  call that caused the allocation failure in the first place, this will deadlock
  because the current thread already holds a lock on the output stream's mutex.
  This feature detects this specific situation and creates a second output
  stream when this happens.

## [0.1.0] - 2023-03-01

### Added

- Initial release with support for STDERR, file based, and dynamic windbg output
  targets.
