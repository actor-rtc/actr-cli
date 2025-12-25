use super::{InitContext, ProjectInitializer};
use crate::error::{ActrCliError, Result};

pub struct KotlinInitializer;

impl ProjectInitializer for KotlinInitializer {
    fn generate_project_structure(&self, _context: &InitContext) -> Result<()> {
        Err(ActrCliError::Unsupported(
            "Kotlin project initialization is not implemented yet".to_string(),
        ))
    }

    fn print_next_steps(&self, _context: &InitContext) {}
}
