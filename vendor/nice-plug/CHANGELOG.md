# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic
Versioning](https://semver.org/spec/v2.0.0.html).

Since there is no stable release yet, the main purpose of this document in its current
state is to list breaking changes.

> Since nice-plug is now its own project, and the first release has been published
> to crates.io, this changelog has been reset. To see the old changelog, go to
> https://codeberg.org/RustAudio/nice-plug/src/commit/aefe2eac919aae5ad43f626d0fbd51748c7371ba/CHANGELOG.md

# nice-plug-egui 0.6.0 and nice-plug-iced 0.5.0

## Breaking Changes
* `nice-plug-egui` bumped to version `0.6.0`
* `nice-plug-iced` bumped to version `0.5.0`
* Updated `nice-plug-egui` and `nice-plug-iced` to `baseview` version `0.4.1`
* Updated `nice-plug-egui` to `egui` version `0.37.0`

## Added
* New settings in `nice-plug-egui` and `nice-plug-iced` to configure the poll timer interval

## Changed
* Updated examples to `baseview` version `0.4.1`

# nice-plug 0.4.2

## Added

Added a new `unsafe_standalone_dpi_fix` feature to `nice-plug`. This is used in conjuction with the new
`Editor::assume_standalone_in_process()` method added to `nice-plug-core` to fix broken dpi scaling issues on
Windows when running in standalone mode. (see: https://github.com/RustAudio/baseview/issues/321)

Editor implementations should call `baseview::assume_standalone_in_process()` inside of the
`Editor::assume_standalone_in_process()` method.

Enabling this feature is generally safe for most plugins. The only case where it *should not* be enabled is if
your plugin hosts other plugins.

More specifically, do *NOT* enable this if any of the following applies:
* Multiple `baseview` versions are present in the final binary
* `baseview` is being used in conjunction with other platform windowing libraries (e.g. `winit`, `SDL`, etc.)
* The current process may host other plugins that need to interact with the platform's GUI capabilities

## Changed
* `nice-plug-core`, `nice-plug`, bumped to version `0.4.2`
* `nice-plug-egui` bumped to version `0.5.1`
* `nice-plug-iced` bumped to version `0.4.1`

# nice-plug 0.4.1

## Changed
* `nice-plug-core`, `nice-plug`, bumped to version `0.4.1`
* Deprecated functions and constants that use "MIDI note" terminology in favor of "MIDI key" for consistency

## Added
* Added `MIDDLE_C` constant and `number_or_middle_c()` method to `Key` struct
* Added missing documentation

# nice-plug 0.4.0

## Breaking Changes
* `nice-plug-core`, `nice-plug`, and `nice-plug-iced` bumped to version `0.4`
* `nice-plug-egui` bumped to version `0.5`
* The `Editor` trait and the `ResizeHint` struct now use the new `NativeSize` type instead of `PhysicalSize`
* `track_info_updated()` method has moved from the `Plugin` trait to the `Editor` trait
* Minimum Rust version bumped to `1.88` (#55)
* `ProcessContext::send_event()` method has been renamed to `try_send_event()`, and is now fallible
* The `note` fields in `NoteEvent` have been renamed to `key` to be more consistent with MIDI terminology
* `NoteEvent` voice IDs, channel numbers, and key numbers all now support wildcard variants
* `NoteEvent::as_midi()` now takes a reference to `self`
* `SysExMessage::to_buffer()` was renamed to `as_buffer()`, and now takes a reference to `self`

## Added
* Added ASIO standalone backend (by Jakub Turowski in #79)
* Added `FloatRange::gain_range()` method (by Jeremy Stafford in #81)
* Added `GuiContext::request_restart()` to allow plugin GUIs to request a restart of the plugin
* Added `Plugin::INPUT_EVENT_CAPACITY` constant to allow users to adjust the allocated capacity for input events
* Added `Plugin::stop_processing()` method that is called when the host is about to deactivate the plugin or send
the plugin to sleep (currently only used in CLAP plugins)
* Added `track_info_changed()` method to `NiceGuiApp` trait
* Added `new_track_info()` method to `IcedNiceContext`

## Changed
* Many panics triggered by the host misbehaving or by malformed state have been replaced with debug log messages

## Optimized
* Added `ParamInfo` struct to parameters to reduce memory footprint in hot audio path (by Jakub Turowski in #80)
* Optimized event sorting logic (for the most common case) in the VST3 wrapper and made it realtime safe

## Fixed
* Made plugin locking logic more robust and guaranteed to be realtime-safe
* Added protections when loading malformed state (#85)
* Plugins no longer panic when the host sends a large number of events at once (#87)
* In-place processing is now only enabled when the number of main input and output channels match (#84)
* Non-finite parameter values from the host are now filtered out to prevent state corruption (#86)
* Fixed sample-accurate block splitting logic in clap wrapper (#91)
* Fixed a regression where parameters wouldn't be rescanned after a state change
* Fixed a rounding issue in `v2s_f32_hz_then_khz()`
* Added a flag to prevent filling up VST3 fd socket
* Fixed a minor atomic ordering issue with voice info in clap wrapper

Also thanks to maxmcorp and davfre for finding various issues!

# nice-plug 0.3.0

## Breaking Changes
* All crates updated to use baseview version `0.3.1`
* `nice-plug-core`, `nice-plug`, and `nice-plug-iced` bumped to version `0.3`
* `nice-plug-egui` bumped to version `0.4`
* `nice-plug-egui` updated to use `egui` version `0.36`
* A new `editor` cargo feature was added to `nice-plug` and `nice-plug-core`. Disabling this will disable all GUI-related
code entirely, saving some processing overhead and reducing binary size.
* `Plugin::initialize()` and `InitContext` renamed to `Plugin::activate()` and `ActivateContext`
* A new `Editor` type was added to the `Plugin` trait. Plugins without an editor can set this to `()`.
* `Plugin::editor()` now returns `Option<Self::Editor>` instead of `Option<Box<dyn Editor>>`
* The `Editor` trait was completely overhauled to support better handling of window resizing and scaling. See
[editor.rs](https://codeberg.org/RustAudio/nice-plug/src/commit/1f496ddd098b5ce050f4617322ec4c8718b94a62/crates/nice-plug-core/src/editor.rs)
for more details.
* Added `resize_hint` field to `EguiNiceSettings`
* The build and update closures in `create_egui_editor` were replaced with a new `NiceEguiApp` trait
* `EguiState` was renamed to `EguiEditorState`
* Renamed `EditorState` in `nice-plug-iced` to `PersistentState`
* Renamed `EditorSettings` in `nice-plug-iced` to `IcedNiceSettings` and added `resize_hint` field
* `nice-plug-egui` no longer automatically repaints every frame. Use the new `RepaintNotifier` to notify the GUI when it
should repaint.

## Added
* Better integration for window resizing and scaling in CLAP, VST3, and standalone targets

## Changed
* Window size is no longer saved/loaded with the plugin's preset state

# nice-plug 0.2.3

## Fixed
* Actually invoke logging setup in CLAP and VST3 targets ([#66](https://codeberg.org/RustAudio/nice-plug/pulls/66))

# nice-plug 0.2.2

## Fixed
* Fixed compiling on Windows

## Changed
* There is no longer a compiler error when using the `assert_process_allocs` feature on the x86_64-pc-windows-gnu target.

# nice-plug 0.2.0

## Breaking Changes
* all crates updated to use baseview version `0.2`
* `nice-plug-core` and `nice-plug` bumped to version 0.2
* `nice-log` bumped to version 0.3
* `nice-plug-egui` bumped to version 0.4
* `nice-plug-iced` bumped to version 0.2
* `Editor::size()` now returns the `Size` type from the `dpi` crate
* `Editor::set_size()` now uses `PhysicalSize<u32>` from the `dpi` crate
* `ParentWindowHandle` enum has been reworked to be more in-line with `raw-window-handle` 0.6
* `create_egui_editor` has been updated for the new version of `egui-baseview`
* `EguiState` in `nice-plug-egui` has been overhauled (window resizing can now be done with the standard `ui.send_viewport_command()` method in egui.)
* The `Queue` struct in `nice-plug-egui` was renamed to `ExtraOutputCommands`
* Some methods removed in `ExtraOutputCommands` in favor of `ui.send_viewport_command()`.
* reworked methods in `NiceGuiContext` from `nice-plug-iced`
* added `window_title` field to `EditorSettings` in `nice-plug-iced`
* `WrapperConfig::dpi_scale` in the standalone target now uses `f64` instead of `f32`
* `nice_log` was overhauled to support the `tracing` crate

## Added
* `Editor::set_scale_factor()` (called when the host requests to set the scaling factor)
* `Plugin::setup_logger()` (can be used to set up logging filters, called when the program first initializes)
* Added a new `tracing-subscriber` feature to `nice-plug` that when enabled, automatically sets up a default logger using `tracing- subscriber`.

## Changed
* `Editor::spawn()` now returns `Box<dyn Any>` instead of `Box<dyn Any + Send>`
* nice-plug now uses `tracing` and `tracing-subscriber` instead of `log`

# nice-plug 0.1.9

* Fixed "unsafe" compiler warning when using the `nice_export_vst3!` macro.

# nice-plug 0.1.8

* use `objc2` dependency in place of the old unmaintained `objc` crate
* re-exported `AtomicF32` and `AtomicF64` in the `util` module

# nice-plug 0.1.7

* bumped `core-foundation` dependency to latest version

# nice-plug 0.1.6

* updated the standalone backend to use `cpal` 0.18

# nice-plug 0.1.5

* improved non-linux/macos unix support
* updated windows dependencies

# nice-plug 0.1.3

nice-plug has been published to crates.io! 🎉

(versions 0.1.0, 0.1.1, and 0.1.2 are identical to 0.1.3, the only changes were fixing
issues with the readme)
