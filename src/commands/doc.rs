//! Doc command implementation - generate project documentation

use crate::commands::Command;
use crate::error::Result;
use actr_config::{Config, ConfigParser};
use async_trait::async_trait;
use clap::Args;
use std::path::Path;
use tracing::{debug, info};

#[derive(Args)]
pub struct DocCommand {
    /// Output directory for documentation (defaults to "./docs")
    #[arg(short = 'o', long = "output")]
    pub output_dir: Option<String>,
}

#[async_trait]
impl Command for DocCommand {
    async fn execute(&self) -> Result<()> {
        let output_dir = self.output_dir.as_deref().unwrap_or("docs");

        info!("📚 Generating project documentation to: {}", output_dir);

        // Create output directory
        std::fs::create_dir_all(output_dir)?;

        // Load project configuration
        let config = if Path::new("Actr.toml").exists() {
            Some(ConfigParser::from_file("Actr.toml")?)
        } else {
            None
        };

        // Generate documentation files
        self.generate_index_html(output_dir, &config).await?;
        self.generate_api_html(output_dir, &config).await?;
        self.generate_config_html(output_dir, &config).await?;

        info!("✅ Documentation generated successfully");
        info!("📄 Generated files:");
        info!("  - {}/index.html (project overview)", output_dir);
        info!("  - {}/api.html (API interface documentation)", output_dir);
        info!(
            "  - {}/config.html (configuration documentation)",
            output_dir
        );

        Ok(())
    }
}

impl DocCommand {
    /// Generate project overview documentation
    async fn generate_index_html(&self, output_dir: &str, config: &Option<Config>) -> Result<()> {
        debug!("Generating index.html...");

        let project_name = config
            .as_ref()
            .map(|c| c.package.name.as_str())
            .unwrap_or("Actor-RTC Project");
        // Config does not expose a version; fall back to Cargo.toml when available.
        let project_version = Self::read_cargo_version().unwrap_or_else(|| "unknown".to_string());
        let project_description = config
            .as_ref()
            .and_then(|c| c.package.description.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("An Actor-RTC project");

        let html_content = format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{project_name} - Project Overview</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 0; padding: 20px; line-height: 1.6; }}
        .header {{ background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }}
        .content {{ max-width: 800px; margin: 0 auto; }}
        .section {{ background: white; padding: 20px; margin: 20px 0; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }}
        .nav {{ display: flex; gap: 10px; margin: 20px 0; }}
        .nav a {{ padding: 10px 20px; background: #f0f0f0; text-decoration: none; color: #333; border-radius: 4px; }}
        .nav a:hover {{ background: #667eea; color: white; }}
        h1, h2 {{ margin-top: 0; }}
        .badge {{ background: #667eea; color: white; padding: 4px 8px; border-radius: 4px; font-size: 0.8em; }}
    </style>
</head>
<body>
    <div class="content">
        <div class="header">
            <h1>{project_name}</h1>
            <p>{project_description}</p>
            <span class="badge">v{project_version}</span>
        </div>

        <div class="nav">
            <a href="index.html">Overview</a>
            <a href="api.html">API Docs</a>
            <a href="config.html">Configuration</a>
        </div>

        <div class="section">
            <h2>Project Info</h2>
            <p><strong>Name:</strong> {project_name}</p>
            <p><strong>Version:</strong> {project_version}</p>
            <p><strong>Description:</strong> {project_description}</p>
        </div>

        <div class="section">
            <h2>Common Commands</h2>
            <p>Run these from the project root:</p>
            <pre><code># Generate code from proto files
actr gen --input proto --output src/generated

# Install dependencies from Actr.toml
actr install

# Discover services on the network
actr discovery

# Validate dependencies (currently a placeholder command)
actr check --verbose</code></pre>
        </div>

        <div class="section">
            <h2>Project Structure</h2>
            <pre><code>{project_name}/
├── Actr.toml          # Project configuration
├── src/               # Source code
│   ├── main.rs        # Entrypoint
│   └── generated/     # Generated code
├── protos/            # Protocol Buffers definitions
└── docs/              # Project documentation</code></pre>
        </div>

        <div class="section">
            <h2>Related Links</h2>
            <ul>
                <li><a href="api.html">API Documentation</a> - Service interface definitions</li>
                <li><a href="config.html">Configuration</a> - Project configuration reference</li>
            </ul>
        </div>
    </div>
</body>
</html>"#
        );

        let index_path = Path::new(output_dir).join("index.html");
        std::fs::write(index_path, html_content)?;

        Ok(())
    }

    /// Generate API documentation
    async fn generate_api_html(&self, output_dir: &str, config: &Option<Config>) -> Result<()> {
        debug!("Generating api.html...");

        let project_name = config
            .as_ref()
            .map(|c| c.package.name.as_str())
            .unwrap_or("Actor-RTC Project");

        // Collect proto files information
        let mut proto_info = Vec::new();
        let proto_dir = Path::new("protos");

        if proto_dir.exists()
            && let Ok(entries) = std::fs::read_dir(proto_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("proto") {
                    let filename = path.file_name().unwrap().to_string_lossy();
                    let content = std::fs::read_to_string(&path).unwrap_or_default();
                    proto_info.push((filename.to_string(), content));
                }
            }
        }

        let mut proto_sections = String::new();
        if proto_info.is_empty() {
            proto_sections.push_str(
                r#"<div class="section">
                <p>No Protocol Buffers files found.</p>
            </div>"#,
            );
        } else {
            for (filename, content) in proto_info {
                proto_sections.push_str(&format!(
                    r#"<div class="section">
                    <h3>{}</h3>
                    <pre><code>{}</code></pre>
                </div>"#,
                    filename,
                    Self::html_escape(&content)
                ));
            }
        }

        let html_content = format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{project_name} - API Documentation</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 0; padding: 20px; line-height: 1.6; }}
        .header {{ background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }}
        .content {{ max-width: 1000px; margin: 0 auto; }}
        .section {{ background: white; padding: 20px; margin: 20px 0; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }}
        .nav {{ display: flex; gap: 10px; margin: 20px 0; }}
        .nav a {{ padding: 10px 20px; background: #f0f0f0; text-decoration: none; color: #333; border-radius: 4px; }}
        .nav a:hover {{ background: #667eea; color: white; }}
        .nav a.active {{ background: #667eea; color: white; }}
        h1, h2, h3 {{ margin-top: 0; }}
        pre {{ background: #f5f5f5; padding: 15px; border-radius: 4px; overflow-x: auto; }}
        code {{ font-family: 'Monaco', 'Consolas', monospace; }}
    </style>
</head>
<body>
    <div class="content">
        <div class="header">
            <h1>{project_name} - API Documentation</h1>
            <p>Service interfaces and protocol definitions</p>
        </div>

        <div class="nav">
            <a href="index.html">Overview</a>
            <a href="api.html" class="active">API Docs</a>
            <a href="config.html">Configuration</a>
        </div>

        <div class="section">
            <h2>Protocol Buffers Definitions</h2>
            <p>Protocol Buffers files found in this project:</p>
        </div>

        {proto_sections}
    </div>
</body>
</html>"#
        );

        let api_path = Path::new(output_dir).join("api.html");
        std::fs::write(api_path, html_content)?;

        Ok(())
    }

    /// Generate configuration documentation
    async fn generate_config_html(&self, output_dir: &str, config: &Option<Config>) -> Result<()> {
        debug!("Generating config.html...");

        let project_name = config
            .as_ref()
            .map(|c| c.package.name.as_str())
            .unwrap_or("Actor-RTC Project");

        // Generate configuration example
        // Note: Config doesn't implement Serialize, read raw Actr.toml instead
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

        let html_content = format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} - Configuration</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 0; padding: 20px; line-height: 1.6; }}
        .header {{ background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }}
        .content {{ max-width: 1000px; margin: 0 auto; }}
        .section {{ background: white; padding: 20px; margin: 20px 0; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }}
        .nav {{ display: flex; gap: 10px; margin: 20px 0; }}
        .nav a {{ padding: 10px 20px; background: #f0f0f0; text-decoration: none; color: #333; border-radius: 4px; }}
        .nav a:hover {{ background: #667eea; color: white; }}
        .nav a.active {{ background: #667eea; color: white; }}
        h1, h2, h3 {{ margin-top: 0; }}
        pre {{ background: #f5f5f5; padding: 15px; border-radius: 4px; overflow-x: auto; }}
        code {{ font-family: 'Monaco', 'Consolas', monospace; background: #f0f0f0; padding: 2px 4px; border-radius: 2px; }}
        .config-table {{ width: 100%; border-collapse: collapse; margin: 15px 0; }}
        .config-table th, .config-table td {{ border: 1px solid #ddd; padding: 12px; text-align: left; }}
        .config-table th {{ background: #f5f5f5; font-weight: bold; }}
    </style>
</head>
<body>
    <div class="content">
        <div class="header">
            <h1>{} - Configuration</h1>
            <p>Project configuration reference</p>
        </div>

        <div class="nav">
            <a href="index.html">Overview</a>
            <a href="api.html">API Docs</a>
            <a href="config.html" class="active">Configuration</a>
        </div>

        <div class="section">
            <h2>Configuration Layout</h2>
            <p><code>Actr.toml</code> is the main configuration file for the project.</p>

            <table class="config-table">
                <tr>
                    <th>Key</th>
                    <th>Purpose</th>
                    <th>Notes</th>
                </tr>
                <tr>
                    <td><code>edition</code></td>
                    <td>Config format version</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>inherit</code></td>
                    <td>Parent config file path</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>exports</code></td>
                    <td>Exported proto files for service specs</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[package]</code></td>
                    <td>Package metadata (name, description, authors, license, tags)</td>
                    <td>Required</td>
                </tr>
                <tr>
                    <td><code>[package.actr_type]</code></td>
                    <td>Actor type definition (manufacturer, name)</td>
                    <td>Required</td>
                </tr>
                <tr>
                    <td><code>[dependencies]</code></td>
                    <td>Dependency map (empty or fingerprinted entries)</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[system.signaling]</code></td>
                    <td>Signaling server configuration</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[system.deployment]</code></td>
                    <td>Deployment configuration</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[system.discovery]</code></td>
                    <td>Discovery configuration</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[system.storage]</code></td>
                    <td>Storage configuration (mailbox path)</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[system.webrtc]</code></td>
                    <td>WebRTC configuration (STUN/TURN/relay)</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[system.observability]</code></td>
                    <td>Tracing and logging configuration</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[acl]</code> / <code>[[acl.rules]]</code></td>
                    <td>Access control rules</td>
                    <td>Optional</td>
                </tr>
                <tr>
                    <td><code>[scripts]</code></td>
                    <td>Custom script commands</td>
                    <td>Optional</td>
                </tr>
            </table>
        </div>

        <div class="section">
            <h2>Example</h2>
            <pre><code>{}</code></pre>
        </div>

        <div class="section">
            <h2>Managing Dependencies</h2>
            <p>Use the install command to add or install dependencies:</p>
            <pre><code># Add a dependency and update Actr.toml
actr install user-service

# Install dependencies listed in Actr.toml
actr install</code></pre>
        </div>

        <div class="section">
            <h2>Dependency Formats</h2>
            <p>Define Protocol Buffers dependencies under <code>[dependencies]</code>:</p>
            <pre><code># Local file path
user_service = "protos/user.proto"

# HTTP URL
api_service = "https://example.com/api/service.proto"

# Actor registry
[dependencies.payment]
name = "payment-service"
actr_type = "payment"
fingerprint = "sha256:a1b2c3d4..."</code></pre>
        </div>
    </div>
</body>
</html>"#,
            project_name,
            project_name,
            Self::html_escape(&config_example)
        );

        let config_path = Path::new(output_dir).join("config.html");
        std::fs::write(config_path, html_content)?;

        Ok(())
    }

    /// Simple HTML escape function
    fn html_escape(text: &str) -> String {
        text.replace("&", "&amp;")
            .replace("<", "&lt;")
            .replace(">", "&gt;")
            .replace("\"", "&quot;")
            .replace("'", "&#x27;")
    }

    fn read_cargo_version() -> Option<String> {
        let cargo_toml = std::fs::read_to_string("Cargo.toml").ok()?;
        let value: toml::Value = cargo_toml.parse().ok()?;
        value
            .get("package")
            .and_then(|package| package.get("version"))
            .and_then(|version| version.as_str())
            .map(|version| version.to_string())
    }
}
