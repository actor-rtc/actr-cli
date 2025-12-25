use super::{InitContext, ProjectInitializer};
use crate::error::{ActrCliError, Result};

pub struct PythonInitializer;

impl ProjectInitializer for PythonInitializer {
    fn generate_project_structure(&self, _context: &InitContext) -> Result<()> {
        Err(ActrCliError::Unsupported(
            "Python project initialization is not implemented yet".to_string(),
        ))
    }

    fn print_next_steps(&self, _context: &InitContext) {}
}
