//! Development-only console diagnostics. This module is absent from release builds.

use std::io::{self, Write};

struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let _ = writeln!(
                io::stderr().lock(),
                "[{}] {}",
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

pub fn initialize() {
    static LOGGER: ConsoleLogger = ConsoleLogger;
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(log::LevelFilter::Info);
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}
