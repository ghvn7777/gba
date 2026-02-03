//! Init workflow implementation.
//!
//! Handles repository initialization for GBA by creating the `.gba/` and `.trees/`
//! directories, writing a default configuration, updating `.gitignore`, generating
//! a directory tree listing, and delegating to the init agent for codebase analysis.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use tracing::{debug, info, instrument};

use crate::engine::Engine;
use crate::error::CoreError;

/// Directories to skip when generating the repository tree listing.
const SKIPPED_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    ".trees",
    "vendor",
    "dist",
    "build",
];

/// Maximum directory depth for tree generation.
const MAX_TREE_DEPTH: usize = 4;

/// Run the init workflow.
///
/// Performs the following steps:
/// 1. Verifies the repository is not already initialized
/// 2. Creates `.gba/` directory with a default `config.yaml`
/// 3. Creates `.trees/` directory
/// 4. Adds `.trees/` to `.gitignore` if not already present
/// 5. Generates a directory tree listing of the repository
/// 6. Calls the init agent to analyze the repo and generate context documents
///
/// # Errors
///
/// Returns `CoreError::AlreadyInitialized` if `.gba/` already exists.
/// Returns `CoreError::Io` if directory or file creation fails.
/// Returns `CoreError::Agent` if the init agent fails.
#[instrument(skip(engine))]
pub(crate) async fn run_init(engine: &Engine) -> Result<(), CoreError> {
    let repo_path = engine.config().repo_path().clone();
    let gba_dir = engine.config().gba_dir();
    let trees_dir = engine.config().trees_dir();

    // Step 1: Check not already initialized
    if gba_dir.exists() {
        return Err(CoreError::AlreadyInitialized);
    }
    info!(repo = %repo_path.display(), "initializing repository for GBA");

    // Step 2: Create .gba/ with default config.yaml
    fs::create_dir_all(&gba_dir)?;
    write_default_config(&gba_dir)?;
    debug!(path = %gba_dir.display(), "created .gba directory with default config");

    // Step 3: Create .trees/
    fs::create_dir_all(&trees_dir)?;
    debug!(path = %trees_dir.display(), "created .trees directory");

    // Step 4: Update .gitignore
    update_gitignore(&repo_path)?;
    debug!("updated .gitignore");

    // Step 5: Generate repo tree listing
    let repo_tree = generate_repo_tree(&repo_path)?;
    debug!(
        lines = repo_tree.lines().count(),
        "generated repository tree"
    );

    // Step 6: Call init agent
    let context = serde_json::json!({
        "repo_path": repo_path.display().to_string(),
        "repo_tree": repo_tree,
    });
    engine
        .agent_runner()
        .run_agent("init", "init/task", &context, Some(&repo_path))
        .await?;
    info!("init agent completed");

    Ok(())
}

/// Generate a text tree listing of the repository directory structure.
///
/// Walks the directory tree up to [`MAX_TREE_DEPTH`] levels deep, skipping
/// directories listed in [`SKIPPED_DIRS`]. Produces output similar to the
/// `tree` command.
///
/// # Errors
///
/// Returns `CoreError::Io` if the directory cannot be read.
pub(crate) fn generate_repo_tree(repo_path: &Path) -> Result<String, CoreError> {
    let mut output = String::new();
    let dir_name = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".");
    writeln!(output, "{dir_name}/").map_err(|e| CoreError::Agent(e.to_string()))?;
    walk_tree(repo_path, "", 0, &mut output)?;
    Ok(output)
}

/// Write a default `.gba/config.yaml` file.
///
/// Creates a minimal configuration file with commented-out options to guide
/// the user on available settings.
///
/// # Errors
///
/// Returns `CoreError::Io` if the file cannot be written.
pub(crate) fn write_default_config(gba_dir: &Path) -> Result<(), CoreError> {
    let default_config = r#"# GBA Configuration
# See documentation for all available options.

agent:
  # model: claude-sonnet-4-20250514
  # maxTokens: 16384
  permissionMode: auto

git:
  autoCommit: true
  branchPattern: "feat/{id}-{slug}"
  baseBranch: main

review:
  enabled: true
  maxIterations: 3

verification:
  enabled: true
  maxIterations: 3

hooks:
  preCommit: []
  maxRetries: 5
"#;

    let config_path = gba_dir.join("config.yaml");
    fs::write(&config_path, default_config)?;
    Ok(())
}

/// Add `.trees/` to `.gitignore` if not already present.
///
/// Creates the `.gitignore` file if it does not exist. Appends `.trees/`
/// on a new line if the entry is not already in the file.
///
/// # Errors
///
/// Returns `CoreError::Io` if the file cannot be read or written.
pub(crate) fn update_gitignore(repo_path: &Path) -> Result<(), CoreError> {
    let gitignore_path = repo_path.join(".gitignore");
    let entry = ".trees/";

    let content = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };

    // Check if .trees/ is already in .gitignore (exact line match)
    let already_present = content.lines().any(|line| line.trim() == entry);

    if !already_present {
        let mut new_content = content;
        // Ensure we start on a new line if file is non-empty and doesn't end with newline
        if !new_content.is_empty() && !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(entry);
        new_content.push('\n');
        fs::write(&gitignore_path, new_content)?;
    }

    Ok(())
}

/// Recursively walk the directory tree and append entries to the output string.
fn walk_tree(dir: &Path, prefix: &str, depth: usize, output: &mut String) -> Result<(), CoreError> {
    if depth >= MAX_TREE_DEPTH {
        return Ok(());
    }

    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            // Skip hidden files/dirs (except specific ones) and skipped dirs
            if e.path().is_dir() && SKIPPED_DIRS.contains(&name_str.as_ref()) {
                return false;
            }
            true
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let total = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if entry.path().is_dir() {
            writeln!(output, "{prefix}{connector}{name_str}/")
                .map_err(|e| CoreError::Agent(e.to_string()))?;
            let new_prefix = format!("{prefix}{child_prefix}");
            walk_tree(&entry.path(), &new_prefix, depth + 1, output)?;
        } else {
            writeln!(output, "{prefix}{connector}{name_str}")
                .map_err(|e| CoreError::Agent(e.to_string()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_should_generate_repo_tree() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        // Create a directory structure
        fs::create_dir_all(root.join("src")).expect("should create src");
        fs::write(root.join("src/main.rs"), "fn main() {}").expect("should write file");
        fs::write(root.join("Cargo.toml"), "[package]").expect("should write file");
        fs::create_dir_all(root.join("tests")).expect("should create tests");
        fs::write(root.join("tests/integration.rs"), "").expect("should write file");

        // Create a directory that should be skipped
        fs::create_dir_all(root.join("target/debug")).expect("should create target");
        fs::create_dir_all(root.join("node_modules/pkg")).expect("should create node_modules");

        let tree = generate_repo_tree(root).expect("should generate tree");

        // Should contain source files
        assert!(tree.contains("src/"), "tree should contain src/");
        assert!(tree.contains("main.rs"), "tree should contain main.rs");
        assert!(
            tree.contains("Cargo.toml"),
            "tree should contain Cargo.toml"
        );
        assert!(tree.contains("tests/"), "tree should contain tests/");

        // Should skip excluded directories
        assert!(!tree.contains("target/"), "tree should not contain target/");
        assert!(
            !tree.contains("node_modules/"),
            "tree should not contain node_modules/"
        );
    }

    #[test]
    fn test_should_write_default_config() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let gba_dir = dir.path().join(".gba");
        fs::create_dir_all(&gba_dir).expect("should create .gba dir");

        write_default_config(&gba_dir).expect("should write config");

        let config_path = gba_dir.join("config.yaml");
        assert!(config_path.exists(), "config.yaml should exist");

        let content = fs::read_to_string(&config_path).expect("should read config");
        assert!(
            content.contains("permissionMode: auto"),
            "config should contain permissionMode"
        );
        assert!(
            content.contains("autoCommit: true"),
            "config should contain autoCommit"
        );
        assert!(
            content.contains("baseBranch: main"),
            "config should contain baseBranch"
        );

        // Verify the config is valid YAML that can be parsed
        let parsed: Result<crate::config::ProjectConfig, _> = serde_yaml::from_str(&content);
        assert!(
            parsed.is_ok(),
            "default config should be valid YAML: {:?}",
            parsed.err()
        );
    }

    #[test]
    fn test_should_update_gitignore_adds_trees() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        // Create an existing .gitignore without .trees/
        fs::write(root.join(".gitignore"), "target/\n").expect("should write gitignore");

        update_gitignore(root).expect("should update gitignore");

        let content = fs::read_to_string(root.join(".gitignore")).expect("should read gitignore");
        assert!(
            content.contains(".trees/"),
            "gitignore should contain .trees/"
        );
        assert!(
            content.contains("target/"),
            "gitignore should still contain target/"
        );
    }

    #[test]
    fn test_should_update_gitignore_no_duplicate() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        // Create a .gitignore that already has .trees/
        fs::write(root.join(".gitignore"), "target/\n.trees/\n").expect("should write gitignore");

        update_gitignore(root).expect("should update gitignore");

        let content = fs::read_to_string(root.join(".gitignore")).expect("should read gitignore");
        let count = content
            .lines()
            .filter(|line| line.trim() == ".trees/")
            .count();
        assert_eq!(count, 1, "should not duplicate .trees/ entry");
    }

    #[test]
    fn test_should_create_gitignore_when_missing() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        // No .gitignore exists
        update_gitignore(root).expect("should update gitignore");

        let gitignore_path = root.join(".gitignore");
        assert!(gitignore_path.exists(), ".gitignore should be created");

        let content = fs::read_to_string(&gitignore_path).expect("should read gitignore");
        assert!(
            content.contains(".trees/"),
            "gitignore should contain .trees/"
        );
    }

    #[test]
    fn test_should_respect_max_depth() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        // Create a deep directory structure (deeper than MAX_TREE_DEPTH)
        let deep_path = root.join("a").join("b").join("c").join("d").join("e");
        fs::create_dir_all(&deep_path).expect("should create deep dirs");
        fs::write(deep_path.join("deep.txt"), "content").expect("should write file");

        let tree = generate_repo_tree(root).expect("should generate tree");

        // Level 4 directory "d" should appear, but "e" (level 5) should not
        assert!(tree.contains("d/"), "tree should contain d/ at depth 4");
        assert!(
            !tree.contains("e/"),
            "tree should not contain e/ beyond max depth"
        );
        assert!(
            !tree.contains("deep.txt"),
            "tree should not contain files beyond max depth"
        );
    }

    #[test]
    fn test_should_generate_tree_for_empty_dir() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let tree = generate_repo_tree(dir.path()).expect("should generate tree");

        // Should have at least the root directory name
        assert!(!tree.is_empty(), "tree should not be empty");
    }

    #[test]
    fn test_should_handle_gitignore_without_trailing_newline() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        // Create a .gitignore without trailing newline
        fs::write(root.join(".gitignore"), "target/").expect("should write gitignore");

        update_gitignore(root).expect("should update gitignore");

        let content = fs::read_to_string(root.join(".gitignore")).expect("should read gitignore");
        assert!(
            content.contains(".trees/"),
            "gitignore should contain .trees/"
        );
        // Should have added a newline before .trees/
        assert!(
            content.contains("target/\n.trees/"),
            "should have proper newline separation"
        );
    }

    #[test]
    fn test_should_generate_tree_with_correct_structure() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        fs::create_dir_all(root.join("src")).expect("should create src");
        fs::write(root.join("src/lib.rs"), "").expect("should write file");
        fs::write(root.join("Cargo.toml"), "").expect("should write file");

        let tree = generate_repo_tree(root).expect("should generate tree");

        // Verify tree connector characters are present
        let has_connectors = tree.contains("├── ") || tree.contains("└── ");
        assert!(
            has_connectors,
            "tree should contain tree connector characters"
        );
    }

    #[test]
    fn test_should_skip_all_excluded_directories() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let root = dir.path();

        // Create all excluded directories
        for &skipped in SKIPPED_DIRS {
            fs::create_dir_all(root.join(skipped).join("subdir"))
                .expect("should create skipped dir");
            fs::write(root.join(skipped).join("file.txt"), "content").expect("should write file");
        }

        // Create a non-excluded directory for contrast
        fs::create_dir_all(root.join("src")).expect("should create src");
        fs::write(root.join("src/main.rs"), "").expect("should write file");

        let tree = generate_repo_tree(root).expect("should generate tree");

        assert!(tree.contains("src/"), "tree should contain src/");
        for &skipped in SKIPPED_DIRS {
            let dir_entry = format!("{skipped}/");
            assert!(
                !tree.contains(&dir_entry),
                "tree should not contain {dir_entry}"
            );
        }
    }

    #[tokio::test]
    async fn test_should_return_already_initialized() {
        let dir = tempfile::TempDir::new().expect("should create temp dir");
        let gba_dir = dir.path().join(".gba");
        fs::create_dir_all(&gba_dir).expect("should create .gba dir");

        let config = crate::config::EngineConfig::builder()
            .repo_path(dir.path().to_path_buf())
            .build();

        let engine = crate::engine::Engine::new(config)
            .await
            .expect("should create engine");
        let result = run_init(&engine).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::AlreadyInitialized));
    }
}
