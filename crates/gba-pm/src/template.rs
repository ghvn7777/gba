use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
pub struct PromptTemplate {
    /// Template name used for lookup
    pub name: String,

    /// Raw Jinja2 template source
    pub source: String,

    /// Optional description
    #[builder(default)]
    pub description: Option<String>,
}
