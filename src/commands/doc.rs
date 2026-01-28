//! Doc command implementation - generate project documentation
//!
//! Now uses Handlebars templates and embedded assets for maintainability and portability.

use crate::assets::FixtureAssets;
use crate::commands::Command;
use crate::error::{ActrCliError, Result};
use actr_config::{Config, ConfigParser};
use async_trait::async_trait;
use clap::Args;
use handlebars::Handlebars;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

#[derive(Args)]
#[command(
    about = "Generate project documentation",
    long_about = "Generate static HTML documentation for the project, including project overview, API (Proto) reference, and configuration guide."
)]
pub struct DocCommand {
    /// Output directory for documentation (defaults to "./docs")
    #[arg(short = 'o', long = "output")]
    pub output_dir: Option<String>,
}

#[derive(Serialize)]
struct BaseContext {
    project_name: String,
    project_version: String,
    project_description: String,
    page_title: String,
    is_overview: bool,
    is_api: bool,
    is_config: bool,
    // Project type flags
    is_rust: bool,
    is_swift: bool,
    is_kotlin: bool,
    is_python: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ProjectType {
    Rust,
    Swift,
    Kotlin,
    Python,
}

impl ProjectType {
    fn detect() -> Self {
        if Path::new("project.yml").exists() || Path::new("Package.swift").exists() {
            ProjectType::Swift
        } else if Path::new("build.gradle.kts").exists() || Path::new("build.gradle").exists() {
            ProjectType::Kotlin
        } else if Path::new("pyproject.toml").exists() || Path::new("requirements.txt").exists() {
            ProjectType::Python
        } else {
            ProjectType::Rust
        }
    }
}

#[derive(Serialize)]
struct IndexContext {
    #[serde(flatten)]
    base: BaseContext,
    project_structure: String,
}

#[derive(Serialize)]
struct ApiContext {
    #[serde(flatten)]
    base: BaseContext,
    proto_files: Vec<ProtoFile>,
}

#[derive(Serialize)]
struct ProtoFile {
    filename: String,
    content: String,
}

#[derive(Serialize)]
struct ConfigContext {
    #[serde(flatten)]
    base: BaseContext,
    config_example: String,
}

#[async_trait]
impl Command for DocCommand {
    async fn execute(&self) -> Result<()> {
        let output_dir = self.output_dir.as_deref().unwrap_or("docs");

        if !Path::new("Actr.toml").exists()
            && let Some(root) = Self::find_project_root()
        {
            return Err(ActrCliError::InvalidProject(format!(
                "Actr.toml found at '{}'. Please run 'actr doc' from the project root.",
                root.display()
            )));
        }

        info!("📚 Generating project documentation to: {}", output_dir);

        // Create output directory
        std::fs::create_dir_all(output_dir)?;

        // Load project configuration
        let config = if Path::new("Actr.toml").exists() {
            Some(ConfigParser::from_file("Actr.toml")?)
        } else {
            None
        };

        // Initialize Handlebars
        let hb = self.init_handlebars()?;

        // Generate documentation files
        self.generate_index_html(output_dir, &config, &hb).await?;
        self.generate_api_html(output_dir, &config, &hb).await?;
        self.generate_config_html(output_dir, &config, &hb).await?;

        info!("✅ Documentation generated successfully");
        info!("📄 Generated files:");
        info!("  - {}/index.html (project overview)", output_dir);
        info!("  - {}/api.html (API interface documentation)", output_dir);
        info!(
            "  - {}/config.html (configuration documentation)",
            output_dir
        );

        println!();
        println!("🚀 To preview the documentation locally:");
        println!("   python3 -m http.server --directory {} 8080", output_dir);
        println!("   # or");
        println!("   npx http-server {} -p 8080", output_dir);
        println!();

        Ok(())
    }
}

impl DocCommand {
    fn init_handlebars(&self) -> Result<Handlebars<'static>> {
        let mut hb = Handlebars::new();

        // Helper to load template from assets
        let load_template = |name: &str| -> Result<String> {
            let path = format!("templates/doc/{}.hbs", name);
            let file = FixtureAssets::get(&path).ok_or_else(|| {
                ActrCliError::Internal(anyhow::anyhow!("Template not found: {}", path))
            })?;
            let content = std::str::from_utf8(file.data.as_ref())
                .map_err(|e| ActrCliError::Internal(anyhow::anyhow!("Invalid UTF-8: {}", e)))?
                .to_string();
            Ok(content)
        };

        // Register partials
        hb.register_partial("head", load_template("_head")?)
            .map_err(|e| ActrCliError::Internal(anyhow::anyhow!(e)))?;
        hb.register_partial("nav", load_template("_nav")?)
            .map_err(|e| ActrCliError::Internal(anyhow::anyhow!(e)))?;

        // Register templates
        hb.register_template_string("index", load_template("index")?)
            .map_err(|e| ActrCliError::Internal(anyhow::anyhow!(e)))?;
        hb.register_template_string("api", load_template("api")?)
            .map_err(|e| ActrCliError::Internal(anyhow::anyhow!(e)))?;
        hb.register_template_string("config", load_template("config")?)
            .map_err(|e| ActrCliError::Internal(anyhow::anyhow!(e)))?;

        Ok(hb)
    }

    fn create_base_context(
        &self,
        config: &Option<Config>,
        title: &str,
        active_nav: &str,
    ) -> BaseContext {
        let project_name = config
            .as_ref()
            .map(|c| c.package.name.clone())
            .unwrap_or_else(|| "Actor-RTC Project".to_string());
        let project_version = Self::read_project_version().unwrap_or_else(|| "unknown".to_string());

        let project_description = config
            .as_ref()
            .and_then(|c| c.package.description.clone())
            .unwrap_or_else(|| "An Actor-RTC project".to_string());

        let project_type = ProjectType::detect();

        BaseContext {
            project_name,
            project_version,
            project_description,
            page_title: title.to_string(),
            is_overview: active_nav == "overview",
            is_api: active_nav == "api",
            is_config: active_nav == "config",
            is_rust: project_type == ProjectType::Rust,
            is_swift: project_type == ProjectType::Swift,
            is_kotlin: project_type == ProjectType::Kotlin,
            is_python: project_type == ProjectType::Python,
        }
    }

    /// Generate project overview documentation
    async fn generate_index_html(
        &self,
        output_dir: &str,
        config: &Option<Config>,
        hb: &Handlebars<'_>,
    ) -> Result<()> {
        debug!("Generating index.html...");

        let base_context = self.create_base_context(config, "Project Overview", "overview");
        let project_name = &base_context.project_name;
        // Re-detect or use flags? I'll re-use the detection logic via flags for structure
        let project_type = if base_context.is_swift {
            ProjectType::Swift
        } else if base_context.is_kotlin {
            ProjectType::Kotlin
        } else if base_context.is_python {
            ProjectType::Python
        } else {
            ProjectType::Rust
        };

        let project_structure = self.detect_project_structure(project_name, project_type);

        let context = IndexContext {
            base: base_context,
            project_structure,
        };

        let content = hb.render("index", &context)?;
        let index_path = Path::new(output_dir).join("index.html");
        std::fs::write(index_path, content)?;

        Ok(())
    }

    fn detect_project_structure(&self, project_name: &str, project_type: ProjectType) -> String {
        let mut tree = format!(
            "{}/\n├── Actr.toml          # Project configuration\n",
            project_name
        );

        match project_type {
            ProjectType::Swift => {
                tree.push_str("├── project.yml        # XcodeGen configuration\n");
                tree.push_str(&format!("├── {}/          # Source code\n", project_name));
                tree.push_str("│   ├── App.swift      # Entrypoint\n");
                tree.push_str("│   └── Generated/     # Generated code\n");
            }
            ProjectType::Kotlin => {
                tree.push_str("├── build.gradle.kts   # Gradle configuration\n");
                tree.push_str("├── app/               # App module\n");
                tree.push_str("│   └── src/           # Source code\n");
                tree.push_str("│       └── main/java/ # Java/Kotlin source\n");
            }
            ProjectType::Python => {
                tree.push_str("├── main.py            # Entrypoint\n");
                tree.push_str("└── generated/         # Generated code\n");
            }
            ProjectType::Rust => {
                if Path::new("Cargo.toml").exists() {
                    tree.push_str("├── Cargo.toml         # Rust manifest\n");
                }
                tree.push_str("├── src/               # Source code\n");
                tree.push_str("│   ├── main.rs        # Entrypoint\n");
                tree.push_str("│   └── generated/     # Generated code\n");
            }
        }

        tree.push_str("├── protos/\n");
        tree.push_str("│   ├── local/         # Your service definitions\n");
        tree.push_str("│   └── remote/        # Installed dependencies\n");
        tree.push_str("└── docs/              # Project documentation");
        tree
    }

    /// Generate API documentation
    async fn generate_api_html(
        &self,
        output_dir: &str,
        config: &Option<Config>,
        hb: &Handlebars<'_>,
    ) -> Result<()> {
        debug!("Generating api.html...");

        // Collect proto files information
        let mut proto_files = Vec::new();
        let proto_dir = Path::new("protos");

        if proto_dir.exists() {
            for entry in WalkDir::new(proto_dir).into_iter().flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("proto") {
                    // Use relative path for better context (e.g., "local/local.proto")
                    let relative_path = path.strip_prefix(proto_dir).unwrap_or(path);
                    let filename = relative_path.to_string_lossy().to_string();

                    let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
                        warn!("Failed to read proto file {:?}: {}", path, e);
                        String::new()
                    });
                    proto_files.push(ProtoFile { filename, content });
                }
            }
        }
        proto_files.sort_by(|a, b| a.filename.cmp(&b.filename));

        let context = ApiContext {
            base: self.create_base_context(config, "API Documentation", "api"),
            proto_files,
        };

        let content = hb.render("api", &context)?;
        let api_path = Path::new(output_dir).join("api.html");
        std::fs::write(api_path, content)?;

        Ok(())
    }

    /// Generate configuration documentation
    async fn generate_config_html(
        &self,
        output_dir: &str,
        config: &Option<Config>,
        hb: &Handlebars<'_>,
    ) -> Result<()> {
        debug!("Generating config.html...");

        // Generate configuration example
        let config_example = if Path::new("Actr.toml").exists() {
            std::fs::read_to_string("Actr.toml").unwrap_or_default()
        } else {
            r#"edition = 1
exports = []

[package]
name = "my-actor-service"
description = "An Actor-RTC service"
authors = []
license = "Apache-2.0"
tags = ["latest"]

[package.actr_type]
manufacturer = "my-company"
name = "my-actor-service"

[dependencies]

[system.signaling]
url = "ws://127.0.0.1:8080"

[system.deployment]
realm_id = 1001

[system.discovery]
visible = true

[scripts]
dev = "cargo run"
test = "cargo test""#
                .to_string()
        };

        let context = ConfigContext {
            base: self.create_base_context(config, "Configuration", "config"),
            config_example,
        };

        let content = hb.render("config", &context)?;
        let config_path = Path::new(output_dir).join("config.html");
        std::fs::write(config_path, content)?;

        Ok(())
    }

    fn read_project_version() -> Option<String> {
        // 1. Try Cargo.toml
        if let Ok(cargo_toml) = std::fs::read_to_string("Cargo.toml")
            && let Ok(value) = cargo_toml.parse::<toml::Value>()
            && let Some(version) = value
                .get("package")
                .and_then(|package| package.get("version"))
                .and_then(|version| version.as_str())
        {
            return Some(version.to_string());
        }

        // 2. Try project.yml (XcodeGen)
        if let Ok(project_yml) = std::fs::read_to_string("project.yml")
            && let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&project_yml)
            && let Some(targets) = value.get("targets").and_then(|t| t.as_mapping())
        {
            for (_target_name, target_config) in targets {
                // Check settings.MARKETING_VERSION
                if let Some(version) = target_config
                    .get("settings")
                    .and_then(|s| s.get("MARKETING_VERSION"))
                {
                    if let Some(s) = version.as_str() {
                        return Some(s.to_string());
                    }
                    // Handle numbers (e.g. 1.0)
                    if let Some(f) = version.as_f64() {
                        return Some(f.to_string());
                    }
                    if let Some(i) = version.as_i64() {
                        return Some(i.to_string());
                    }
                }
            }
        }

        // 3. Try Gradle (Kotlin/Groovy)
        if let Some(version) = Self::read_gradle_version("build.gradle.kts")
            .or_else(|| Self::read_gradle_version("build.gradle"))
        {
            return Some(version);
        }

        // 4. Try pyproject.toml (PEP 621 / Poetry)
        if let Ok(pyproject) = std::fs::read_to_string("pyproject.toml")
            && let Ok(value) = pyproject.parse::<toml::Value>()
        {
            if let Some(version) = value
                .get("project")
                .and_then(|p| p.get("version"))
                .and_then(|v| v.as_str())
            {
                return Some(version.to_string());
            }
            if let Some(version) = value
                .get("tool")
                .and_then(|t| t.get("poetry"))
                .and_then(|p| p.get("version"))
                .and_then(|v| v.as_str())
            {
                return Some(version.to_string());
            }
        }

        None
    }

    fn read_gradle_version(path: &str) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
            {
                continue;
            }

            let rest = match trimmed.strip_prefix("version") {
                Some(rest) => rest.trim_start(),
                None => continue,
            };
            let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();

            if let Some(rest) = rest.strip_prefix('"')
                && let Some(end) = rest.find('"')
            {
                return Some(rest[..end].to_string());
            }
            if let Some(rest) = rest.strip_prefix('\'')
                && let Some(end) = rest.find('\'')
            {
                return Some(rest[..end].to_string());
            }
        }

        None
    }

    fn find_project_root() -> Option<PathBuf> {
        let cwd = std::env::current_dir().ok()?;
        for ancestor in cwd.ancestors() {
            if ancestor.join("Actr.toml").exists() {
                return Some(ancestor.to_path_buf());
            }
        }
        None
    }
}
