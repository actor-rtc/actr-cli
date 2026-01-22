//! Project initialization command

use crate::commands::initialize::{self, InitContext};
use crate::commands::{Command, SupportedLanguage};
use crate::error::{ActrCliError, Result};
use crate::template::ProjectTemplateName;
use async_trait::async_trait;
use clap::Args;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Args)]
pub struct InitCommand {
    /// Name of the project to create (use '.' for current directory)
    pub name: Option<String>,

    /// Project template to use (echo, data-stream)
    #[arg(long, default_value_t = ProjectTemplateName::Echo)]
    pub template: ProjectTemplateName,

    /// Project name when initializing in current directory
    #[arg(long)]
    pub project_name: Option<String>,

    /// Signaling server URL
    #[arg(long)]
    pub signaling: Option<String>,

    /// Target language for project initialization
    #[arg(short, long, default_value = "rust")]
    pub language: SupportedLanguage,
}

#[async_trait]
impl Command for InitCommand {
    async fn execute(&self) -> Result<()> {
        // Show welcome header
        println!("🎯 Actor-RTC Project Initialization");
        println!("----------------------------------------");

        // Interactive prompt for missing required fields
        let name = self.prompt_if_missing("project name", self.name.as_ref())?;
        let signaling_url =
            self.prompt_if_missing("signaling server URL", self.signaling.as_ref())?;

        let (project_dir, project_name) = self.resolve_project_info(&name)?;

        info!("🚀 Initializing Actor-RTC project: {}", project_name);

        // Check if target directory exists and is not empty
        if project_dir.exists() && project_dir != Path::new(".") {
            return Err(ActrCliError::InvalidProject(format!(
                "Directory '{}' already exists. Use a different name or remove the existing directory.",
                project_dir.display()
            )));
        }

        // Check if current directory already has Actr.toml
        if project_dir == Path::new(".") && Path::new("Actr.toml").exists() {
            return Err(ActrCliError::InvalidProject(
                "Current directory already contains an Actor-RTC project (Actr.toml exists)"
                    .to_string(),
            ));
        }

        // Create project directory if needed
        if project_dir != Path::new(".") {
            std::fs::create_dir_all(&project_dir)?;
        }

        let context = InitContext {
            project_dir: project_dir.clone(),
            project_name: project_name.clone(),
            signaling_url: signaling_url.clone(),
            template: self.template,
            is_current_dir: project_dir == Path::new("."),
        };

        initialize::execute_initialize(self.language, &context).await?;

        Ok(())
    }
}

impl InitCommand {
    fn resolve_project_info(&self, name: &str) -> Result<(PathBuf, String)> {
        if name == "." {
            // Initialize in current directory - name will be inferred
            let project_name = if let Some(name) = &self.project_name {
                name.clone()
            } else {
                let current_dir = std::env::current_dir().map_err(|e| {
                    ActrCliError::InvalidProject(format!(
                        "Failed to resolve current directory: {e}"
                    ))
                })?;
                current_dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| {
                        ActrCliError::InvalidProject(
                            "Failed to infer project name from current directory".to_string(),
                        )
                    })?
            };
            Ok((PathBuf::from("."), project_name))
        } else {
            // Create new directory - extract project name from path
            let path = PathBuf::from(name);
            let project_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(name)
                .to_string();
            Ok((path, project_name))
        }
    }

    /// Interactive prompt for missing fields with detailed guidance
    fn prompt_if_missing(
        &self,
        field_name: &str,
        current_value: Option<&String>,
    ) -> Result<String> {
        if let Some(value) = current_value {
            return Ok(value.clone());
        }

        match field_name {
            "project name" => {
                println!("┌──────────────────────────────────────────────────────────┐");
                println!("│ 📋  Project Name Configuration                           │");
                println!("├──────────────────────────────────────────────────────────┤");
                println!("│                                                          │");
                println!("│  📝 Requirements:                                        │");
                println!("│     • Only alphanumeric characters, hyphens and _        │");
                println!("│     • Cannot start or end with - or _                    │");
                println!("│                                                          │");
                println!("│  💡 Examples:                                            │");
                println!("│     my-chat-service, user-manager, media_streamer        │");
                println!("│                                                          │");
                println!("└──────────────────────────────────────────────────────────┘");
                print!("🎯 Enter project name [my-actor-project]: ");
            }
            "signaling server URL" => {
                println!("┌──────────────────────────────────────────────────────────┐");
                println!("│ 🌐  Signaling Server Configuration                       │");
                println!("├──────────────────────────────────────────────────────────┤");
                println!("│                                                          │");
                println!("│  📡 WebSocket URL for Actor-RTC signaling coordination   │");
                println!("│                                                          │");
                println!("│  💡 Examples:                                            │");
                println!("│     ws://localhost:8080/                (development)    │");
                println!("│     wss://example.com                   (production      │");
                println!("│     wss://example.com/?token=${{TOKEN}}   (with auth)    │");
                println!("│                                                          │");
                println!("└──────────────────────────────────────────────────────────┘");
                print!("🎯 Enter signaling server URL [wss://actrix1.develenv.com]: ");
            }
            _ => {
                print!("🎯 Enter {field_name}: ");
            }
        }

        io::stdout().flush().map_err(ActrCliError::Io)?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(ActrCliError::Io)?;

        println!();

        let trimmed = input.trim();
        if trimmed.is_empty() {
            // Provide sensible defaults
            let default = match field_name {
                "project name" => "my-actor-project",
                "signaling server URL" => "wss://actrix1.develenv.com/signaling/ws",
                _ => {
                    return Err(ActrCliError::InvalidProject(format!(
                        "{field_name} cannot be empty"
                    )));
                }
            };
            Ok(default.to_string())
        } else {
            // Validate project name if applicable
            if field_name == "project name" {
                self.validate_project_name(trimmed)?;
            }
            Ok(trimmed.to_string())
        }
    }

    /// Validate project name according to requirements
    fn validate_project_name(&self, name: &str) -> Result<()> {
        // Check if name is valid: alphanumeric characters, hyphens, and underscores only
        let is_valid = name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');

        if !is_valid {
            return Err(ActrCliError::InvalidProject(format!(
                "Invalid project name '{name}'. Only alphanumeric characters, hyphens, and underscores are allowed."
            )));
        }

        // Check for other common invalid patterns
        if name.is_empty() {
            return Err(ActrCliError::InvalidProject(
                "Project name cannot be empty".to_string(),
            ));
        }

        if name.starts_with('-') || name.ends_with('-') {
            return Err(ActrCliError::InvalidProject(
                "Project name cannot start or end with a hyphen".to_string(),
            ));
        }

        if name.starts_with('_') || name.ends_with('_') {
            return Err(ActrCliError::InvalidProject(
                "Project name cannot start or end with an underscore".to_string(),
            ));
        }

        Ok(())
    }
}
