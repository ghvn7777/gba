# Verification: 0002-log-to-file

## Acceptance Criteria

1. **`gba plan <slug>` produces a JSON log file** at `.gba/logs/<slug>/<YYYYMMDD_HHMMSS>.log`.
2. **`gba run <slug>` produces a JSON log file** at the same path pattern.
3. **`gba init` does NOT produce a log file** -- only stderr output.
4. **stderr output remains human-readable** for all commands (unchanged from current behavior).
5. **Log file content is valid JSON** -- each line is a valid JSON object (JSON Lines format).
6. **Log files older than 3 days are cleaned up** on every command startup.
7. **Empty subdirectories are removed** after cleanup.
8. **`RUST_LOG` env var controls both layers** -- setting `RUST_LOG=debug` increases verbosity in both stderr and file.
9. **The program does not panic** if `.gba/logs/` does not exist (it is created on demand).
10. **The program does not abort** if cleanup encounters permission errors on individual files.

## Test Scenarios

### Unit Tests

#### `test_should_generate_log_file_path`
- Call the log path generation function with slug `"my-feature"` and a fixed timestamp.
- Assert the path matches `.gba/logs/my-feature/<timestamp>.log`.

#### `test_should_cleanup_old_log_files`
- Create a temp directory with `.gba/logs/test-slug/` containing:
  - A `.log` file with modified time = 4 days ago
  - A `.log` file with modified time = 1 day ago
- Call `cleanup_old_logs`.
- Assert the old file is deleted and the recent file remains.

#### `test_should_remove_empty_dirs_after_cleanup`
- Create a temp directory with `.gba/logs/empty-slug/` containing only a `.log` file 4 days old.
- Call `cleanup_old_logs`.
- Assert the file is deleted and the `empty-slug/` directory is removed.

#### `test_should_not_fail_on_missing_logs_dir`
- Call `cleanup_old_logs` on a repo path with no `.gba/logs/` directory.
- Assert no panic or error.

#### `test_should_return_none_guard_when_no_slug`
- Call `init_tracing` with `slug = None`.
- Assert the returned guard is `None`.

### Integration Tests

#### `test_should_write_json_log_during_plan`
- Set up a temp repo with `.gba/` directory.
- Call `init_tracing` with a slug.
- Emit a `tracing::info!("test message")`.
- Drop the guard to flush.
- Read the log file and verify each line is valid JSON containing `"test message"`.

#### `test_should_preserve_stderr_format`
- Verify that `init_tracing` with a slug does not change stderr output format (still human-readable, not JSON).

## Verification Commands

```bash
# Build the project
cargo build

# Run all tests
cargo test

# Run clippy with strict warnings
cargo clippy -- -D warnings

# Run formatter check
cargo +nightly fmt -- --check

# Manual smoke test: run plan and check log file exists
gba plan test-feature --repo /tmp/test-repo
ls .gba/logs/test-feature/
cat .gba/logs/test-feature/*.log | head -1 | python3 -c "import sys,json; json.load(sys.stdin); print('valid JSON')"
```
