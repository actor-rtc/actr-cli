//! Project template system

pub mod kotlin;
pub mod python;
pub mod rust;
pub mod swift;

pub use crate::commands::SupportedLanguage;
use crate::error::Result;
use crate::utils::{to_pascal_case, to_snake_case};
use clap::ValueEnum;
use handlebars::Handlebars;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

use self::kotlin::KotlinTemplate;
use self::python::PythonTemplate;
use self::rust::RustTemplate;
use self::swift::SwiftTemplate;

/// Project template options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Serialize)]
#[value(rename_all = "lowercase")]
pub enum ProjectTemplateName {
    #[default]
    Echo,
}

impl std::fmt::Display for ProjectTemplateName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pv = self
            .to_possible_value()
            .expect("ValueEnum variant must have a possible value");
        write!(f, "{}", pv.get_name())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateContext {
    #[serde(rename = "PROJECT_NAME")]
    pub project_name: String,
    #[serde(rename = "PROJECT_NAME_SNAKE")]
    pub project_name_snake: String,
    #[serde(rename = "PROJECT_NAME_PASCAL")]
    pub project_name_pascal: String,
    #[serde(rename = "SIGNALING_URL")]
    pub signaling_url: String,
    #[serde(rename = "MANUFACTURER")]
    pub manufacturer: String,
    #[serde(rename = "SERVICE_NAME")]
    pub service_name: String,
}

impl TemplateContext {
    pub fn new(project_name: &str, signaling_url: &str) -> Self {
        Self {
            project_name: project_name.to_string(),
            project_name_snake: to_snake_case(project_name),
            project_name_pascal: to_pascal_case(project_name),
            signaling_url: signaling_url.to_string(),
            manufacturer: "acme".to_string(),
            service_name: "EchoService".to_string(),
        }
    }
}

pub trait LangTemplate: Send + Sync {
    fn load_files(&self, template_name: ProjectTemplateName) -> Result<HashMap<String, String>>;
}

pub struct ProjectTemplate {
    name: ProjectTemplateName,
    lang_template: Box<dyn LangTemplate>,
}

impl ProjectTemplate {
    pub fn new(template_name: ProjectTemplateName, language: SupportedLanguage) -> Self {
        let lang_template: Box<dyn LangTemplate> = match language {
            SupportedLanguage::Swift => Box::new(SwiftTemplate),
            SupportedLanguage::Kotlin => Box::new(KotlinTemplate),
            SupportedLanguage::Python => Box::new(PythonTemplate),
            SupportedLanguage::Rust => Box::new(RustTemplate),
        };

        Self {
            name: template_name,
            lang_template,
        }
    }

    pub fn load_file(
        fixture_path: &Path,
        files: &mut HashMap<String, String>,
        key: &str,
    ) -> Result<()> {
        let content = std::fs::read_to_string(fixture_path)?;
        files.insert(key.to_string(), content);
        Ok(())
    }

    pub fn generate(&self, project_path: &Path, context: &TemplateContext) -> Result<()> {
        let files = self.lang_template.load_files(self.name)?;
        let handlebars = Handlebars::new();

        for (file_path, content) in &files {
            let rendered_path = handlebars.render_template(file_path, context)?;
            let rendered_content = handlebars.render_template(content, context)?;

            let full_path = project_path.join(&rendered_path);

            // Create parent directories if they don't exist
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::write(full_path, rendered_content)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_template_context() {
        let ctx = TemplateContext::new("my-chat-service", "ws://localhost:8080");
        assert_eq!(ctx.project_name, "my-chat-service");
        assert_eq!(ctx.project_name_snake, "my_chat_service");
        assert_eq!(ctx.project_name_pascal, "MyChatService");
        assert_eq!(ctx.signaling_url, "ws://localhost:8080");
    }

    #[test]
    fn test_project_template_new() {
        let template = ProjectTemplate::new(ProjectTemplateName::Echo, SupportedLanguage::Swift);
        assert_eq!(template.name, ProjectTemplateName::Echo);
    }

    #[test]
    fn test_project_template_generation() {
        let temp_dir = TempDir::new().unwrap();
        let template = ProjectTemplate::new(ProjectTemplateName::Echo, SupportedLanguage::Swift);
        let context = TemplateContext::new("test-app", "ws://localhost:8080");

        template
            .generate(temp_dir.path(), &context)
            .expect("Failed to generate");

        // Verify project.yml exists
        assert!(temp_dir.path().join("project.yml").exists());
        // Verify Actr.toml exists
        assert!(temp_dir.path().join("Actr.toml").exists());
        // Verify .gitignore exists
        assert!(temp_dir.path().join(".gitignore").exists());
        // Verify proto file exists
        assert!(temp_dir.path().join("protos/echo.proto").exists());
        // Verify app directory exists
        assert!(
            temp_dir
                .path()
                .join("TestApp")
                .join("TestAppApp.swift")
                .exists()
        );
    }

    #[test]
    fn test_project_template_load_files() {
        let template = ProjectTemplate::new(ProjectTemplateName::Echo, SupportedLanguage::Swift);
        let result = template.lang_template.load_files(ProjectTemplateName::Echo);
        assert!(result.is_ok());
    }
}
