use crate::{error::Result, template::ProjectTemplateName};
use std::path::PathBuf;

/// Context for non-Rust project initialization.
#[derive(Debug, Clone)]
pub struct InitContext {
    pub project_dir: PathBuf,
    pub project_name: String,
    pub signaling_url: String,
    pub template: ProjectTemplateName,
    pub is_current_dir: bool,
}

/// Interface for language-specific project initialization.
pub trait ProjectInitializer: Send + Sync {
    fn generate_project_structure(&self, context: &InitContext) -> Result<()>;
    fn print_next_steps(&self, context: &InitContext);
}
