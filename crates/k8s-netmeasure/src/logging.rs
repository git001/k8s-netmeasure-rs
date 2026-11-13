//! Logging (design §9.3): flat JSON lines on stderr. Result data (`probe` lines, the
//! `selftest` summary) goes to stdout, so the two never mix.

use netmeasure_cli::LogLevel;
use tracing::Level;
use tracing_subscriber::fmt;

/// Installs the global JSON subscriber with `level` as the minimum (`--log-level`).
///
/// The measuring thread never logs inside a cycle, so `debug` does not disturb the
/// timed section; it only adds lines between cycles and from other threads.
pub fn init(level: LogLevel) {
    let subscriber = fmt()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(false)
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_max_level(to_tracing(level))
        .finish();
    // A second init (tests calling twice) is harmless.
    let _ = tracing::subscriber::set_global_default(subscriber);
}

fn to_tracing(level: LogLevel) -> Level {
    match level {
        LogLevel::Error => Level::ERROR,
        LogLevel::Warn => Level::WARN,
        LogLevel::Info => Level::INFO,
        LogLevel::Debug => Level::DEBUG,
        LogLevel::Trace => Level::TRACE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_maps() {
        let all = [
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Trace,
        ];
        let mapped: Vec<Level> = all.into_iter().map(to_tracing).collect();
        assert_eq!(
            mapped,
            [
                Level::ERROR,
                Level::WARN,
                Level::INFO,
                Level::DEBUG,
                Level::TRACE
            ]
        );
    }
}
