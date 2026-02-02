use std::path::Path;

use minijinja::Environment;

use crate::error::PmError;
use crate::template::PromptTemplate;

/// Manages prompt templates and renders them with context variables.
pub struct PromptManager<'a> {
    _env: Environment<'a>,
}

impl<'a> PromptManager<'a> {
    /// Create a new empty PromptManager.
    pub fn new() -> Self {
        Self {
            _env: Environment::new(),
        }
    }

    /// Load all `.j2` / `.jinja` templates from a directory.
    pub fn load_dir(&mut self, _dir: &Path) -> Result<(), PmError> {
        todo!()
    }

    /// Register a single template.
    pub fn add_template(&mut self, _template: PromptTemplate) -> Result<(), PmError> {
        todo!()
    }

    /// Render a template by name with the given context.
    pub fn render(&self, _name: &str, _ctx: &serde_json::Value) -> Result<String, PmError> {
        todo!()
    }
}

impl Default for PromptManager<'_> {
    fn default() -> Self {
        Self::new()
    }
}
