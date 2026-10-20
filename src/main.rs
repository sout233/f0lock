fn main() {
    let success = nice_plug::nice_export_standalone::<f0lock::F0Lock>();
    std::process::exit(if success { 0 } else { 1 });
}
