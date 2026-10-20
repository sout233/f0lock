# nice-plug-core

[![Documentation](https://docs.rs/nice-plug-core/badge.svg)](https://docs.rs/nice-plug-core)
[![Crates.io](https://img.shields.io/crates/v/nice-plug-core.svg)](https://crates.io/crates/nice-plug-core)
[![License](https://img.shields.io/crates/l/nice-plug-core.svg)](https://codeberg.org/BillyDM/nice-plug/src/branch/main/LICENSE)

Core types and traits for plugins made with the [nice-plug](https://codeberg.org/BillyDM/nice-plug) plugin framework.

3rd party GUI libraries can use this to implement an adapter without needing to depend on the nice-plug GitHub repository.

## Cargo features

* `assert_process_allocs` (default) - Enabling this feature will cause the plugin to terminate when allocations occur in the processing function during debug builds. Keep in mind that panics may also allocate if they use string formatting, so temporarily disabling this feature may be necessary when debugging panics in DSP code.
* `simd` - Add adapters to the Buffer object for reading the channel data to and from `std::simd` vectors. Requires a nightly compiler.
