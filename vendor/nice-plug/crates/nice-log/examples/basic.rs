use tracing::level_filters::LevelFilter;

fn main() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_writer(nice_log::writer_from_env())
            .with_max_level(LevelFilter::DEBUG)
            .finish(),
    )
    .unwrap();

    // When changing some of the level filter above some of these messages may no longer be printed
    tracing::error!("This is an error");
    tracing::warn!("This is a warning");
    tracing::info!("This is a regular log message");
    tracing::debug!("This is a debug message, usually only made visible during debug builds");
    tracing::trace!("This is a trace message, usually hidden unless explicitly opted into");

    // Debug and trace messages contain the module path
    some_module::log_from_module();
}

mod some_module {
    pub fn log_from_module() {
        tracing::debug!("This is a debug message printed from another module");
    }
}
