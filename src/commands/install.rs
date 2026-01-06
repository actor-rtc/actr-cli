//! Install Command Implementation
//!
//! Implement install flow based on reuse architecture with check-first principle

use anyhow::Result;
use async_trait::async_trait;
use clap::Args;

use crate::core::{
    ActrCliError, Command, CommandContext, CommandResult, ComponentType, DependencySpec,
    ErrorReporter, InstallResult,
};

/// Install command
#[derive(Args, Debug)]
#[command(
    about = "Install service dependencies",
    long_about = "Install service dependencies. You can install specific service packages, or install all dependencies configured in Actr.toml"
)]
pub struct InstallCommand {
    /// List of service packages to install (e.g., actr://user-service@1.0.0/)
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Force reinstallation
    #[arg(long)]
    pub force: bool,

    /// Force update of all dependencies
    #[arg(long)]
    pub force_update: bool,

    /// Skip fingerprint verification
    #[arg(long)]
    pub skip_verification: bool,
}

#[async_trait]
impl Command for InstallCommand {
    async fn execute(&self, context: &CommandContext) -> Result<CommandResult> {
        // Check-First principle: validate project state first
        if !self.is_actr_project() {
            return Err(ActrCliError::InvalidProject {
                message: "Not an Actor-RTC project. Run 'actr init' to initialize.".to_string(),
            }
            .into());
        }

        // Determine installation mode
        let dependency_specs = if !self.packages.is_empty() {
            // Mode 1: Add new dependency (npm install <package>)
            println!("📦 Adding {} new service dependencies", self.packages.len());
            self.parse_new_packages()?
        } else {
            // Mode 2: Install dependencies in config (npm install)
            if self.force_update {
                println!("📦 Force updating all service dependencies in configuration");
            } else {
                println!("📦 Installing service dependencies in configuration");
            }
            self.load_dependencies_from_config(context).await?
        };

        if dependency_specs.is_empty() {
            println!("ℹ️ No dependencies to install");
            return Ok(CommandResult::Success(
                "No dependencies to install".to_string(),
            ));
        }

        // Get install pipeline (automatically includes ValidationPipeline)
        let install_pipeline = {
            let mut container = context.container.lock().unwrap();
            container.get_install_pipeline()?
        };

        // Execute check-first install flow
        match install_pipeline
            .install_dependencies(&dependency_specs)
            .await
        {
            Ok(install_result) => {
                self.display_install_success(&install_result);
                Ok(CommandResult::Install(install_result))
            }
            Err(e) => {
                // User-friendly error display
                let cli_error = ActrCliError::InstallFailed {
                    reason: e.to_string(),
                };
                eprintln!("{}", ErrorReporter::format_error(&cli_error));
                Err(e)
            }
        }
    }

    fn required_components(&self) -> Vec<ComponentType> {
        // Install command needs complete install pipeline components
        vec![
            ComponentType::ConfigManager,
            ComponentType::DependencyResolver,
            ComponentType::ServiceDiscovery,
            ComponentType::NetworkValidator,
            ComponentType::FingerprintValidator,
            ComponentType::ProtoProcessor,
            ComponentType::CacheManager,
        ]
    }

    fn name(&self) -> &str {
        "install"
    }

    fn description(&self) -> &str {
        "npm-style service-level dependency management (check-first architecture)"
    }
}

impl InstallCommand {
    pub fn new(
        packages: Vec<String>,
        force: bool,
        force_update: bool,
        skip_verification: bool,
    ) -> Self {
        Self {
            packages,
            force,
            force_update,
            skip_verification,
        }
    }

    // Create from clap Args
    pub fn from_args(args: &InstallCommand) -> Self {
        InstallCommand {
            packages: args.packages.clone(),
            force: args.force,
            force_update: args.force_update,
            skip_verification: args.skip_verification,
        }
    }

    /// Check if in Actor-RTC project
    fn is_actr_project(&self) -> bool {
        std::path::Path::new("Actr.toml").exists()
    }

    /// Parse new package specs
    fn parse_new_packages(&self) -> Result<Vec<DependencySpec>> {
        let mut specs = Vec::new();

        for package_spec in &self.packages {
            let spec = self.parse_package_spec(package_spec)?;
            specs.push(spec);
        }

        Ok(specs)
    }

    /// Parse single package spec
    fn parse_package_spec(&self, package_spec: &str) -> Result<DependencySpec> {
        if package_spec.starts_with("actr://") {
            // Direct actr:// URI
            self.parse_actr_uri(package_spec)
        } else if package_spec.contains('@') {
            // service-name@version format
            self.parse_versioned_spec(package_spec)
        } else {
            // Simple service name
            self.parse_simple_spec(package_spec)
        }
    }

    /// Parse actr:// URI
    fn parse_actr_uri(&self, uri: &str) -> Result<DependencySpec> {
        // Simplified URI parsing, actual implementation should be more strict
        if !uri.starts_with("actr://") {
            return Err(anyhow::anyhow!("Invalid actr:// URI: {uri}"));
        }

        let uri_part = &uri[7..]; // Remove "actr://"
        let service_name = if let Some(pos) = uri_part.find('/') {
            uri_part[..pos].to_string()
        } else {
            uri_part.to_string()
        };

        // Extract query parameters (simplified version)
        let fingerprint = if uri.contains('?') {
            self.parse_query_params(uri)?
        } else {
            None
        };

        Ok(DependencySpec {
            name: service_name,
            uri: uri.to_string(),
            fingerprint,
        })
    }

    /// Parse query parameters
    fn parse_query_params(&self, uri: &str) -> Result<Option<String>> {
        if let Some(query_start) = uri.find('?') {
            let query = &uri[query_start + 1..];
            let mut fingerprint = None;

            for param in query.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    match key {
                        "fingerprint" => fingerprint = Some(value.to_string()),
                        _ => {} // Ignore unknown parameters
                    }
                }
            }

            Ok(fingerprint)
        } else {
            Ok(None)
        }
    }

    /// Parse versioned spec (service@tag)
    fn parse_versioned_spec(&self, spec: &str) -> Result<DependencySpec> {
        let parts: Vec<&str> = spec.split('@').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid package specification: {spec}. Use 'service-name@tag'"
            ));
        }

        let service_name = parts[0].to_string();
        let _tag = parts[1].to_string();
        let uri = format!("actr://{service_name}/");

        Ok(DependencySpec {
            name: service_name,
            uri,
            fingerprint: None,
        })
    }

    /// Parse simple spec (service-name)
    fn parse_simple_spec(&self, spec: &str) -> Result<DependencySpec> {
        let service_name = spec.to_string();
        let uri = format!("actr://{service_name}/");

        Ok(DependencySpec {
            name: service_name,
            uri,
            fingerprint: None,
        })
    }

    /// Load dependencies from config file
    async fn load_dependencies_from_config(
        &self,
        context: &CommandContext,
    ) -> Result<Vec<DependencySpec>> {
        let config_manager = {
            let container = context.container.lock().unwrap();
            container.get_config_manager()?
        };
        let config = config_manager
            .load_config(
                config_manager
                    .get_project_root()
                    .join("Actr.toml")
                    .as_path(),
            )
            .await?;

        let mut specs = Vec::new();

        for dependency in &config.dependencies {
            let uri = format!(
                "actr://{}:{}+{}@v1/",
                dependency.realm.realm_id,
                dependency.actr_type.manufacturer,
                dependency.actr_type.name
            );
            specs.push(DependencySpec {
                name: dependency.alias.clone(),
                uri,
                fingerprint: dependency.fingerprint.clone(),
            });
        }

        Ok(specs)
    }

    /// Display install success information
    fn display_install_success(&self, result: &InstallResult) {
        println!();
        println!("✅ Installation successful!");
        println!(
            "   📦 Installed dependencies: {}",
            result.installed_dependencies.len()
        );
        println!("   🗂️  Cache updates: {}", result.cache_updates);

        if result.updated_config {
            println!("   📝 Configuration file updated");
        }

        if result.updated_lock_file {
            println!("   🔒 Lock file updated");
        }

        if !result.warnings.is_empty() {
            println!();
            println!("⚠️  Warnings:");
            for warning in &result.warnings {
                println!("   • {warning}");
            }
        }

        println!();
        println!("💡 Tip: Run 'actr gen' to generate the latest code");
    }
}

impl Default for InstallCommand {
    fn default() -> Self {
        Self::new(Vec::new(), false, false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_spec() {
        let cmd = InstallCommand::default();
        let spec = cmd.parse_simple_spec("user-service").unwrap();

        assert_eq!(spec.name, "user-service");
        assert_eq!(spec.uri, "actr://user-service/");
        assert_eq!(spec.fingerprint, None);
    }

    #[test]
    fn test_parse_versioned_spec() {
        let cmd = InstallCommand::default();
        let spec = cmd.parse_versioned_spec("user-service@1.2.0").unwrap();

        assert_eq!(spec.name, "user-service");
        assert_eq!(spec.uri, "actr://user-service/");
        assert_eq!(spec.fingerprint, None);
    }

    #[test]
    fn test_parse_actr_uri_simple() {
        let cmd = InstallCommand::default();
        let spec = cmd.parse_actr_uri("actr://user-service/").unwrap();

        assert_eq!(spec.name, "user-service");
        assert_eq!(spec.uri, "actr://user-service/");
        assert_eq!(spec.fingerprint, None);
    }

    #[test]
    fn test_parse_actr_uri_with_params() {
        let cmd = InstallCommand::default();
        let spec = cmd
            .parse_actr_uri("actr://user-service/?version=1.2.0&fingerprint=sha256:abc123")
            .unwrap();

        assert_eq!(spec.name, "user-service");
        assert_eq!(
            spec.uri,
            "actr://user-service/?version=1.2.0&fingerprint=sha256:abc123"
        );
        assert_eq!(spec.fingerprint, Some("sha256:abc123".to_string()));
    }
}
