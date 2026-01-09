//! Check command implementation - verify Actor-RTC service availability
//!
//! The check command validates that services are available in the registry
//! and optionally verifies they match the configured dependencies.

use crate::core::{
    ActrCliError, AvailabilityStatus, Command, CommandContext, CommandResult, ComponentType,
    NetworkServiceDiscovery, ServiceDiscovery,
};
use actr_config::{Config, ConfigParser, LockFile};
use anyhow::Result;
use async_trait::async_trait;
use clap::Args;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

/// Check command - validates service availability
#[derive(Args, Debug)]
#[command(
    about = "Validate project dependencies",
    long_about = "Validate that services are available in the registry and match the configured dependencies"
)]
pub struct CheckCommand {
    /// Service names to check (e.g., "user-service", "order-service")
    /// If not provided, checks all services from the configuration file
    #[arg(value_name = "SERVICE_NAME")]
    pub packages: Vec<String>,

    /// Configuration file to load services from (defaults to Actr.toml)
    #[arg(short = 'f', long = "file")]
    pub config_file: Option<String>,

    /// Show detailed connection information
    #[arg(short, long)]
    pub verbose: bool,

    /// Timeout for each service check in seconds
    #[arg(long, default_value = "10")]
    pub timeout: u64,

    /// Also verify services are installed in Actr.lock.toml
    #[arg(long)]
    pub lock: bool,
}

#[async_trait]
impl Command for CheckCommand {
    async fn execute(&self, context: &CommandContext) -> Result<CommandResult> {
        let config_path = self.config_file.as_deref().unwrap_or("Actr.toml");
        let config_path = self.resolve_config_path(context, config_path);
        let mut loaded_config: Option<Config> = None;

        // Determine which service packages to check
        let packages_to_check = if self.packages.is_empty() {
            info!(
                "🔍 Loading services from configuration: {}",
                config_path.display()
            );
            let config = self.load_config(&config_path)?;
            let packages = self.load_packages_from_config(&config, &config_path);
            loaded_config = Some(config);
            packages
        } else {
            info!("🔍 Checking provided services");
            self.packages.clone()
        };

        if packages_to_check.is_empty() {
            info!("ℹ️ No services to check");
            return Ok(CommandResult::Success("No services to check".to_string()));
        }

        let service_discovery =
            self.resolve_service_discovery(context, &config_path, loaded_config.as_ref())?;

        info!("📦 Checking {} services...", packages_to_check.len());

        let mut total_checked = 0;
        let mut available_count = 0;
        let mut unavailable_count = 0;
        let mut results: Vec<(String, ServiceCheckResult)> = Vec::new();
        let mut problem_services: HashSet<String> = HashSet::new();

        for package in &packages_to_check {
            total_checked += 1;
            let check_result = self
                .check_service(package.as_str(), &service_discovery)
                .await;

            match check_result {
                Ok(status) => {
                    let is_available = status.is_available;
                    results.push((package.clone(), ServiceCheckResult::Available(status)));

                    if is_available {
                        available_count += 1;
                    } else {
                        unavailable_count += 1;
                        problem_services.insert(package.clone());
                    }
                }
                Err(e) => {
                    results.push((package.clone(), ServiceCheckResult::Error(e.to_string())));
                    unavailable_count += 1;
                    problem_services.insert(package.clone());
                }
            }
        }

        let missing_in_lock = if self.lock {
            info!("🔒 Checking Actr.lock.toml");
            self.check_lock_file(&packages_to_check, &config_path)?
        } else {
            Vec::new()
        };

        for name in &missing_in_lock {
            problem_services.insert(name.clone());
        }

        // Summary
        info!("");
        info!("📊 Service Check Summary:");
        info!("   Total checked: {}", total_checked);
        info!("   ✅ Available: {}", available_count);
        info!("   ❌ Unavailable: {}", unavailable_count);
        if self.lock {
            info!("   🔒 Missing in Actr.lock.toml: {}", missing_in_lock.len());
        }

        // Detailed output if verbose
        if self.verbose {
            info!("");
            info!("📋 Detailed Results:");
            for (name, result) in &results {
                match result {
                    ServiceCheckResult::Available(status) => {
                        if status.is_available {
                            info!("   ✅ [{}] Available", name);
                        } else {
                            info!("   ❌ [{}] Not found in registry", name);
                        }
                    }
                    ServiceCheckResult::Error(error_msg) => {
                        info!("   ❌ [{}] Error: {}", name, error_msg);
                    }
                }
            }
            if self.lock && !missing_in_lock.is_empty() {
                for name in &missing_in_lock {
                    info!("   🔒 [{}] Missing from Actr.lock.toml", name);
                }
            }
        }

        if !problem_services.is_empty() {
            let mut problems = Vec::new();
            if unavailable_count > 0 {
                problems.push(format!("{} services are unavailable", unavailable_count));
            }
            if self.lock && !missing_in_lock.is_empty() {
                problems.push(format!(
                    "{} services are missing from Actr.lock.toml",
                    missing_in_lock.len()
                ));
            }
            let message = if problems.is_empty() {
                "Service checks failed".to_string()
            } else {
                problems.join(", ")
            };
            error!("⚠️ {message}");
            return Err(ActrCliError::Dependency { message }.into());
        }

        if self.lock {
            info!("🎉 All services are available and present in Actr.lock.toml!");
        } else {
            info!("🎉 All services are available and accessible!");
        }

        Ok(CommandResult::Success(format!(
            "Checked {} services, all available",
            total_checked
        )))
    }

    fn required_components(&self) -> Vec<ComponentType> {
        vec![ComponentType::ServiceDiscovery]
    }

    fn name(&self) -> &str {
        "check"
    }

    fn description(&self) -> &str {
        "Validate that services are available in the registry"
    }
}

impl CheckCommand {
    fn resolve_config_path(&self, context: &CommandContext, config_path: &str) -> PathBuf {
        let path = Path::new(config_path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            context.working_dir.join(path)
        }
    }

    fn load_config(&self, config_path: &Path) -> Result<Config> {
        Ok(
            ConfigParser::from_file(config_path).map_err(|e| ActrCliError::Config {
                message: format!("Failed to load config {}: {e}", config_path.display()),
            })?,
        )
    }

    /// Load service names from configuration file
    fn load_packages_from_config(&self, config: &Config, config_path: &Path) -> Vec<String> {
        let mut packages = Vec::new();

        for dependency in &config.dependencies {
            packages.push(dependency.name.clone());
            debug!(
                "Added service: {} (alias: {})",
                dependency.name, dependency.alias
            );
        }

        if packages.is_empty() {
            info!(
                "ℹ️ No dependencies found in configuration file: {}",
                config_path.display()
            );
        } else {
            info!("📋 Found {} services in configuration", packages.len());
        }

        packages
    }

    fn resolve_service_discovery(
        &self,
        context: &CommandContext,
        config_path: &Path,
        config: Option<&Config>,
    ) -> Result<Arc<dyn ServiceDiscovery>> {
        if self.config_file.is_some() {
            let config = match config {
                Some(config) => config.clone(),
                None => self.load_config(config_path)?,
            };
            return Ok(Arc::new(NetworkServiceDiscovery::new(config)));
        }

        let container = context.container.lock().unwrap();
        match container.get_service_discovery() {
            Ok(service_discovery) => Ok(service_discovery),
            Err(_) => {
                let config = match config {
                    Some(config) => config.clone(),
                    None => self.load_config(config_path)?,
                };
                Ok(Arc::new(NetworkServiceDiscovery::new(config)))
            }
        }
    }

    fn check_lock_file(&self, packages: &[String], config_path: &Path) -> Result<Vec<String>> {
        let lock_file_path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("Actr.lock.toml");

        if !lock_file_path.exists() {
            return Err(ActrCliError::Config {
                message: format!("Lock file not found: {}", lock_file_path.display()),
            }
            .into());
        }

        let lock_file = LockFile::from_file(&lock_file_path).map_err(|e| ActrCliError::Config {
            message: format!("Failed to load lock file {}: {e}", lock_file_path.display()),
        })?;

        let mut missing = Vec::new();
        let mut seen = HashSet::new();

        for package in packages {
            if !seen.insert(package) {
                continue;
            }
            if lock_file.get_dependency(package).is_none() {
                missing.push(package.clone());
            }
        }

        Ok(missing)
    }

    /// Check service availability using ServiceDiscovery
    async fn check_service(
        &self,
        service_name: &str,
        service_discovery: &Arc<dyn ServiceDiscovery>,
    ) -> Result<AvailabilityStatus> {
        debug!("Checking service availability: {}", service_name);

        let status = if self.timeout == 0 {
            service_discovery
                .check_service_availability(service_name)
                .await?
        } else {
            let duration = Duration::from_secs(self.timeout);
            match tokio::time::timeout(
                duration,
                service_discovery.check_service_availability(service_name),
            )
            .await
            {
                Ok(result) => result?,
                Err(_) => {
                    return Err(ActrCliError::Network {
                        message: format!(
                            "Timeout after {}s while checking {}",
                            self.timeout, service_name
                        ),
                    }
                    .into());
                }
            }
        };

        Ok(status)
    }
}

/// Result of a service check
enum ServiceCheckResult {
    Available(AvailabilityStatus),
    Error(String),
}
