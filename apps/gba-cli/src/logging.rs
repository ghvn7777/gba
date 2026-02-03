//! Logging initialization and log file management.
//!
//! Provides dual-output tracing: stderr (human-readable) and an optional
//! JSON log file at `.gba/logs/<slug>/<timestamp>.log`. File logging is
//! enabled for commands that operate on a specific feature (plan, run).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Maximum age of log files before cleanup, in days.
const LOG_RETENTION_DAYS: u64 = 3;

/// Initialize the tracing subscriber with stderr output.
///
/// When `slug` is `Some`, an additional JSON file layer is added
/// that writes to `.gba/logs/<slug>/<timestamp>.log` under `repo_path`.
///
/// Returns an optional [`WorkerGuard`] that must be held for the
/// lifetime of the program to ensure all buffered logs are flushed.
///
/// # Errors
///
/// Returns an error if the log directory cannot be created or the
/// log file cannot be opened.
pub fn init_tracing(repo_path: &Path, slug: Option<&str>) -> Result<Option<WorkerGuard>> {
    let guard = build_tracing(repo_path, slug)?;

    if let Some((subscriber, guard)) = guard {
        subscriber.init();
        Ok(Some(guard))
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        Ok(None)
    }
}

/// Build the tracing subscriber layers without registering globally.
///
/// Returns `Some((subscriber, guard))` when a slug is provided (dual-layer),
/// or `None` when only stderr logging is needed.
fn build_tracing(
    repo_path: &Path,
    slug: Option<&str>,
) -> Result<Option<(impl tracing::Subscriber + Send + Sync, WorkerGuard)>> {
    let Some(slug) = slug else {
        return Ok(None);
    };

    let (non_blocking, guard) = open_log_writer(repo_path, slug)?;

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(EnvFilter::from_default_env()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking)
                .with_filter(EnvFilter::from_default_env()),
        );

    Ok(Some((subscriber, guard)))
}

/// Create the log directory and file, returning a non-blocking writer and guard.
///
/// Builds the log path as `.gba/logs/<slug>/<YYYYMMDD_HHMMSS>.log`, creates
/// the parent directories, opens the file, and wraps it in a non-blocking
/// writer via `tracing_appender`.
fn open_log_writer(
    repo_path: &Path,
    slug: &str,
) -> Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    let log_path = build_log_path(repo_path, slug);

    // build_log_path always produces a path with a parent directory
    // (`.gba/logs/<slug>/`), so this branch is unreachable in practice.
    let log_dir = log_path.parent().context(format!(
        "failed to resolve parent directory for log path: {}",
        log_path.display(),
    ))?;

    fs::create_dir_all(log_dir)
        .with_context(|| format!("failed to create log directory: {}", log_dir.display()))?;

    let log_file = fs::File::create(&log_path)
        .with_context(|| format!("failed to create log file: {}", log_path.display()))?;

    Ok(tracing_appender::non_blocking(log_file))
}

/// Remove log files older than 3 days from `.gba/logs/`.
///
/// Walks the logs directory, removes `.log` files with a modified
/// timestamp older than 3 days, and removes any empty subdirectories.
///
/// This is a best-effort operation: errors on individual files are
/// logged as warnings via `eprintln!` (since tracing may not be
/// initialized yet) but do not cause the function to fail.
pub fn cleanup_old_logs(repo_path: &Path) {
    let logs_dir = repo_path.join(".gba").join("logs");
    if !logs_dir.is_dir() {
        return;
    }

    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60);

    remove_old_log_files(&logs_dir, cutoff);
    remove_empty_dirs(&logs_dir);
}

/// Build the log file path: `.gba/logs/<slug>/<YYYYMMDD_HHMMSS>.log`.
fn build_log_path(repo_path: &Path, slug: &str) -> PathBuf {
    let now = std::time::SystemTime::now();
    let timestamp = format_utc_timestamp(now);
    repo_path
        .join(".gba")
        .join("logs")
        .join(slug)
        .join(format!("{timestamp}.log"))
}

/// Format a [`SystemTime`] as `YYYYMMDD_HHMMSS` in UTC.
fn format_utc_timestamp(time: std::time::SystemTime) -> String {
    let duration = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Manual UTC date/time calculation to avoid external date crate dependency.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_date(days);

    format!("{year:04}{month:02}{day:02}_{hours:02}{minutes:02}{seconds:02}")
}

/// Convert days since Unix epoch to (year, month, day) in the Gregorian calendar.
fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm based on civil_from_days from Howard Hinnant's date library.
    // Shifts epoch to 0000-03-01 for easier leap year handling.
    let z = days_since_epoch as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // year of era [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month marker [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    (y as u64, m, d)
}

/// Recursively remove `.log` files older than `cutoff` from `dir`.
fn remove_old_log_files(dir: &Path, cutoff: std::time::SystemTime) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!(
                "warning: failed to read log directory {}: {e}",
                dir.display()
            );
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "warning: failed to read directory entry in {}: {e}",
                    dir.display()
                );
                continue;
            }
        };

        let path = entry.path();

        if path.is_dir() {
            remove_old_log_files(&path, cutoff);
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }

        let modified = match fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "warning: failed to read metadata for {}: {e}",
                    path.display()
                );
                continue;
            }
        };

        if modified < cutoff
            && let Err(e) = fs::remove_file(&path)
        {
            eprintln!(
                "warning: failed to remove old log file {}: {e}",
                path.display(),
            );
        }
    }
}

/// Remove empty subdirectories under `dir` (does not remove `dir` itself).
fn remove_empty_dirs(dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_dir() {
            remove_empty_dirs(&path);
            // Try to remove; this will fail if not empty, which is fine.
            let _ = fs::remove_dir(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn test_should_format_utc_timestamp_at_epoch() {
        let epoch = std::time::UNIX_EPOCH;
        let result = format_utc_timestamp(epoch);
        assert_eq!(result, "19700101_000000");
    }

    #[test]
    fn test_should_format_utc_timestamp_known_date() {
        // 2026-02-03 14:30:12 UTC = 1770129012 seconds since epoch
        let time = std::time::UNIX_EPOCH + Duration::from_secs(1_770_129_012);
        let result = format_utc_timestamp(time);
        assert_eq!(result, "20260203_143012");
    }

    #[test]
    fn test_should_build_log_path_with_slug() {
        let repo = PathBuf::from("/tmp/test-repo");
        let path = build_log_path(&repo, "0002-log-to-file");
        let path_str = path.to_string_lossy();

        assert!(path_str.starts_with("/tmp/test-repo/.gba/logs/0002-log-to-file/"));
        assert!(path_str.ends_with(".log"));

        // The filename should be a valid timestamp format: YYYYMMDD_HHMMSS.log
        let filename = path.file_stem().unwrap().to_string_lossy();
        assert_eq!(filename.len(), 15); // YYYYMMDD_HHMMSS
        assert_eq!(&filename[8..9], "_");
    }

    #[test]
    fn test_should_convert_days_to_known_dates() {
        // Unix epoch: 1970-01-01
        assert_eq!(days_to_date(0), (1970, 1, 1));
        // 2000-01-01 is day 10957
        assert_eq!(days_to_date(10957), (2000, 1, 1));
        // 2024-02-29 (leap day) is day 19782
        assert_eq!(days_to_date(19782), (2024, 2, 29));
    }

    #[test]
    fn test_should_cleanup_old_log_files() {
        let tmp = tempfile::tempdir().unwrap();
        let logs_dir = tmp.path().join(".gba").join("logs").join("test-slug");
        fs::create_dir_all(&logs_dir).unwrap();

        // Create a "recent" file
        let recent = logs_dir.join("recent.log");
        fs::write(&recent, "recent log").unwrap();

        // Create an "old" file and backdate its modified time
        let old = logs_dir.join("old.log");
        fs::write(&old, "old log").unwrap();

        let four_days_ago = SystemTime::now() - Duration::from_secs(4 * 24 * 60 * 60);
        filetime::set_file_mtime(&old, filetime::FileTime::from_system_time(four_days_ago))
            .unwrap();

        cleanup_old_logs(tmp.path());

        assert!(recent.exists(), "recent log file should be preserved");
        assert!(!old.exists(), "old log file should be removed");
    }

    #[test]
    fn test_should_remove_empty_dirs_after_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let slug_dir = tmp.path().join(".gba").join("logs").join("empty-slug");
        fs::create_dir_all(&slug_dir).unwrap();

        // Create and backdate a single file so the dir becomes empty after cleanup.
        let old = slug_dir.join("old.log");
        fs::write(&old, "old").unwrap();

        let four_days_ago = SystemTime::now() - Duration::from_secs(4 * 24 * 60 * 60);
        filetime::set_file_mtime(&old, filetime::FileTime::from_system_time(four_days_ago))
            .unwrap();

        cleanup_old_logs(tmp.path());

        assert!(!slug_dir.exists(), "empty slug directory should be removed");
    }

    #[test]
    fn test_should_skip_non_log_files() {
        let tmp = tempfile::tempdir().unwrap();
        let logs_dir = tmp.path().join(".gba").join("logs").join("test-slug");
        fs::create_dir_all(&logs_dir).unwrap();

        // Create a non-.log file and backdate it
        let non_log = logs_dir.join("notes.txt");
        fs::write(&non_log, "notes").unwrap();

        let four_days_ago = SystemTime::now() - Duration::from_secs(4 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &non_log,
            filetime::FileTime::from_system_time(four_days_ago),
        )
        .unwrap();

        cleanup_old_logs(tmp.path());

        assert!(non_log.exists(), "non-.log files should not be removed");
    }

    #[test]
    fn test_should_handle_nonexistent_logs_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // Should not panic or error when logs directory doesn't exist.
        cleanup_old_logs(tmp.path());
    }

    #[test]
    fn test_should_create_log_dir_and_file_for_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let (_non_blocking, _guard) = open_log_writer(tmp.path(), "my-feature").unwrap();

        let logs_dir = tmp.path().join(".gba").join("logs").join("my-feature");
        assert!(logs_dir.is_dir(), "log directory should be created");

        let entries: Vec<_> = fs::read_dir(&logs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1, "exactly one log file should be created");

        let log_file = entries[0].path();
        assert_eq!(
            log_file.extension().and_then(|e| e.to_str()),
            Some("log"),
            "log file should have .log extension",
        );

        // Filename should match YYYYMMDD_HHMMSS format.
        let stem = log_file.file_stem().unwrap().to_string_lossy();
        assert_eq!(stem.len(), 15);
        assert_eq!(&stem[8..9], "_");
    }

    #[test]
    fn test_should_return_error_for_invalid_repo_path() {
        // A path that cannot be created (e.g., under /dev/null).
        let result = open_log_writer(Path::new("/dev/null"), "test-slug");
        assert!(
            result.is_err(),
            "should fail when directory cannot be created"
        );
    }

    #[test]
    fn test_should_return_none_guard_when_no_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let result = build_tracing(tmp.path(), None).unwrap();
        assert!(
            result.is_none(),
            "should return None when no slug is provided",
        );
    }
}
