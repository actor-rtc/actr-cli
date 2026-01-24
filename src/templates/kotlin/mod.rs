use super::{LangTemplate, ProjectTemplateName};
use crate::error::Result;
use std::collections::HashMap;

pub mod data_stream;
pub mod echo;

pub struct KotlinTemplate;

impl LangTemplate for KotlinTemplate {
    fn load_files(&self, template_name: ProjectTemplateName) -> Result<HashMap<String, String>> {
        let mut files = HashMap::new();

        match template_name {
            ProjectTemplateName::Echo => {
                echo::load(&mut files)?;
            }
            ProjectTemplateName::DataStream => {
                data_stream::load(&mut files)?;
            }
        }

        Ok(files)
    }
}
