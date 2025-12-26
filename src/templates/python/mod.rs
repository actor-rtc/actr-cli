use super::{LangTemplate, ProjectTemplateName};
use crate::error::{ActrCliError, Result};
use std::collections::HashMap;

pub struct PythonTemplate;

impl LangTemplate for PythonTemplate {
    fn load_files(&self, _template_name: ProjectTemplateName) -> Result<HashMap<String, String>> {
        Err(ActrCliError::Unsupported(
            "Python project initialization is not implemented yet".to_string(),
        ))
    }
}
