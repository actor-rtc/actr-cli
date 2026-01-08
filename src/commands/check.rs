//! Check command implementation - verify Actor-RTC service availability
//!
//! The check command validates that services are available in the registry
//! and optionally verifies they match the configured dependencies.

use crate::core::{
    ActrCliError, AvailabilityStatus, Command, CommandContext, CommandResult, ComponentType,
    ServiceDiscovery,
};
use anyhow::Result;
use async_trait::async_trait;
use clap::Args;
use std::sync::Arc;
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

        // Get ServiceDiscovery component
        let service_discovery = {
            let container = context.container.lock().unwrap();
            container.get_service_discovery()?
        };

        // Determine which service packages to check
        let packages_to_check = if self.packages.is_empty() {
            // Load service names from configuration file
            info!("🔍 Loading services from configuration: {}", config_path);
            self.load_packages_from_config(config_path)?
        } else {
            // Use provided service names
            info!("🔍 Checking provided services");
            self.packages.clone()
        };

        if packages_to_check.is_empty() {
            info!("ℹ️ No services to check");
            return Ok(CommandResult::Success("No services to check".to_string()));
        }

        info!("📦 Checking {} services...", packages_to_check.len());

        let mut total_checked = 0;
        let mut available_count = 0;
        let mut unavailable_count = 0;
        let mut results: Vec<(String, ServiceCheckResult)> = Vec::new();

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
                    }
                }
                Err(e) => {
                    results.push((package.clone(), ServiceCheckResult::Error(e.to_string())));
                    unavailable_count += 1;
                }
            }
        }

        // Summary
        info!("");
        info!("📊 Service Check Summary:");
        info!("   Total checked: {}", total_checked);
        info!("   ✅ Available: {}", available_count);
        info!("   ❌ Unavailable: {}", unavailable_count);

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
        }

        if unavailable_count > 0 {
            error!("⚠️ {} services are not available", unavailable_count);
            return Err(ActrCliError::Dependency {
                message: format!("{} services are unavailable", unavailable_count),
            }
            .into());
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
    /// Load service names from configuration file
    fn load_packages_from_config(&self, config_path: &str) -> Result<Vec<String>> {
        use actr_config::ConfigParser;

        // Load configuration
        let config = ConfigParser::from_file(config_path).map_err(|e| ActrCliError::Config {
            message: format!("Failed to load config: {e}"),
        })?;

        let mut packages = Vec::new();

        // Extract service names from dependencies
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
                config_path
            );
        } else {
            info!("📋 Found {} services in configuration", packages.len());
        }

        Ok(packages)
    }

    /// Check service availability using ServiceDiscovery
    async fn check_service(
        &self,
        service_name: &str,
        service_discovery: &Arc<dyn ServiceDiscovery>,
    ) -> Result<AvailabilityStatus> {
        debug!("Checking service availability: {}", service_name);

        // Use ServiceDiscovery to check availability
        let status = service_discovery
            .check_service_availability(service_name)
            .await?;

        Ok(status)
    }
}

/// Result of a service check
enum ServiceCheckResult {
    Available(AvailabilityStatus),
    Error(String),
}
