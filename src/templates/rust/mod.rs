pub mod echo;

pub use echo::load;

use super::{LangTemplate, ProjectTemplateName};
use crate::error::Result;
use std::collections::HashMap;

pub struct RustTemplate;

impl LangTemplate for RustTemplate {
    fn load_files(&self, template_name: ProjectTemplateName) -> Result<HashMap<String, String>> {
        let mut files = HashMap::new();

        match template_name {
            ProjectTemplateName::Echo => {
                echo::load(&mut files)?;
            }
        }

        Ok(files)
    }
}
