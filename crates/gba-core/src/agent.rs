//! Agent runner module (internal).
//!
//! Wraps `claude-agent-sdk-rs` to provide a unified interface for running
//! agent sessions. Handles system prompt construction, permission mode mapping,
//! and tool configuration based on the agent's `config.yml`.

use std::path::Path;

use claude_agent_sdk_rs::{
    ClaudeAgentOptions, Message, PermissionMode as SdkPermissionMode, ResultMessage, SystemPrompt,
    SystemPromptPreset, Tools,
};
use tracing::{debug, error, instrument};

use crate::config::{EngineConfig, PermissionMode, ProjectConfig};
use crate::error::CoreError;

/// Wraps the Claude Agent SDK to run agent sessions.
///
/// `AgentRunner` holds the prompt manager and merged configuration needed
/// to construct agent options for each session. It provides both collecting
/// and streaming execution modes.
#[derive(Debug)]
pub(crate) struct AgentRunner {
    /// Prompt manager for rendering templates.
    prompt_manager: gba_pm::PromptManager,
    /// Resolved model name (CLI override > project config > SDK default).
    model: Option<String>,
    /// Resolved max tokens (CLI override > project config).
    #[allow(dead_code)] // Retained for future use when agent token limits are enforced
    max_tokens: Option<u32>,
    /// Permission mode from project config.
    permission_mode: PermissionMode,
}

impl AgentRunner {
    /// Create a new agent runner from engine and project configuration.
    ///
    /// Loads prompt templates (built-in and any custom override directories),
    /// and merges model/token settings with CLI overrides taking precedence.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Prompt` if templates cannot be loaded.
    #[instrument(skip_all)]
    pub(crate) fn new(
        config: &EngineConfig,
        project_config: &ProjectConfig,
    ) -> Result<Self, CoreError> {
        let mut pm = gba_pm::PromptManager::new()?;

        // Load custom prompt directories from project config
        for dir in &project_config.prompts.include {
            let resolved = if dir.is_absolute() {
                dir.clone()
            } else {
                config.repo_path().join(dir)
            };
            if resolved.is_dir() {
                pm.load_dir(&resolved)?;
                debug!(dir = %resolved.display(), "loaded custom prompt directory");
            }
        }

        // CLI overrides take precedence over project config
        let model = config
            .model()
            .map(String::from)
            .or_else(|| project_config.agent.model.clone());

        let max_tokens = config.max_tokens().or(project_config.agent.max_tokens);

        Ok(Self {
            prompt_manager: pm,
            model,
            max_tokens,
            permission_mode: project_config.agent.permission_mode.clone(),
        })
    }

    /// Run an agent session and collect all messages.
    ///
    /// Renders the system and task prompts from templates, constructs SDK
    /// options from the agent's `config.yml`, and executes a one-shot query.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Prompt` if template rendering fails.
    /// Returns `CoreError::Agent` if the SDK query fails.
    #[instrument(skip(self, context))]
    pub(crate) async fn run_agent(
        &self,
        agent_name: &str,
        task_template: &str,
        context: &serde_json::Value,
        cwd: Option<&Path>,
    ) -> Result<Vec<Message>, CoreError> {
        let options = self.build_options(agent_name, context, cwd)?;
        let task_prompt = self.prompt_manager.render(task_template, context)?;

        debug!(agent = agent_name, task = task_template, "running agent");

        let messages = claude_agent_sdk_rs::query(&task_prompt, Some(options))
            .await
            .map_err(|e| {
                error!(agent = agent_name, error = %e, "agent query failed");
                CoreError::Agent(format!(
                    "agent {agent_name} failed: {e}. Check your network connection and API credentials."
                ))
            })?;

        Ok(messages)
    }

    /// Run an agent session with a streaming callback for real-time events.
    ///
    /// Same as [`run_agent`](Self::run_agent) but streams messages through
    /// the provided callback as they arrive. Returns the final result message.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Prompt` if template rendering fails.
    /// Returns `CoreError::Agent` if the SDK query fails.
    #[allow(dead_code)] // Will be used for real-time streaming output in run workflow
    #[instrument(skip(self, context, callback))]
    pub(crate) async fn run_agent_stream(
        &self,
        agent_name: &str,
        task_template: &str,
        context: &serde_json::Value,
        cwd: Option<&Path>,
        callback: impl Fn(Message) + Send + 'static,
    ) -> Result<ResultMessage, CoreError> {
        let options = self.build_options(agent_name, context, cwd)?;
        let task_prompt = self.prompt_manager.render(task_template, context)?;

        debug!(
            agent = agent_name,
            task = task_template,
            "running agent (streaming)"
        );

        let mut stream = claude_agent_sdk_rs::query_stream(&task_prompt, Some(options))
            .await
            .map_err(|e| {
                error!(agent = agent_name, error = %e, "agent stream failed");
                CoreError::Agent(format!(
                    "agent {agent_name} stream failed: {e}. Check your network connection and API credentials."
                ))
            })?;

        use futures::StreamExt as _;
        let mut result_msg: Option<ResultMessage> = None;

        while let Some(msg_result) = stream.next().await {
            let msg = msg_result
                .map_err(|e| CoreError::Agent(format!("agent {agent_name} stream error: {e}")))?;

            if let Message::Result(ref r) = msg {
                result_msg = Some(r.clone());
            }

            callback(msg);
        }

        result_msg
            .ok_or_else(|| CoreError::Agent(format!("agent {agent_name} ended without result")))
    }

    /// Returns a reference to the internal prompt manager.
    #[allow(dead_code)] // Used in tests and will be used for template introspection
    pub(crate) fn prompt_manager(&self) -> &gba_pm::PromptManager {
        &self.prompt_manager
    }

    /// Build SDK options for an agent session.
    ///
    /// This is the public variant of the internal `build_options` method,
    /// exposed so that the plan workflow can construct `ClaudeAgentOptions`
    /// for direct use with `ClaudeClient` (bidirectional streaming).
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Agent` if the agent config cannot be loaded.
    /// Returns `CoreError::Prompt` if template rendering fails.
    pub(crate) fn build_agent_options(
        &self,
        agent_name: &str,
        context: &serde_json::Value,
        cwd: Option<&Path>,
    ) -> Result<ClaudeAgentOptions, CoreError> {
        self.build_options(agent_name, context, cwd)
    }

    /// Render a prompt template with the given context.
    ///
    /// # Errors
    ///
    /// Returns `CoreError::Prompt` if template rendering fails.
    pub(crate) fn render_template(
        &self,
        template_name: &str,
        context: &serde_json::Value,
    ) -> Result<String, CoreError> {
        self.prompt_manager
            .render(template_name, context)
            .map_err(CoreError::from)
    }

    /// Build SDK options for an agent session.
    fn build_options(
        &self,
        agent_name: &str,
        context: &serde_json::Value,
        cwd: Option<&Path>,
    ) -> Result<ClaudeAgentOptions, CoreError> {
        let agent_config = gba_pm::PromptManager::load_agent_config(agent_name).map_err(|e| {
            CoreError::Agent(format!("failed to load agent config for {agent_name}: {e}"))
        })?;

        // Render the system prompt
        let system_template = format!("{agent_name}/system");
        let rendered_system = self.prompt_manager.render(&system_template, context)?;

        // Build system prompt based on preset flag
        let system_prompt = if agent_config.preset {
            SystemPrompt::Preset(SystemPromptPreset::with_append(
                "claude_code",
                rendered_system,
            ))
        } else {
            SystemPrompt::Text(rendered_system)
        };

        // Map permission mode
        let sdk_permission_mode = match self.permission_mode {
            PermissionMode::Auto => SdkPermissionMode::BypassPermissions,
            PermissionMode::Manual => SdkPermissionMode::Default,
            PermissionMode::None => SdkPermissionMode::Plan,
        };

        // Build options using struct initialization because typed-builder
        // changes type on each setter call, making conditional fields awkward.
        let tools = if agent_config.tools.is_empty() {
            None
        } else {
            Some(Tools::from(agent_config.tools))
        };

        let options = ClaudeAgentOptions {
            system_prompt: Some(system_prompt),
            permission_mode: Some(sdk_permission_mode),
            disallowed_tools: agent_config.disallowed_tools,
            tools,
            model: self.model.clone(),
            cwd: cwd.map(Path::to_path_buf),
            ..Default::default()
        };

        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_should_create_agent_runner_with_defaults() {
        let engine_config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/test"))
            .build();
        let project_config = ProjectConfig::default();

        let runner = AgentRunner::new(&engine_config, &project_config);
        assert!(runner.is_ok(), "should create runner: {:?}", runner.err());

        let runner = runner.expect("runner should be ok");
        assert!(runner.model.is_none());
        assert!(runner.max_tokens.is_none());
        assert_eq!(runner.permission_mode, PermissionMode::Auto);
    }

    #[test]
    fn test_should_prefer_cli_model_override() {
        let engine_config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/test"))
            .model("cli-model")
            .build();
        let mut project_config = ProjectConfig::default();
        project_config.agent.model = Some("config-model".to_owned());

        let runner =
            AgentRunner::new(&engine_config, &project_config).expect("should create runner");

        // CLI override takes precedence
        assert_eq!(runner.model.as_deref(), Some("cli-model"));
    }

    #[test]
    fn test_should_use_project_model_when_no_cli_override() {
        let engine_config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/test"))
            .build();
        let mut project_config = ProjectConfig::default();
        project_config.agent.model = Some("config-model".to_owned());

        let runner =
            AgentRunner::new(&engine_config, &project_config).expect("should create runner");

        assert_eq!(runner.model.as_deref(), Some("config-model"));
    }

    #[test]
    fn test_should_build_options_for_preset_agent() {
        let engine_config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/test"))
            .build();
        let project_config = ProjectConfig::default();
        let runner =
            AgentRunner::new(&engine_config, &project_config).expect("should create runner");

        let context = serde_json::json!({"repo_path": "/tmp/test"});
        let options = runner.build_options("init", &context, None);
        assert!(options.is_ok(), "should build options: {:?}", options.err());
    }

    #[test]
    fn test_should_build_options_for_non_preset_agent() {
        let engine_config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/test"))
            .build();
        let project_config = ProjectConfig::default();
        let runner =
            AgentRunner::new(&engine_config, &project_config).expect("should create runner");

        let context = serde_json::json!({
            "repo_path": "/tmp/test",
            "feature_slug": "test"
        });
        let options = runner.build_options("review", &context, None);
        assert!(options.is_ok(), "should build options: {:?}", options.err());
    }

    #[test]
    fn test_should_expose_prompt_manager() {
        let engine_config = EngineConfig::builder()
            .repo_path(PathBuf::from("/tmp/test"))
            .build();
        let project_config = ProjectConfig::default();
        let runner =
            AgentRunner::new(&engine_config, &project_config).expect("should create runner");

        let templates = runner.prompt_manager().list_templates();
        assert!(!templates.is_empty(), "should have templates available");
    }
}
