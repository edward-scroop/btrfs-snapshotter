// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: Copyright 2026 Edward Scroop <edward.scroop@gmail.com>

use jiff::{RoundMode, ToSpan, Unit, Zoned, ZonedRound};
use std::{
    cmp::Ordering,
    io,
    path::{Path, PathBuf},
    process::Command,
    thread::sleep,
};
use tracing::info_span;

mod init;

struct Config {
    minutes: i8,
    subvolume_path: PathBuf,
    subvolume_name: String,
    snapshot_path: PathBuf,
    hourly_limit: usize,
    daily_limit: usize,
    weekly_limit: usize,
    monthly_limit: usize,
}

struct Snapshot {
    snapshot_path: PathBuf,
    time: Zoned,
    keep_hourly: bool,
    keep_daily: bool,
    keep_weekly: bool,
    keep_monthly: bool,
}

impl Ord for Snapshot {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time.cmp(&other.time)
    }
}

impl PartialOrd for Snapshot {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl Eq for Snapshot {}

impl Snapshot {
    fn keep(&self) -> bool {
        self.keep_hourly || self.keep_daily || self.keep_weekly || self.keep_monthly
    }
}

fn main() {
    // Guard must live for the life of the program to ensure logs are written to log file.
    let _guard = init::init_logging();

    let config = Config {
        minutes: 0,
        subvolume_path: PathBuf::from("/"),
        subvolume_name: String::from("@rootfs"),
        snapshot_path: PathBuf::from("/snapshots"),
        hourly_limit: 12,
        daily_limit: 7,
        weekly_limit: 2,
        monthly_limit: 2,
    };
    let start_time = Zoned::now()
        .round(
            ZonedRound::new()
                .smallest(Unit::Second)
                .mode(RoundMode::Trunc),
        )
        .expect("Should never fail as it matches jiff invariants.");
    let first_snapshot_time = Zoned::now()
        .round(
            ZonedRound::new()
                .smallest(Unit::Second)
                .mode(RoundMode::Trunc),
        )
        .expect("Should never fail as it matches jiff invariants.")
        .with()
        .minute(config.minutes)
        .second(0)
        .build()
        .expect("Timestamp should be valid.");
    let start_to_first_snapshot = &start_time
        .until(&first_snapshot_time)
        .expect("Should never fail as it matches jiff invariants.");
    let mut snapshot_time = if start_to_first_snapshot.is_negative() {
        first_snapshot_time
            .checked_add(1.hour())
            .expect("Time should never overflow.")
    } else {
        first_snapshot_time
    };

    loop {
        sleep_until(&snapshot_time);

        let mut snapshot_path = config.snapshot_path.clone();
        snapshot_path.push(
            config.subvolume_name.clone() + "-" + &snapshot_time.to_string().replace("/", "__"),
        );
        if let Err(e) = create_btrfs_snapshot(
            config.subvolume_path.as_path(),
            snapshot_path.as_path(),
            true,
        ) {
            eprintln!("{}", e);
        }

        let snapshots = btrfs_snapshots(config.snapshot_path.as_path());

        match snapshots {
            Ok(x) => {
                let mut matching_snapshots: Vec<Snapshot> = Vec::with_capacity(x.len());
                let subvolume_name = config.subvolume_name.clone() + "-";

                for snapshot in x.iter() {
                    let snapshot_dirname = snapshot
                        .file_name()
                        .expect("Snapshot path should be valid.")
                        .to_str()
                        .expect("Snapshot path should be valid utf8.");

                    if snapshot_dirname.starts_with(&subvolume_name) {
                        matching_snapshots.push(Snapshot {
                            snapshot_path: snapshot.to_path_buf(),
                            time: snapshot_dirname.replace("__", "/")[subvolume_name.len()..]
                                .parse()
                                .expect("Time string should be parsed by jiff."),
                            keep_hourly: false,
                            keep_daily: false,
                            keep_weekly: false,
                            keep_monthly: false,
                        })
                    }
                }
                matching_snapshots.sort();

                if matching_snapshots.len() >= config.hourly_limit {
                    for i in 0..config.hourly_limit {
                        if let Some(snapshot) = matching_snapshots.get_mut(i) {
                            snapshot.keep_hourly = true;
                        } else {
                            break;
                        }
                    }

                    let mut count = config.daily_limit;
                    let mut time = snapshot_time
                        .yesterday()
                        .expect("Time should never be near Zoned min.");

                    for snapshot in matching_snapshots.iter_mut() {
                        if snapshot.time <= time {
                            snapshot.keep_daily = true;

                            if count == 1 {
                                break;
                            } else {
                                count -= 1;
                                time = time
                                    .yesterday()
                                    .expect("Time should never be near Zoned min.");
                            }
                        }
                    }
                    for snapshot in matching_snapshots.iter_mut().rev() {
                        if count == 0 {
                            break;
                        }

                        if !snapshot.keep_daily {
                            snapshot.keep_daily = true;
                            count -= 1;
                        }
                    }

                    count = config.weekly_limit;
                    time = snapshot_time
                        .yesterday()
                        .expect("Time should never be near Zoned min.");

                    for snapshot in matching_snapshots.iter_mut() {
                        if snapshot.time <= time {
                            snapshot.keep_weekly = true;

                            if count == 1 {
                                break;
                            } else {
                                count -= 1;
                                time = time
                                    .yesterday()
                                    .expect("Time should never be near Zoned min.");
                            }
                        }
                    }
                    for snapshot in matching_snapshots.iter_mut().rev() {
                        if count == 0 {
                            break;
                        }

                        if !snapshot.keep_weekly {
                            snapshot.keep_weekly = true;
                            count -= 1;
                        }
                    }

                    count = config.monthly_limit;
                    time = snapshot_time
                        .yesterday()
                        .expect("Time should never be Zoned min.");

                    for snapshot in matching_snapshots.iter_mut() {
                        if snapshot.time <= time {
                            snapshot.keep_monthly = true;

                            if count == 1 {
                                break;
                            } else {
                                count -= 1;
                                time = time
                                    .yesterday()
                                    .expect("Time should never be near Zoned min.");
                            }
                        }
                    }
                    for snapshot in matching_snapshots.iter_mut().rev() {
                        if count == 0 {
                            break;
                        }

                        if !snapshot.keep_monthly {
                            snapshot.keep_monthly = true;
                            count -= 1;
                        }
                    }

                    for snapshot in matching_snapshots.iter() {
                        if !snapshot.keep() {
                            if let Err(e) = delete_btrfs_snapshot(snapshot.snapshot_path.as_path())
                            {
                                eprintln!("{}", e);
                            }
                        }
                    }
                }
            }
            Err(e) => eprintln!("{}", e),
        }

        snapshot_time = snapshot_time
            .checked_add(1.hour())
            .expect("Time should never be near Zoned limit.")
    }
}

fn btrfs_snapshots(snapshot_dir: &Path) -> io::Result<Vec<PathBuf>> {
    tracing::info!(
        "Getting btrfs snapshots from snapshot dir: {}.",
        snapshot_dir.to_string_lossy()
    );
    let mut btrfs_snapshots = Vec::new();

    for entry in snapshot_dir.read_dir()? {
        let path = entry?.path();

        if path.is_dir() {
            btrfs_snapshots.push(PathBuf::from(
                path.to_str()
                    .expect("Path should be valid utf8.")
                    .replace("___", "/"),
            ));
        }
    }

    Ok(btrfs_snapshots)
}

fn sleep_until(next_time: &Zoned) {
    let now = Zoned::now()
        .round(
            ZonedRound::new()
                .smallest(Unit::Second)
                .mode(RoundMode::Trunc),
        )
        .expect("Should never fail as it matches jiff invariants.");
    let sleep_duration = now
        .until(next_time)
        .expect("Should never fail as it matches jiff invariants")
        .to_duration(&now)
        .expect("Should never overflow span.")
        .unsigned_abs();

    tracing::info!(
        "Sleeping for {} seconds until {}.",
        sleep_duration.as_secs_f64(),
        next_time
    );
    sleep(sleep_duration);
}

fn create_btrfs_snapshot(
    btrfs_subvolume_path: &Path,
    snapshot_destination: &Path,
    readonly: bool,
) -> Result<(), String> {
    let mut command = Command::new("btrfs");
    let mut args: Vec<&str> = Vec::new();
    let span = info_span!("create_btrfs_snapshot");
    let _span_guard = span.entered();

    tracing::info!("Creating btrfs snapshot.");

    args.push("subvolume");
    args.push("snapshot");

    if readonly {
        args.push("-r");
    }

    args.push(
        btrfs_subvolume_path
            .to_str()
            .expect("Path should be valid utf8."),
    );
    args.push(
        snapshot_destination
            .to_str()
            .expect("Path should be valid utf8."),
    );

    tracing::debug!("With args. {:?}", args);
    command.args(args);

    let output = match command.output() {
        Ok(x) => x,
        Err(e) => return Err(e.to_string()),
    };

    if output.status.success() {
        Ok(())
    } else {
        let stderr = str::from_utf8(&output.stderr)
            .expect("Stderr should be utf8.")
            .to_string();

        tracing::error!("Error running btrfs command. Output: {}", stderr);

        Err(stderr)
    }
}

fn delete_btrfs_snapshot(snapshot_path: &Path) -> Result<(), String> {
    let mut command = Command::new("btrfs");
    let mut args: Vec<&str> = Vec::new();
    let span = info_span!("delete_btrfs_snapshot");
    let _span_guard = span.entered();

    tracing::info!("Deleting btrfs snapshot.");

    args.push("subvolume");
    args.push("delete");
    args.push("-C");
    args.push(snapshot_path.to_str().expect("Path should be valid utf8."));

    command.args(args);

    let output = match command.output() {
        Ok(x) => x,
        Err(e) => return Err(e.to_string()),
    };

    if output.status.success() {
        Ok(())
    } else {
        let stderr = str::from_utf8(&output.stderr)
            .expect("Stderr should be utf8.")
            .to_string();

        tracing::error!("Error running btrfs command. Output: {}", stderr);

        Err(stderr)
    }
}
