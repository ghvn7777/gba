//! Prompt manager implementation.
//!
//! `PromptManager` loads built-in Jinja2 templates at compile time and supports
//! loading custom overrides from a directory at runtime.

use std::fs;
use std::path::Path;

use minijinja::Environment;
use tracing::debug;

use crate::error::PmError;
use crate::template::AgentConfig;

/// Built-in templates embedded at compile time from the `agents/` directory.
/// Each entry is `(name, source)` where name follows `{agent}/{template}` convention.
const BUILT_IN_TEMPLATES: &[(&str, &str)] = &[
    // init agent
    (
        "init/system",
        include_str!("../../../agents/init/system.md.j2"),
    ),
    ("init/task", include_str!("../../../agents/init/task.md.j2")),
    // plan agent
    (
        "plan/system",
        include_str!("../../../agents/plan/system.md.j2"),
    ),
    ("plan/task", include_str!("../../../agents/plan/task.md.j2")),
    // code agent
    (
        "code/system",
        include_str!("../../../agents/code/system.md.j2"),
    ),
    ("code/task", include_str!("../../../agents/code/task.md.j2")),
    (
        "code/resume",
        include_str!("../../../agents/code/resume.md.j2"),
    ),
    (
        "code/hook_fix",
        include_str!("../../../agents/code/hook_fix.md.j2"),
    ),
    ("code/pr", include_str!("../../../agents/code/pr.md.j2")),
    // review agent
    (
        "review/system",
        include_str!("../../../agents/review/system.md.j2"),
    ),
    (
        "review/task",
        include_str!("../../../agents/review/task.md.j2"),
    ),
    (
        "review/fix",
        include_str!("../../../agents/review/fix.md.j2"),
    ),
    // verify agent
    (
        "verify/system",
        include_str!("../../../agents/verify/system.md.j2"),
    ),
    (
        "verify/task",
        include_str!("../../../agents/verify/task.md.j2"),
    ),
    (
        "verify/fix",
        include_str!("../../../agents/verify/fix.md.j2"),
    ),
];

/// Built-in agent configurations embedded at compile time.
const BUILT_IN_CONFIGS: &[(&str, &str)] = &[
    ("init", include_str!("../../../agents/init/config.yml")),
    ("plan", include_str!("../../../agents/plan/config.yml")),
    ("code", include_str!("../../../agents/code/config.yml")),
    ("review", include_str!("../../../agents/review/config.yml")),
    ("verify", include_str!("../../../agents/verify/config.yml")),
];

/// Manages prompt templates and renders them with context variables.
///
/// Supports built-in templates (compiled into the binary via `include_str!`)
/// and custom overrides loaded from disk. Custom templates with the same name
/// as built-in templates will replace them.
///
/// # Examples
///
/// ```
/// use gba_pm::PromptManager;
///
/// let pm = PromptManager::new().unwrap();
/// let names = pm.list_templates();
/// assert!(names.contains(&"init/system"));
/// ```
#[derive(Debug)]
pub struct PromptManager {
    env: Environment<'static>,
}

impl PromptManager {
    /// Create a manager pre-loaded with built-in templates.
    ///
    /// All templates from the `agents/` directory are embedded at compile time
    /// and registered with the internal minijinja environment.
    ///
    /// # Errors
    ///
    /// Returns `PmError::InvalidTemplate` if any built-in template has invalid
    /// Jinja2 syntax.
    pub fn new() -> Result<Self, PmError> {
        let mut env = Environment::new();

        for &(name, source) in BUILT_IN_TEMPLATES {
            env.add_template_owned(name.to_owned(), source.to_owned())
                .map_err(|e| PmError::InvalidTemplate(format!("{name}: {e}")))?;
            debug!(template = name, "loaded built-in template");
        }

        Ok(Self { env })
    }

    /// Load custom templates from a directory, overriding built-in templates
    /// with the same name.
    ///
    /// Walks the directory recursively looking for `.md.j2` files. Template
    /// names are derived from relative paths with the extension stripped.
    /// For example, `dir/init/system.md.j2` becomes `init/system`.
    ///
    /// # Errors
    ///
    /// Returns `PmError::Io` if the directory cannot be read. Returns
    /// `PmError::InvalidTemplate` if a template file contains invalid Jinja2 syntax.
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), PmError> {
        if !dir.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("directory not found: {}", dir.display()),
            )
            .into());
        }

        load_templates_recursive(dir, dir, &mut self.env)?;

        Ok(())
    }

    /// Render a named template with the given context.
    ///
    /// The context is a `serde_json::Value` that provides variables available
    /// to the Jinja2 template during rendering.
    ///
    /// # Errors
    ///
    /// Returns `PmError::TemplateNotFound` if no template with the given name exists.
    /// Returns `PmError::RenderError` if rendering fails (e.g., missing variables).
    pub fn render(&self, name: &str, ctx: &serde_json::Value) -> Result<String, PmError> {
        let tmpl = self
            .env
            .get_template(name)
            .map_err(|_| PmError::TemplateNotFound(name.to_owned()))?;

        tmpl.render(ctx)
            .map_err(|e| PmError::RenderError(format!("{name}: {e}")))
    }

    /// List all available template names.
    ///
    /// Returns a sorted list of template names including both built-in and
    /// custom override templates.
    pub fn list_templates(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.env.templates().map(|(name, _)| name).collect();
        names.sort_unstable();
        names
    }

    /// Load an agent configuration by agent name.
    ///
    /// Looks up the built-in `config.yml` for the given agent. Returns
    /// the parsed `AgentConfig`.
    ///
    /// # Errors
    ///
    /// Returns `PmError::TemplateNotFound` if no built-in config exists for the agent.
    /// Returns `PmError::ConfigParse` if the YAML content cannot be parsed.
    pub fn load_agent_config(name: &str) -> Result<AgentConfig, PmError> {
        let yaml_source = BUILT_IN_CONFIGS
            .iter()
            .find(|&&(n, _)| n == name)
            .map(|&(_, src)| src)
            .ok_or_else(|| PmError::TemplateNotFound(format!("agent config: {name}")))?;

        serde_yaml::from_str(yaml_source).map_err(|e| PmError::ConfigParse(format!("{name}: {e}")))
    }

    /// Load an agent configuration from a YAML file on disk.
    ///
    /// # Errors
    ///
    /// Returns `PmError::Io` if the file cannot be read.
    /// Returns `PmError::ConfigParse` if the YAML content cannot be parsed.
    pub fn load_agent_config_from_file(path: &Path) -> Result<AgentConfig, PmError> {
        let content = fs::read_to_string(path)?;
        serde_yaml::from_str(&content)
            .map_err(|e| PmError::ConfigParse(format!("{}: {e}", path.display())))
    }
}

/// Recursively walk a directory and load all `.md.j2` files as templates.
fn load_templates_recursive(
    base: &Path,
    current: &Path,
    env: &mut Environment<'static>,
) -> Result<(), PmError> {
    let entries = fs::read_dir(current)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            load_templates_recursive(base, &path, env)?;
        } else if let Some(ext) = path.extension() {
            // We look for files ending in `.j2` whose stem ends in `.md`
            // i.e., files matching `*.md.j2`.
            if ext == "j2" {
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();

                if file_name.ends_with(".md.j2") {
                    let name = template_name_from_path(base, &path)?;
                    let source = fs::read_to_string(&path)?;

                    env.add_template_owned(name.clone(), source)
                        .map_err(|e| PmError::InvalidTemplate(format!("{name}: {e}")))?;

                    debug!(template = %name, path = %path.display(), "loaded custom template");
                }
            }
        }
    }

    Ok(())
}

/// Derive a template name from a file path relative to the base directory.
///
/// Strips the base directory prefix and the `.md.j2` extension.
/// For example, `base/init/system.md.j2` becomes `init/system`.
fn template_name_from_path(base: &Path, path: &Path) -> Result<String, PmError> {
    let relative = path.strip_prefix(base).map_err(|e| {
        PmError::InvalidTemplate(format!(
            "path {} is not relative to {}: {e}",
            path.display(),
            base.display(),
        ))
    })?;

    // Convert to string and strip the `.md.j2` suffix
    let rel_str = relative.to_str().ok_or_else(|| {
        PmError::InvalidTemplate(format!("non-UTF-8 path: {}", relative.display()))
    })?;

    // Use forward slashes for template names regardless of OS
    let normalized = rel_str.replace('\\', "/");

    let name = normalized
        .strip_suffix(".md.j2")
        .ok_or_else(|| {
            PmError::InvalidTemplate(format!("expected .md.j2 extension: {normalized}"))
        })?
        .to_owned();

    Ok(name)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_should_create_manager_with_built_in_templates() {
        let pm = PromptManager::new();
        assert!(pm.is_ok(), "PromptManager::new() should succeed");

        let pm = pm.unwrap();
        let templates = pm.list_templates();
        assert!(!templates.is_empty(), "should have built-in templates");
    }

    #[test]
    fn test_should_list_all_built_in_templates() {
        let pm = PromptManager::new().unwrap();
        let templates = pm.list_templates();

        // Verify all expected built-in templates are present
        let expected = vec![
            "code/hook_fix",
            "code/pr",
            "code/resume",
            "code/system",
            "code/task",
            "init/system",
            "init/task",
            "plan/system",
            "plan/task",
            "review/fix",
            "review/system",
            "review/task",
            "verify/fix",
            "verify/system",
            "verify/task",
        ];

        assert_eq!(templates, expected);
    }

    #[test]
    fn test_should_render_template_with_variables() {
        let pm = PromptManager::new().unwrap();

        let ctx = json!({
            "repo_path": "/tmp/test-repo",
        });

        let result = pm.render("init/system", &ctx);
        assert!(result.is_ok(), "render should succeed: {:?}", result.err());

        let rendered = result.unwrap();
        assert!(
            rendered.contains("/tmp/test-repo"),
            "rendered output should contain the repo_path variable value"
        );
    }

    #[test]
    fn test_should_return_error_for_missing_template() {
        let pm = PromptManager::new().unwrap();

        let ctx = json!({});
        let result = pm.render("nonexistent/template", &ctx);
        assert!(result.is_err(), "rendering a missing template should fail");
        assert!(
            matches!(result.unwrap_err(), PmError::TemplateNotFound(_)),
            "error should be TemplateNotFound"
        );
    }

    #[test]
    fn test_should_load_custom_templates_from_directory() {
        let dir = TempDir::new().unwrap();

        // Create a custom template: custom_agent/custom_task.md.j2
        let agent_dir = dir.path().join("custom_agent");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(agent_dir.join("custom_task.md.j2"), "Hello {{ name }}!").unwrap();

        let mut pm = PromptManager::new().unwrap();
        let result = pm.load_dir(dir.path());
        assert!(
            result.is_ok(),
            "load_dir should succeed: {:?}",
            result.err()
        );

        // Verify the custom template was loaded
        let templates = pm.list_templates();
        assert!(
            templates.contains(&"custom_agent/custom_task"),
            "should contain the custom template"
        );

        // Render the custom template
        let rendered = pm.render("custom_agent/custom_task", &json!({"name": "World"}));
        assert!(rendered.is_ok());
        assert_eq!(rendered.unwrap(), "Hello World!");
    }

    #[test]
    fn test_should_override_built_in_templates_with_custom() {
        let dir = TempDir::new().unwrap();

        // Create a custom template that overrides init/system
        let init_dir = dir.path().join("init");
        fs::create_dir_all(&init_dir).unwrap();
        fs::write(
            init_dir.join("system.md.j2"),
            "Custom init system prompt for {{ repo_path }}",
        )
        .unwrap();

        let mut pm = PromptManager::new().unwrap();
        pm.load_dir(dir.path()).unwrap();

        let rendered = pm
            .render("init/system", &json!({"repo_path": "/my/repo"}))
            .unwrap();

        assert_eq!(
            rendered, "Custom init system prompt for /my/repo",
            "custom template should override built-in"
        );
    }

    #[test]
    fn test_should_return_error_for_nonexistent_directory() {
        let mut pm = PromptManager::new().unwrap();
        let result = pm.load_dir(Path::new("/nonexistent/directory"));
        assert!(result.is_err(), "load_dir on nonexistent dir should fail");
        assert!(
            matches!(result.unwrap_err(), PmError::Io(_)),
            "error should be Io"
        );
    }

    #[test]
    fn test_should_ignore_non_j2_files_in_load_dir() {
        let dir = TempDir::new().unwrap();

        let agent_dir = dir.path().join("agent");
        fs::create_dir_all(&agent_dir).unwrap();

        // Create a .md.j2 file (should be loaded)
        fs::write(agent_dir.join("template.md.j2"), "Valid template").unwrap();

        // Create non-.md.j2 files (should be ignored)
        fs::write(agent_dir.join("readme.md"), "Not a template").unwrap();
        fs::write(agent_dir.join("config.yml"), "preset: true").unwrap();
        fs::write(agent_dir.join("notes.txt"), "Just notes").unwrap();

        let mut pm = PromptManager::new().unwrap();
        pm.load_dir(dir.path()).unwrap();

        let templates = pm.list_templates();
        assert!(
            templates.contains(&"agent/template"),
            "should load .md.j2 files"
        );
        // The built-in count + 1 custom template
        let custom_count = templates.iter().filter(|t| t.starts_with("agent/")).count();
        assert_eq!(custom_count, 1, "should only load .md.j2 files");
    }

    #[test]
    fn test_should_load_nested_templates_from_directory() {
        let dir = TempDir::new().unwrap();

        // Create nested structure: agent/sub/deep.md.j2
        let nested_dir = dir.path().join("agent").join("sub");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(nested_dir.join("deep.md.j2"), "Deeply nested: {{ value }}").unwrap();

        let mut pm = PromptManager::new().unwrap();
        pm.load_dir(dir.path()).unwrap();

        let templates = pm.list_templates();
        assert!(
            templates.contains(&"agent/sub/deep"),
            "should load deeply nested templates"
        );

        let rendered = pm
            .render("agent/sub/deep", &json!({"value": "found"}))
            .unwrap();
        assert_eq!(rendered, "Deeply nested: found");
    }

    #[test]
    fn test_should_return_sorted_template_list() {
        let pm = PromptManager::new().unwrap();
        let templates = pm.list_templates();

        let mut sorted = templates.clone();
        sorted.sort_unstable();

        assert_eq!(
            templates, sorted,
            "list_templates should return sorted names"
        );
    }

    #[test]
    fn test_should_load_built_in_agent_config() {
        let config = PromptManager::load_agent_config("init");
        assert!(
            config.is_ok(),
            "should load init agent config: {:?}",
            config.err()
        );

        let config = config.unwrap();
        assert!(config.preset, "init agent should have preset: true");
        assert!(
            config.tools.is_empty(),
            "init agent should have no tools restriction"
        );
        assert!(
            config.disallowed_tools.is_empty(),
            "init agent should have no disallowed tools"
        );
    }

    #[test]
    fn test_should_load_review_agent_config_with_preset_false() {
        let config = PromptManager::load_agent_config("review").unwrap();
        assert!(!config.preset, "review agent should have preset: false");
    }

    #[test]
    fn test_should_return_error_for_unknown_agent_config() {
        let config = PromptManager::load_agent_config("unknown_agent");
        assert!(config.is_err(), "should fail for unknown agent");
        assert!(
            matches!(config.unwrap_err(), PmError::TemplateNotFound(_)),
            "error should be TemplateNotFound"
        );
    }

    #[test]
    fn test_should_load_agent_config_from_file() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.yml");
        fs::write(
            &config_path,
            "preset: false\ntools:\n  - Read\n  - Grep\ndisallowedTools:\n  - Write\n",
        )
        .unwrap();

        let config = PromptManager::load_agent_config_from_file(&config_path);
        assert!(
            config.is_ok(),
            "should load config from file: {:?}",
            config.err()
        );

        let config = config.unwrap();
        assert!(!config.preset);
        assert_eq!(config.tools, vec!["Read", "Grep"]);
        assert_eq!(config.disallowed_tools, vec!["Write"]);
    }

    #[test]
    fn test_should_render_template_with_empty_context() {
        let pm = PromptManager::new().unwrap();

        // The code/pr template uses variables, but we should get a render
        // (minijinja renders undefined variables as empty by default)
        let ctx = json!({});

        // The init/system template uses {{ repo_path }}, rendering with
        // empty context should either succeed (with empty values) or fail
        // depending on minijinja's undefined behavior. Let's test a template
        // that does not use conditional logic on required vars.
        let result = pm.render("init/system", &ctx);
        // minijinja with default settings treats undefined as empty string
        assert!(
            result.is_ok(),
            "render with empty context should succeed (undefined renders as empty)"
        );
    }

    #[test]
    fn test_should_derive_template_name_from_path() {
        let base = Path::new("/base");
        let path = Path::new("/base/agent/template.md.j2");
        let name = template_name_from_path(base, path).unwrap();
        assert_eq!(name, "agent/template");
    }

    #[test]
    fn test_should_reject_path_without_md_j2_extension() {
        let base = Path::new("/base");
        let path = Path::new("/base/agent/readme.md");
        let result = template_name_from_path(base, path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PmError::InvalidTemplate(_)));
    }
}
