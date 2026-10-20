# nice-plug: cargo subcommand for bundling plugins

This is nice-plug's `cargo xtask` command, but as a `cargo` subcommand. This can
be used as an alternative to
<https://codeberg.org/BillyDM/nice-plug/src/branch/main/crates/nice-plug-xtask>
if you don't want to create an `xtask` module in your workspace.

## Installation

This can be installed by running:

```shell
cargo install cargo-nice-plug
```

Alternatively, you can install directly from the git repository:

```shell
cargo install --git https://codeberg.org/RustAudio/nice-plug.git cargo-nice-plug
```

## Usage

Once that's installed, you can compile and bundle plugins using:

```shell
cargo nice-plug bundle <package_name> --release
```
