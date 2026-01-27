use std::process::exit;

use jiff::Zoned;
use tracing_appender::{non_blocking::WorkerGuard, rolling::Rotation};
use tracing_subscriber::{
    filter,
    fmt::{self, format, time::FormatTime},
    prelude::*,
};

struct JiffLocal;

impl FormatTime for JiffLocal {
    fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", Zoned::now().to_string())
    }
}

pub fn init_logging() -> WorkerGuard {
    let rolling_appender = match tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(Rotation::NEVER)
        .filename_prefix("btrfs-snapshotter")
        .filename_suffix("log")
        .build("/var/log")
    {
        Ok(x) => x,
        Err(e) => {
            eprintln!(
                "Error initialising logger. tracing message: {}",
                e.to_string()
            );
            exit(1);
        }
    };

    let (file_writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .lossy(false)
        .finish(rolling_appender);
    let logfile_layer = fmt::Layer::default()
        .with_ansi(false)
        .with_writer(file_writer)
        .with_timer(JiffLocal)
        .with_filter(filter::LevelFilter::INFO);
    let stdout_layer = fmt::Layer::default()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .event_format(format().compact())
        .with_timer(JiffLocal)
        .with_filter(filter::LevelFilter::INFO);
    let subscriber = tracing_subscriber::Registry::default()
        .with(logfile_layer)
        .with(stdout_layer);

    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!(
            "Error initialising logger. tracing message: {}",
            e.to_string()
        );
        exit(1);
    };

    guard
}
