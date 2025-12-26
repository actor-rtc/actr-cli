use super::{LangTemplate, ProjectTemplateName};
use crate::error::{ActrCliError, Result};
use std::collections::HashMap;

pub struct KotlinTemplate;

impl LangTemplate for KotlinTemplate {
    fn load_files(&self, _template_name: ProjectTemplateName) -> Result<HashMap<String, String>> {
        Err(ActrCliError::Unsupported(
            "Kotlin project initialization is not implemented yet".to_string(),
        ))
    }
}
