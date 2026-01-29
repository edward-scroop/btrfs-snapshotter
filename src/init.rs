use crate::Config;
use jiff::Zoned;
use serde::Deserialize;
use std::{path::PathBuf, process::exit};
use tracing_appender::{non_blocking::WorkerGuard, rolling::Rotation};
use tracing_subscriber::{
    filter,
    fmt::{self, format, time::FormatTime},
    prelude::*,
};

struct JiffLocal;

impl FormatTime for JiffLocal {
    fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", Zoned::now())
    }
}

#[derive(Deserialize)]
struct TempConfig {
    minutes: Option<i8>,
    subvolume_path: Option<PathBuf>,
    subvolume_name: Option<String>,
    snapshot_path: Option<PathBuf>,
    hourly_limit: Option<usize>,
    daily_limit: Option<usize>,
    weekly_limit: Option<usize>,
    monthly_limit: Option<usize>,
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
            eprintln!("Error initialising logger. tracing message: {}", e);
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
        eprintln!("Error initialising logger. tracing message: {}", e);
        exit(1);
    };

    guard
}

pub fn load_config() -> Config {
    let config_file_path = "/etc/btrfs-snapshotter/config.toml";
    let config_file = match std::fs::read(config_file_path) {
        Ok(x) => x,
        Err(e) => {
            eprintln!(
                "Error loading config file: {} | Error: {}",
                config_file_path, e
            );
            std::process::exit(1);
        }
    };

    let temp_config: TempConfig = match toml::from_slice(config_file.as_slice()) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let mut config = Config::default();
    if let Some(x) = temp_config.minutes {
        config.minutes = x;
    }
    if let Some(x) = temp_config.subvolume_path {
        config.subvolume_path = x;
    }
    if let Some(x) = temp_config.subvolume_name {
        config.subvolume_name = x;
    }
    if let Some(x) = temp_config.snapshot_path {
        config.snapshot_path = x;
    }
    if let Some(x) = temp_config.hourly_limit {
        config.hourly_limit = x;
    }
    if let Some(x) = temp_config.daily_limit {
        config.daily_limit = x;
    }
    if let Some(x) = temp_config.weekly_limit {
        config.weekly_limit = x;
    }
    if let Some(x) = temp_config.monthly_limit {
        config.monthly_limit = x;
    }

    config
}
