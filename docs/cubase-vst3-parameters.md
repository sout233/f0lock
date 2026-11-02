# Cubase VST3 parameter readback

## Symptoms

In Cubase 15 on Windows, menus opened but selecting an item did not change
the parameter. Active/Bypass and knob double-click reset also failed, while
continuous knob dragging worked. The original report described the same
behavior in Cubase 12 with f0lock 0.2.0 and 0.2.1.

## Cause

Mouse events and parameter gestures reached the plugin correctly.
`performEdit()` succeeded, but Cubase called `getParamNormalized()` from
`endEdit()` before the audio callback consumed the new value. The shared
VST3 wrapper returned the old DSP value.

For example, enabling bypass submitted `1.0`, but Cubase read back `0.0`.
Dragging worked because audio callbacks ran between successive edits.

## Fix

The wrapper publishes pending controller values before `performEdit()`.
Controller queries return these immediately; DSP parameters still update
through the audio callback. A matching audio update atomically acknowledges
the pending value, while older, different updates leave it intact.
Host controller changes use the same mechanism. Restoring state clears
pending edits so they cannot override the restored values.

## Regression tests

Tests in `vendor/nice-plug/crates/nice-plug/src/wrapper/vst3/tests/parameters.rs`
cover single edits/reset, delayed audio acknowledgement, host automation,
state restore, and editing while processing is stopped. The single-edit test
failed before the fix and passed afterwards.

Verification in Cubase 15 on 2026-11-02 confirmed that the fixed VST3 resolved
the issue. Temporary diagnostic logging was removed from the verified build.
All nine wrapper unit tests passed. Editor tests had
44 passes, one ignored test, and three spectrum-point failures also reproduced
on the unchanged `1375f1f` baseline.

```powershell
cargo test --manifest-path vendor/nice-plug/crates/nice-plug/Cargo.toml --target-dir target --config 'patch.crates-io.egui-baseview.path="D:/f0lock/vendor/egui-baseview"' --config 'patch.crates-io.baseview.path="D:/f0lock/vendor/baseview"' --lib parameter_tests
```
