<div align="center">
<img src="https://codeberg.org/RustAudio/nice-plug/raw/branch/main/branding/logo.svg" width="84px" height="84px"/>
<h1>nice-plug</h1>

[![Tests](https://ci.codeberg.org/api/badges/17499/status.svg)](https://ci.codeberg.org/repos/17499)
[![Documentation](https://docs.rs/nice-plug/badge.svg)](https://docs.rs/nice_plug)
[![Crates.io](https://img.shields.io/crates/v/nice_plug.svg)](https://crates.io/crates/nice_plug)
[![License](https://img.shields.io/crates/l/nice_plug.svg)](https://codeberg.org/RustAudio/nice-plug/src/branch/main/LICENSE)

A [Rust](https://rust-lang.org/) audio plugin development framework that is nice to
use :)

</div>

---

The idea is to have a stateful yet simple plugin API that gets rid of as much
unnecessary ceremony wherever possible, while also keeping the amount of magic to
a minimum and making it easy to experiment with different approaches to things.

> This project is currently experimental, and has recently undergone some large changes, so expect some bugs. If you find any, please create an issue!

---

> nice-plug started out as a fork of the awesome [NIH-plug](https://github.com/robbert-vdh/nih-plug)
> framework authored by Robbert van der Helm. It has since become its own separate
> community-led project, and is now the recommended toolkit for Rust audio plugin
> developers.

### Table of contents

- [Getting Started](#getting-started)
- [Features](#features)
- [GUI](#gui)
- [Example plugins](#example-plugins)
- [Get Involved](#get-involved)
  - [Contributing](#contributing)
  - [AI Policy](#ai-policy)
- [Licensing](#licensing)

---

# Getting Started

See [Getting Started with nice-plug](https://codeberg.org/RustAudio/nice-plug/src/branch/main/GETTING_STARTED.md)
for a quick guide on getting started with using nice-plug to develop your own plugins.

A [cookiecutter template](https://codeberg.org/RustAudio/nice-plug-template) is also available.

# Features

> For a list of available crate flags, see
> [crates/nice-plug/Cargo.toml](https://codeberg.org/RustAudio/nice-plug/src/branch/main/crates/nice-plug/Cargo.toml).

- Supports both VST3 and [CLAP](https://github.com/free-audio/clap) by simply
  adding the corresponding `nice_export_<api>!(Foo)` macro to your plugin's
  library.
- Standalone binaries can be made by calling `nice_export_standalone(Foo)` from
  your `main()` function. Standalones come with a CLI for configuration and full
  JACK audio, MIDI, and transport support.
- Rich declarative parameter system without any boilerplate.
  - Define parameters for your plugin by adding `FloatParam`, `IntParam`,
    `BoolParam`, and `EnumParam<T>` fields to your parameter struct, assign
    stable IDs to them with the `#[id = "foobar"]`, and a `#[derive(Params)]`
    does all of the boring work for you.
  - Parameters can have complex value distributions and the parameter objects
    come with built-in smoothers and callbacks.
  - Use simple enums deriving the `Enum` trait with the `EnumParam<T>` parameter
    type for parameters that allow the user to choose between multiple discrete
    options. That way you can use regular Rust pattern matching when working
    with these values without having to do any conversions yourself.
  - Store additional non-parameter state for your plugin by adding any field
    that can be serialized with [Serde](https://serde.rs/) to your plugin's
    `Params` object and annotating them with `#[persist = "key"]`.
  - Optional support for state migrations, for handling breaking changes in
    plugin parameters.
  - Group your parameters into logical groups by nesting `Params` objects using
    the `#[nested(group = "...")]`attribute.
  - The `#[nested]` attribute also enables you to use multiple copies of the
    same parameter, either as regular object fields or through arrays.
  - When needed, you can also provide your own implementation for the `Params`
    trait to enable compile time generated parameters and other bespoke
    functionality.
- Stateful. Behaves mostly like JUCE, just without all of the boilerplate.
- Comes with a simple yet powerful way to asynchronously run background tasks
  from a plugin that's both type-safe and realtime-safe.
- Does not make any assumptions on how you want to process audio, but does come
  with utilities and adapters to help with common access patterns.
  - Efficiently iterate over an audio buffer either per-sample per-channel,
    per-block per-channel, or even per-block per-sample-per-channel with the
    option to manually index the buffer or get access to a channel slice at any
    time.
  - Easily leverage per-channel SIMD using the SIMD adapters on the buffer and
    block iterators.
  - Comes with bring-your-own-FFT adapters for common (inverse) short-time
    Fourier Transform operations. More to come.
- Optional sample accurate automation support for VST3 and CLAP that can be
  enabled by setting the `Plugin::SAMPLE_ACCURATE_AUTOMATION` constant to
  `true`.
- Optional support for compressing the human readable JSON state files using
  [Zstandard](https://en.wikipedia.org/wiki/Zstd).
- Modular GUI/Editor API that lets you use any GUI library that can run on
  [baseview](https://github.com/RustAudio/baseview). By extension, this allows
  for rendering with OpenGL, [wgpu](wgpu.rs), and/or
  [softbuffer](https://github.com/rust-windowing/softbuffer).
- Full support for receiving and outputting both modern polyphonic note
  expression events as well as MIDI CCs, channel pressure, and pitch bend for
  CLAP and VST3.
  - MIDI SysEx is also supported. Plugins can define their own structs or sum
    types to wrap around those messages so they don't need to interact with raw
    byte buffers in the process function.
- Support for flexible dynamic buffer configurations, including variable numbers
  of input and output ports.
- First-class support several more exotic CLAP features:
  - Both monophonic and polyphonic parameter modulation are supported.
  - Plugins can declaratively define pages of remote controls that DAWs can bind
    to hardware controllers.
- A plugin bundler accessible with the [cargo-nice-plug](https://codeberg.org/RustAudio/nice-plug/src/branch/main/crates/cargo-nice-plug)
  package or via a [custom xtask command](https://codeberg.org/RustAudio/nice-plug/src/branch/main/crates/nice-plug-xtask).
- Tested on Linux and Windows, with limited testing on macOS.
- See the [`Plugin`](https://codeberg.org/RustAudio/nice-plug/src/branch/main/crates/nice-plug-core/src/plugin.rs)
  trait's documentation for a more complete overview of the core API.

# GUI

Any GUI library that can run on [baseview](https://github.com/RustAudio/baseview)
can be used for nice-plug.

At this early stage, there is no single GUI framework that is being focused on.
For now, integrations for the following GUI libraries are provided:

- [nice-plug-egui](https://codeberg.org/RustAudio/nice-plug/src/branch/main/crates/nice-plug-egui) - Adapter for [egui](https://github.com/emilk/egui).
- [nice-plug-iced](https://codeberg.org/RustAudio/nice-plug/src/branch/main/crates/nice-plug-iced) - Adapter for [Iced](https://iced.rs/).
  - [iced_audio](https://github.com/iced-rs/iced_audio) - Extra audio-related widgets for Iced.
- nice-plug-slint (WIP) - Adapter for [Slint](https://slint.dev/).
- [vizia-plug](https://github.com/vizia/vizia-plug) (3rd party) - Adapter for [Vizia](https://github.com/vizia/vizia).

Some of these options may or may not have first-party support in the future. The
plan is to see which options the community gravitates towards, and then officially
support one or two of the most popular ones.

Also, keep in mind that none of these options currently have good documentation
for how to create plugin GUIs with them. For now, take a look at the examples
to get started. (Feel free to contribute any guides, documentation, and or
example plugins!)

> It is recommended to not even worry about GUI when developing the DSP of your plugin, especially if you are just starting out learning DSP. nice-plug makes it easy to create GUI-less plugins!

# Example plugins

The best way to get an idea for what the API looks like is to look at the
examples.

- [**gain**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/gain/src/lib.rs)
  is a simple smoothed gain plugin that shows off a couple other parts of the API,
  like support for storing arbitrary serializable state.
- **gain_\<gui\>** are the same plugins as gain, but with a GUI to control the
  parameter and a digital peak meter.
    - [**gain_egui**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/gain_egui)
    - [**gain_iced**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/gain_iced)
- [**iced_audio_widget**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/iced_audio_widget) is an example of how to use the
  [iced_audio](https://github.com/iced-rs/iced_audio) widget library for [Iced](https://iced.rs/) in nice-plug.
- Examples for adding your own custom GUI framework on top of raw rendering APIs:
  - [**byo_gui_gl**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/byo_gui_gl) - for rendering with OpenGL
  - [**byo_gui_wgpu**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/byo_gui_wgpu) - for rendering with [wgpu](wgpu.rs)
  - [**byo_gui_softbuffer**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/byo_gui_softbuffer) - for rendering with
  [softbuffer](https://github.com/rust-windowing/softbuffer) (software rendering)
- [**midi_inverter**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/midi_inverter/src/lib.rs) takes note/MIDI events and
  flips around the note, channel, expression, pressure, and CC values. This
  example demonstrates how to receive and output those events.
- [**poly_mod_synth**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/poly_mod_synth/src/lib.rs) is a simple polyphonic
  synthesizer with support for polyphonic modulation in supported CLAP hosts.
  This demonstrates how polyphonic modulation can be used in nice-plug.
- [**sine**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/sine/src/lib.rs) is a simple test tone generator plugin with
  frequency smoothing that can also make use of MIDI input instead of generating
  a static signal based on the plugin's parameters.
- [**stft**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/stft/src/lib.rs) shows off some of nice-plug's other optional
  higher level helper features, such as an adapter to process audio with a
  short-term Fourier transform using the overlap-add method, all using the
  compositional `Buffer` interfaces.
- [**sysex**](https://codeberg.org/RustAudio/nice-plug/src/branch/main/examples/sysex/src/lib.rs) is a simple example of how to send and
  receive SysEx messages by defining custom message types.

The example plugins can be built using:

```shell
cargo xtask bundle <package_name> --release
```

# Get Involved

If you have any questions or you wish to get involved in the project, feel free
to join us in the [Rust Audio Discord Server](https://discord.gg/Qs2Zwtf9Gf)
in the `#nice-plug` channel!

### Contributing

Contributions are very much welcomed! As long as they comply to the policy and
licensing requirements below.

> nice-plug uses optional nightly features. To make rust analyzer happy, you can
> enable the nightly compiler for your local repository with `rustup override add
nightly`.

### AI policy

The general AI policy of the RustAudio Community applies to this repository. Please
ensure compliance to these rules before submitting your contribution to this project.

Please refer to the Rust Audio community's policy an AI usage:
https://rust.audio/community/ai/

# Licensing

The nice-plug framework and all of the example plugins are licensed under the
[ISC license](https://www.isc.org/licenses/).

The logos in `branding/` are licensed under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).
