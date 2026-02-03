# Feature 0002: Log to File

## Overview

Add dual-output tracing so that all `tracing` log output goes to both stderr (human-readable) and a JSON log file at `.gba/logs/<slug>/<timestamp>.log`. File logging is enabled for `plan` and `run` commands. Log files older than 3 days are auto-cleaned on every command startup.

## Architecture

### Tracing Layer Composition

The current single-layer tracing setup in `main.rs` is replaced with a multi-layer subscriber:

```
┌─────────────────────────────────────────┐
│          tracing-subscriber             │
│  ┌───────────────┐  ┌────────────────┐  │
│  │  stderr layer  │  │  file layer    │  │
│  │  (fmt, human)  │  │  (fmt, JSON)   │  │
│  │  EnvFilter     │  │  EnvFilter     │  │
│  └───────┬───────┘  └───────┬────────┘  │
│          │                  │            │
│          ▼                  ▼            │
│       stderr         .gba/logs/...log   │
└─────────────────────────────────────────┘
```

- **stderr layer**: `fmt::Layer` with human-readable format, filtered by `RUST_LOG` env var (existing behavior).
- **file layer**: `fmt::Layer` with `json()` format, writing to a log file via `tracing-appender`. Only added when a slug is available (`plan`, `run`).

### Log File Path

```
.gba/logs/<slug>/<YYYYMMDD_HHMMSS>.log
```

- `<slug>` is the feature slug (e.g., `0002-log-to-file`).
- `<YYYYMMDD_HHMMSS>` is the UTC timestamp at process start.
- Example: `.gba/logs/0002-log-to-file/20260203_143012.log`

### Cleanup Strategy

On every command startup (including `init`), scan `.gba/logs/` recursively. Delete any `.log` files whose filesystem `modified` timestamp is older than 3 days. Remove empty subdirectories after cleanup.

This runs synchronously before the main workflow since it is a fast filesystem scan.

## Interface Design

### New Module: `gba-cli/src/logging.rs`

```rust
use std::path::Path;

use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;

/// Initialize the tracing subscriber with stderr output.
///
/// When `slug` is `Some`, an additional JSON file layer is added
/// that writes to `.gba/logs/<slug>/<timestamp>.log` under `repo_path`.
///
/// Returns an optional `WorkerGuard` that must be held for the
/// lifetime of the program to ensure all buffered logs are flushed.
///
/// # Errors
///
/// Returns an error if the log directory cannot be created or the
/// log file cannot be opened.
pub fn init_tracing(
    repo_path: &Path,
    slug: Option<&str>,
) -> Result<Option<WorkerGuard>>;

/// Remove log files older than 3 days from `.gba/logs/`.
///
/// Walks the logs directory, removes `.log` files with a modified
/// timestamp older than 3 days, and removes any empty subdirectories.
///
/// This is a best-effort operation: errors on individual files are
/// logged as warnings but do not cause the function to fail.
pub fn cleanup_old_logs(repo_path: &Path);
```

### Changes to `main.rs`

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Extract slug and repo_path before consuming cli.
    let (repo_path, slug) = cli.log_context();

    // Clean old logs (best-effort, before tracing is initialized).
    logging::cleanup_old_logs(&repo_path);

    // Initialize tracing with optional file layer.
    let _guard = logging::init_tracing(&repo_path, slug.as_deref())?;

    cli.run().await
}
```

### Changes to `Cli`

Add a helper method to extract logging context without consuming the struct:

```rust
impl Cli {
    /// Extract the repo path and optional slug for logging setup.
    ///
    /// Returns `(repo_path, Some(slug))` for `plan` and `run` commands,
    /// and `(repo_path, None)` for `init`.
    pub fn log_context(&self) -> (PathBuf, Option<String>);
}
```

## Dependencies

### New

| Crate | Version | Purpose |
|-------|---------|---------|
| `tracing-appender` | `0.2` | Non-blocking file writer for tracing |

`tracing-appender` is the official companion crate to `tracing-subscriber`, maintained by the `tokio-rs/tracing` project. It provides `non_blocking()` for async-safe file writing and returns a `WorkerGuard` that flushes on drop.

### Existing (no changes)

- `tracing` -- already used throughout
- `tracing-subscriber` -- already used, `fmt` and `env-filter` features already enabled

## Behavioral Details

### Command-Level Behavior

| Command | stderr layer | file layer | cleanup |
|---------|-------------|-----------|---------|
| `init`  | yes         | no        | yes     |
| `plan`  | yes         | yes       | yes     |
| `run`   | yes         | yes       | yes     |

### Guard Lifetime

The `WorkerGuard` returned by `init_tracing` must be held (bound to `_guard` in `main`) for the entire program lifetime. Dropping the guard flushes any buffered log entries to the file. This is a requirement of `tracing-appender::non_blocking`.

### Error Handling

- **Log directory creation failure**: Propagated as `anyhow::Error` from `init_tracing`. The CLI exits with an error.
- **Cleanup errors on individual files**: Logged as `eprintln!` warnings (since tracing may not be initialized yet). Do not abort the program.
- **File write errors during operation**: Handled internally by `tracing-appender`'s non-blocking writer (drops events if the buffer is full, which is acceptable for logging).

### Backward Compatibility

- `RUST_LOG` environment variable continues to work as before for both layers.
- No changes to `.gba/config.yaml` schema.
- No changes to `gba-core` crate.
- The `.gba/logs/` directory is created on-demand; existing repos without it work fine.
