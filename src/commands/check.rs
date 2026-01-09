//! Check command implementation - verify Actor-RTC service availability
//!
//! The check command validates that services are available in the registry
//! and optionally verifies they match the configured dependencies.

use crate::core::{
    ActrCliError, AvailabilityStatus, Command, CommandContext, CommandResult, ComponentType,
    ConnectivityStatus, HealthStatus, NetworkServiceDiscovery, ServiceDiscovery,
};
use actr_config::{Config, ConfigParser, LockFile};
use anyhow::Result;
use async_trait::async_trait;
use clap::Args;
use std::collections::{HashMap, HashSet};
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
        let (network_validator, fingerprint_validator) = {
            let container = context.container.lock().unwrap();
            (
                container.get_network_validator()?,
                container.get_fingerprint_validator()?,
            )
        };

        // Use loaded_config directly if available, otherwise load from file if it exists
        // Load config if not already loaded and file exists
        if loaded_config.is_none() && config_path.exists() {
            let config = self.load_config(&config_path)?;
            loaded_config = Some(config);
        }
        let fingerprint_config = loaded_config.as_ref();
        let expected_fingerprints = self.collect_expected_fingerprints(fingerprint_config);

        let lock_file = if self.lock {
            info!("🔒 Checking Actr.lock.toml");
            Some(self.load_lock_file(&config_path)?)
        } else {
            None
        };
        let lock_entries = lock_file
            .as_ref()
            .map(|lock| {
                lock.dependencies
                    .iter()
                    .cloned()
                    .map(|dep| (dep.name.clone(), dep))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();

        info!("📦 Checking {} services...", packages_to_check.len());

        let mut total_checked = 0;
        let mut available_count = 0;
        let mut unavailable_count = 0;
        let mut network_failures = 0;
        let mut fingerprint_mismatches = 0;
        let mut lock_mismatches = 0;
        let mut missing_in_lock: Vec<String> = Vec::new();
        let mut results: Vec<ServiceCheckReport> = Vec::new();
        let mut problem_services: HashSet<String> = HashSet::new();

        for package in &packages_to_check {
            total_checked += 1;
            let expected_fingerprint = expected_fingerprints.get(package).cloned();
            let lock_entry = lock_entries.get(package);

            let mut report = ServiceCheckReport::new(package.clone());
            report.fingerprint_expected = expected_fingerprint.clone();

            let check_result = self
                .check_service(package.as_str(), &service_discovery)
                .await;
            match check_result {
                Ok(status) => {
                    report.availability = Some(status.clone());
                    if status.is_available {
                        available_count += 1;
                    } else {
                        unavailable_count += 1;
                        problem_services.insert(package.clone());
                    }
                }
                Err(e) => {
                    report.availability_error = Some(e.to_string());
                    unavailable_count += 1;
                    problem_services.insert(package.clone());
                }
            }

            if report.is_available() {
                report.connectivity_checked = true;
                match network_validator.check_connectivity(package).await {
                    Ok(connectivity) => {
                        if !connectivity.is_reachable {
                            network_failures += 1;
                            problem_services.insert(package.clone());
                        }
                        report.connectivity = Some(connectivity);
                    }
                    Err(e) => {
                        network_failures += 1;
                        problem_services.insert(package.clone());
                        report.connectivity_error = Some(e.to_string());
                    }
                }
            }

            // Fetch fingerprint if verbose, expected fingerprint exists, or lock check is enabled
            let should_fetch_fingerprint =
                self.verbose || report.fingerprint_expected.is_some() || self.lock;
            if should_fetch_fingerprint && report.is_available() {
                report.fingerprint_checked = true;
                match service_discovery.get_service_details(package).await {
                    Ok(details) => match fingerprint_validator
                        .compute_service_fingerprint(&details.info)
                        .await
                    {
                        Ok(actual) => {
                            report.fingerprint_actual = Some(actual.value);
                        }
                        Err(e) => {
                            report.fingerprint_error = Some(e.to_string());
                        }
                    },
                    Err(e) => {
                        report.fingerprint_error = Some(e.to_string());
                    }
                }
            }

            if let (Some(expected), Some(actual)) = (
                report.fingerprint_expected.as_deref(),
                report.fingerprint_actual.as_deref(),
            ) {
                let matched = expected == actual;
                report.fingerprint_match = Some(matched);
                if !matched {
                    fingerprint_mismatches += 1;
                    problem_services.insert(package.clone());
                }
            }

            if self.lock {
                report.lock_detail.checked = true;
                if let Some(lock_entry) = lock_entry {
                    report.lock_detail.present = true;
                    report.lock_detail.fingerprint = Some(lock_entry.fingerprint.clone());
                    if let Some(actual) = report.fingerprint_actual.as_deref() {
                        let matched = lock_entry.fingerprint == actual;
                        report.lock_detail.is_match = Some(matched);
                        if !matched {
                            lock_mismatches += 1;
                            problem_services.insert(package.clone());
                        }
                    }
                } else {
                    missing_in_lock.push(package.clone());
                    problem_services.insert(package.clone());
                }
            }

            results.push(report);
        }

        // Summary
        info!("");
        info!("📊 Service Check Summary:");
        info!("   Total checked: {}", total_checked);
        info!("   ✅ Available: {}", available_count);
        info!("   ❌ Unavailable: {}", unavailable_count);
        info!("   🌐 Network failures: {}", network_failures);
        info!("   🔐 Fingerprint mismatches: {}", fingerprint_mismatches);
        if self.lock {
            info!("   🔒 Missing in Actr.lock.toml: {}", missing_in_lock.len());
            info!("   🔒 Lock mismatches: {}", lock_mismatches);
        }

        // Detailed output if verbose
        if self.verbose {
            info!("");
            info!("📋 Detailed Results:");
            for report in &results {
                info!("   🔎 [{}]", report.name);
                match (&report.availability, &report.availability_error) {
                    (Some(status), _) => {
                        if status.is_available {
                            info!("      Availability: available");
                        } else {
                            info!("      Availability: unavailable");
                        }
                        info!("      Health: {}", format_health(&status.health));
                    }
                    (None, Some(error_msg)) => {
                        info!("      Availability: error ({})", error_msg);
                    }
                    _ => {
                        info!("      Availability: unknown");
                    }
                }

                if report.connectivity_checked {
                    if let Some(connectivity) = &report.connectivity {
                        let latency = connectivity
                            .response_time_ms
                            .map(|value| format!("{value}ms"))
                            .unwrap_or_else(|| "unknown".to_string());
                        let reachability = if connectivity.is_reachable {
                            "reachable"
                        } else {
                            "unreachable"
                        };
                        info!(
                            "      Connectivity: {} (latency: {})",
                            reachability, latency
                        );
                        if let Some(error) = connectivity.error.as_deref() {
                            info!("      Connectivity error: {}", error);
                        }
                    } else if let Some(error) = report.connectivity_error.as_deref() {
                        info!("      Connectivity: error ({})", error);
                    } else {
                        info!("      Connectivity: unknown");
                    }
                } else {
                    info!("      Connectivity: skipped");
                }

                if report.fingerprint_checked || report.fingerprint_expected.is_some() {
                    let expected = report.fingerprint_expected.as_deref().unwrap_or("-");
                    let actual = report.fingerprint_actual.as_deref().unwrap_or("-");
                    let matched = match report.fingerprint_match {
                        Some(true) => "match",
                        Some(false) => "mismatch",
                        None => "unknown",
                    };
                    info!(
                        "      Fingerprint: expected={} actual={} match={}",
                        expected, actual, matched
                    );
                    if let Some(error) = report.fingerprint_error.as_deref() {
                        info!("      Fingerprint error: {}", error);
                    }
                } else {
                    info!("      Fingerprint: skipped");
                }

                info!("      Lock: {}", format_lock_detail(&report.lock_detail));
            }
        }

        if !problem_services.is_empty() {
            let mut problems = Vec::new();
            if unavailable_count > 0 {
                problems.push(format!("{} services are unavailable", unavailable_count));
            }
            if network_failures > 0 {
                problems.push(format!(
                    "{} services failed network checks",
                    network_failures
                ));
            }
            if fingerprint_mismatches > 0 {
                problems.push(format!(
                    "{} services failed fingerprint checks",
                    fingerprint_mismatches
                ));
            }
            if self.lock && !missing_in_lock.is_empty() {
                problems.push(format!(
                    "{} services are missing from Actr.lock.toml",
                    missing_in_lock.len()
                ));
            }
            if self.lock && lock_mismatches > 0 {
                problems.push(format!("{} services failed lock checks", lock_mismatches));
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
            info!("🎉 All services passed checks and match Actr.lock.toml!");
        } else {
            info!("🎉 All services passed availability checks!");
        }

        Ok(CommandResult::Success(format!(
            "Checked {} services, all available",
            total_checked
        )))
    }

    fn required_components(&self) -> Vec<ComponentType> {
        vec![
            ComponentType::ServiceDiscovery,
            ComponentType::NetworkValidator,
            ComponentType::FingerprintValidator,
        ]
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

    fn collect_expected_fingerprints(&self, config: Option<&Config>) -> HashMap<String, String> {
        let mut expected = HashMap::new();
        if let Some(config) = config {
            for dependency in &config.dependencies {
                if let Some(fingerprint) = dependency.fingerprint.as_ref()
                    && !fingerprint.is_empty()
                {
                    expected.insert(dependency.name.clone(), fingerprint.clone());
                }
            }
        }
        expected
    }

    fn load_lock_file(&self, config_path: &Path) -> Result<LockFile> {
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
        Ok(lock_file)
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

struct ServiceCheckReport {
    name: String,
    availability: Option<AvailabilityStatus>,
    availability_error: Option<String>,
    connectivity_checked: bool,
    connectivity: Option<ConnectivityStatus>,
    connectivity_error: Option<String>,
    fingerprint_checked: bool,
    fingerprint_expected: Option<String>,
    fingerprint_actual: Option<String>,
    fingerprint_match: Option<bool>,
    fingerprint_error: Option<String>,
    lock_detail: LockCheckDetail,
}

impl ServiceCheckReport {
    fn new(name: String) -> Self {
        Self {
            name,
            availability: None,
            availability_error: None,
            connectivity_checked: false,
            connectivity: None,
            connectivity_error: None,
            fingerprint_checked: false,
            fingerprint_expected: None,
            fingerprint_actual: None,
            fingerprint_match: None,
            fingerprint_error: None,
            lock_detail: LockCheckDetail::skipped(),
        }
    }

    fn is_available(&self) -> bool {
        self.availability
            .as_ref()
            .map(|status| status.is_available)
            .unwrap_or(false)
    }
}

struct LockCheckDetail {
    checked: bool,
    present: bool,
    fingerprint: Option<String>,
    is_match: Option<bool>,
    error: Option<String>,
}

impl LockCheckDetail {
    fn skipped() -> Self {
        Self {
            checked: false,
            present: false,
            fingerprint: None,
            is_match: None,
            error: None,
        }
    }
}

fn format_health(health: &HealthStatus) -> &'static str {
    match health {
        HealthStatus::Healthy => "healthy",
        HealthStatus::Degraded => "degraded",
        HealthStatus::Unhealthy => "unhealthy",
        HealthStatus::Unknown => "unknown",
    }
}

fn format_lock_detail(detail: &LockCheckDetail) -> String {
    if !detail.checked {
        return "skipped".to_string();
    }
    if !detail.present {
        return "missing".to_string();
    }

    let fingerprint = detail.fingerprint.as_deref().unwrap_or("-");
    let matched = match detail.is_match {
        Some(true) => "match",
        Some(false) => "mismatch",
        None => "unknown",
    };

    if let Some(error) = detail.error.as_deref() {
        format!(
            "present (fingerprint={}, match={}, error={})",
            fingerprint, matched, error
        )
    } else {
        format!("present (fingerprint={}, match={})", fingerprint, matched)
    }
}
