fn main() -> nice_plug_xtask::Result<()> {
    // This includes both the `cargo` command and the `nice-plug` subcommand, so we should get rid of
    // those first
    let args = std::env::args().skip(2);
    nice_plug_xtask::main_with_args("cargo nice-plug", args)
}
