pub mod echo;

use super::{LangTemplate, ProjectTemplateName};
use crate::error::Result;
use std::collections::HashMap;

pub struct PythonTemplate;

impl LangTemplate for PythonTemplate {
    fn load_files(
        &self,
        template_name: ProjectTemplateName,
        service_name: &str,
    ) -> Result<HashMap<String, String>> {
        let mut files = HashMap::new();

        match template_name {
            ProjectTemplateName::Echo => {
                echo::load(&mut files, service_name)?;
            }
        }

        Ok(files)
    }
}
