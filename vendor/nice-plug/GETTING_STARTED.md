# Getting Started with nice-plug

A quick guide on getting started with using nice-plug to develop your own plugins.

## 1. Create a new project

Create a new library (not binary) crate for your plugin with:
```shell
cargo new --lib my_plugin
```

Add the following to your `Cargo.toml` so that the plugin can be compiled as a shared library:
```toml
[lib]
crate-type = ["cdylib"]
```

Add the nice-plug dependency to your `Cargo.toml`:
```toml
[dependencies]
nice-plug = "0.4"
```

> For a list of available crate flags, see
> [crates/nice-plug/Cargo.toml](https://codeberg.org/RustAudio/nice-plug/src/branch/main/crates/nice-plug/Cargo.toml).

## 2. (Optional) Standalone build target

If you wish to also export your plugin as a standalone application, add "lib" to "crate-type" and enable the `standalone` feature flag:

```toml
[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
nice-plug = { version = "0.4", features = ["standalone"] }
```

And add a `main.rs` file next to the `lib.rs` file with the following contents:
```rust
use nice_plug::prelude::*;

fn main() {
    nice_export_standalone::<my_plugin::MyPlugin>();
}
```

## 3. (Optional) Compiler settings

For better perfomance, it is recommended to add the following profile settings to your `Cargo.toml`:

```toml
# Enable a small amount of optimization in the dev profile.
[profile.dev]
opt-level = 1

# Enable more optimization in the release profile at the cost of compile time.
[profile.release]
codegen-units = 1
lto = "thin"
# (optional) helps reduce binary size
strip = "symbols"

[profile.profiling]
inherits = "release"
debug = true
strip = "none"
```

Additionally, you can enable the `unsafe_flush_denormals` feature flag, which can lead to a significant performance increases in some cases. HOWEVER, the Rust compiler technically considers this to be undefined behavior, so use at your own risk! Though if any UB did occur, the only damage will likely just be audio glitches, not memory safety issues.

```toml
[dependencies]
nice-plug = { version = "0.4", features = ["unsafe_flush_denormals"] }
```

## 4. Build system setup

While `cargo` is great, it alone is not sufficient for building CLAP/VST3 plugins. For this there are two available options:

*(TODO: Explain how to create plugin bundles and how to create universal MacOS binaries)*

### Option A: Using cargo-nice-plug

Install the `cargo-nice-plug` program by running:

```shell
cargo install cargo-nice-plug
```

Alternatively, you can install directly from the git repository:
```shell
cargo install --git https://codeberg.org/RustAudio/nice-plug.git cargo-nice-plug
```

### Option B: Using xtask

See [nice-plug-xtask](crates/nice-plug-xtask/README.md) on how to set up your own xtask system.

## 5. Initial boilerplate

Here is the boilerplate for the simplest plugin with a single gain parameter. Add the following contents to `lib.rs`:

> For an explanation of this boilerplate, see the [gain example](examples/gain/src/lib.rs).

```rust
use nice_plug::prelude::*;
use std::sync::Arc;

pub struct MyPlugin {
    params: Arc<MyPluginParams>,
}

#[derive(Params)]
struct MyPluginParams {
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for MyPluginParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::gain_range(-30.0, 30.0),
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyPluginParams::default()),
        }
    }
}

impl Plugin for MyPlugin {
    const NAME: &'static str = "My Plugin";
    const VENDOR: &'static str = "Moist Plugins GmbH";
    const URL: &'static str = "https://youtu.be/dQw4w9WgXcQ";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[],
            aux_output_ports: &[],
            names: PortNames::const_default(),
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = ();
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for channel_samples in buffer.iter_samples() {
            let gain = self.params.gain.smoothed.next();

            for sample in channel_samples {
                *sample *= gain;
            }
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

impl ClapPlugin for MyPlugin {
    const CLAP_ID: &'static str = "com.moist-plugins-gmbh.my-plugin";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("My cool plugin");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for MyPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"MyMoistPlugin001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nice_export_clap!(MyPlugin);
nice_export_vst3!(MyPlugin);
```

## 6. Build your plugin

### Option A: Build clap/vst3 plugins using cargo-nice-plug

```shell
cargo nice-plug bundle my_plugin
```
or if you want to build your plugin in release mode:
```shell
cargo nice-plug bundle my_plugin --release
```

### Option B: Build clap/vst3 plugins using xtask

```shell
cargo xtask bundle <package_name>
```
or if you want to build your plugin in release mode:
```shell
cargo xtask bundle <package_name> --release
```

### Option C: Build standalone app

The standalone version of your plugin can be built with the regular `cargo run` command. This can be useful for faster iterations when developing your plugin's GUI.

## 7. Load and test your plugin

After building, the `my-plugin.clap`/`my-plugin.vst3` build artifacts are located in `target/bundled/`.

[Bitwig](https://www.bitwig.com/) is an excellent DAW for testing nice-plug plugins:
* It runs natively on Mac, Windows, and Linux
* It supports both CLAP and VST3 plugins
* It has an unlimited "demo" mode (the only restriction is that saving/exporting is disabled)
* It is able to unload/reload plugins without having to restart the DAW
* You can add your `target/bundled/` directory to the plugin search paths

Debug output from your plugin can be found in Bitwig's `engine.log` file. (`~/.BitwigStudio/log/engine.log` on Linux)

*(TODO: Explain additional methods for debugging)*

## 8. Next steps

Currently nice-plug's documentation isn't very extensive. For now, you can check out the examples in the [nice-plug repository](https://codeberg.org/RustAudio/nice-plug), and also check out the [API documentation](https://docs.rs/nice-plug).

It is recommended to not even worry about GUI when developing the DSP of your plugin, especially if you are just starting out learning DSP. nice-plug makes it easy to create GUI-less plugins!

For an extensive list of resources on audio plugin development in general, see [Awesome Audio DSP](https://codeberg.org/BillyDM/awesome-audio-dsp).

If you have any questions, feel free to join us in the [Rust Audio Discord Server](https://discord.gg/Qs2Zwtf9Gf) in the `#nice-plug` channel!
