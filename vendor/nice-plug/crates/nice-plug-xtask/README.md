# nice-plug: bundler and other utilities

This is nice-plug's `cargo xtask` command, as a library. This can be used as an alternative to the [cargo-nice-plug](../cargo-nice-plug) program.

To use this, add an `xtask` binary to your project using `cargo new --bin xtask`. Then add that binary to the Cargo workspace in your repository's main
`Cargo.toml` file like so:

```toml
# Cargo.toml

[workspace]
members = ["xtask"]
```

Add `nice-plug-xtask` to the new xtask package's dependencies, and call its main
function from the new xtask binary:

```toml
# xtask/Cargo.toml

[dependencies]
nice-plug-xtask = "0.1.0"
```

```rust
// xtask/src/main.rs

fn main() -> nice_plug_xtask::Result<()> {
    nice_plug_xtask::main()
}
```

Lastly, create a `.cargo/config.toml` file in your repository and add a Cargo alias.
This allows you to run the binary using `cargo xtask`:

```toml
# .cargo/config

[alias]
xtask = "run --package xtask --release --"
```

Now you can build the plugin with:
```shell
cargo xtask bundle <package_name>
```
or if you want to build your plugin in release mode:
```shell
cargo xtask bundle <package_name> --release
```
