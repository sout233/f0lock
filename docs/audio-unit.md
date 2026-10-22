# Audio Unit (macOS / Logic Pro)

f0lock's AUv2 is `f0lock.component`, a self-contained wrapper around the same
CLAP engine and egui interface. No separate CLAP installation is required.
It supports Intel and Apple Silicon on macOS 14 or newer.

Install the macOS PKG, or copy the component into
`~/Library/Audio/Plug-Ins/Components/`. Restart Logic and insert
**Audio FX > Audio Units > sout > f0lock**. Logic's Plug-in Manager can rescan it.
The macOS ZIP and PKG include AU, CLAP and VST3.

## Build on macOS

Install Rust, CMake and Xcode command line tools, then run:

```sh
cargo xtask bundle-universal f0lock --release --locked
python3 tools/ci/build_au.py
python3 tools/ci/package.py macos
```

The AU build downloads pinned, checksum-verified versions of
[clap-wrapper](https://github.com/free-audio/clap-wrapper) (0.17.0),
CLAP (1.2.10) and Apple's AudioUnitSDK (1.1.0). Their licenses are included.
Release CI checks Universal binaries and code signatures, then installs only
the AU and runs `auval -strict -v aufx F0lk Sout`.

The stable AU identity is `aufx / F0lk / Sout`; changing it breaks project recall.
Builds use ad-hoc signing, without Developer ID signing or notarization.
