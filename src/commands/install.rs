//! Install Command Implementation
//!
//! Implement install flow based on reuse architecture with check-first principle

use crate::core::{
    ActrCliError, Command, CommandContext, CommandResult, ComponentType, DependencySpec,
    ErrorReporter, InstallResult,
};
use actr_config::LockFile;
use actr_protocol::{ActrType, ActrTypeExt};
use actr_version::{CompatibilityLevel, Fingerprint, ProtoFile, ServiceCompatibility};
use anyhow::Result;
use async_trait::async_trait;
use clap::Args;

/// Install command
#[derive(Args, Debug)]
#[command(
    about = "Install service dependencies",
    long_about = "Install service dependencies. You can install specific service packages, or install all dependencies configured in Actr.toml.\n\nExamples:\n  actr install                          # Install all dependencies from Actr.toml\n  actr install user-service             # Install a service by name\n  actr install my-alias --actr-type acme+EchoService  # Install with alias and explicit actr_type"
)]
pub struct InstallCommand {
    /// Package name or alias (when used with --actr-type, this becomes the alias)
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Actor type for the dependency (format: manufacturer+name, e.g., acme+EchoService).
    /// When specified, the PACKAGE argument is treated as an alias.
    #[arg(long, value_name = "TYPE")]
    pub actr_type: Option<String>,

    /// Service fingerprint for version pinning
    #[arg(long, value_name = "FINGERPRINT")]
    pub fingerprint: Option<String>,

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

/// Installation mode
#[derive(Debug, Clone)]
pub enum InstallMode {
    /// Mode 1: Add new dependency (npm install <package>)
    /// - Pull remote proto to protos/ folder
    /// - Modify Actr.toml (add dependency)
    /// - Update Actr.lock.toml
    AddNewPackage { packages: Vec<String> },

    /// Mode 1b: Add dependency with explicit alias and actr_type (actr install <alias> --actr-type <type>)
    /// - Discover service by actr_type
    /// - Use first argument as alias
    /// - Modify Actr.toml (add dependency with alias)
    /// - Update Actr.lock.toml
    AddWithAlias {
        alias: String,
        actr_type: ActrType,
        fingerprint: Option<String>,
    },

    /// Mode 2: Install dependencies in config (npm install)
    /// - Do NOT modify Actr.toml
    /// - Use lock file versions if available
    /// - Only update Actr.lock.toml
    InstallFromConfig { force_update: bool },
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
        let mode = if let Some(actr_type_str) = &self.actr_type {
            // Mode 1b: Install with explicit alias and actr_type
            if self.packages.is_empty() {
                return Err(ActrCliError::InvalidArgument {
                    message:
                        "When using --actr-type, you must provide an alias as the first argument"
                            .to_string(),
                }
                .into());
            }
            let alias = self.packages[0].clone();
            let actr_type = ActrType::from_string_repr(actr_type_str).map_err(|_| {
                ActrCliError::InvalidArgument {
                    message: format!(
                        "Invalid actr_type format '{}'. Expected format: manufacturer+name (e.g., acme+EchoService)",
                        actr_type_str
                    ),
                }
            })?;
            InstallMode::AddWithAlias {
                alias,
                actr_type,
                fingerprint: self.fingerprint.clone(),
            }
        } else if !self.packages.is_empty() {
            InstallMode::AddNewPackage {
                packages: self.packages.clone(),
            }
        } else {
            InstallMode::InstallFromConfig {
                force_update: self.force_update,
            }
        };

        // Execute based on mode
        match mode {
            InstallMode::AddNewPackage { ref packages } => {
                self.execute_add_package(context, packages).await
            }
            InstallMode::AddWithAlias {
                ref alias,
                ref actr_type,
                ref fingerprint,
            } => {
                self.execute_add_with_alias(context, alias, actr_type, fingerprint.as_deref())
                    .await
            }
            InstallMode::InstallFromConfig { force_update } => {
                self.execute_install_from_config(context, force_update)
                    .await
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
        actr_type: Option<String>,
        fingerprint: Option<String>,
        force: bool,
        force_update: bool,
        skip_verification: bool,
    ) -> Self {
        Self {
            packages,
            actr_type,
            fingerprint,
            force,
            force_update,
            skip_verification,
        }
    }

    // Create from clap Args
    pub fn from_args(args: &InstallCommand) -> Self {
        InstallCommand {
            packages: args.packages.clone(),
            actr_type: args.actr_type.clone(),
            fingerprint: args.fingerprint.clone(),
            force: args.force,
            force_update: args.force_update,
            skip_verification: args.skip_verification,
        }
    }

    /// Check if in Actor-RTC project
    fn is_actr_project(&self) -> bool {
        std::path::Path::new("Actr.toml").exists()
    }

    /// Execute Mode 1: Add new package (actr install <package>)
    /// - Pull remote proto to protos/ folder
    /// - Modify Actr.toml (add dependency)
    /// - Update Actr.lock.toml
    async fn execute_add_package(
        &self,
        context: &CommandContext,
        packages: &[String],
    ) -> Result<CommandResult> {
        println!("actr install {}", packages.join(" "));

        let install_pipeline = {
            let mut container = context.container.lock().unwrap();
            container.get_install_pipeline()?
        };

        let mut resolved_specs = Vec::new();

        println!("🔍 Phase 1: Complete Validation");
        for package in packages {
            // Phase 1: Check-First validation
            println!("  ├─ 📋 Parsing dependency spec: {}", package);

            // Discover service details
            let service_details = install_pipeline
                .validation_pipeline()
                .service_discovery()
                .get_service_details(package)
                .await?;

            println!(
                "  ├─ 🔍 Service discovery: fingerprint {}",
                service_details.info.fingerprint
            );

            // Connectivity check
            let connectivity = install_pipeline
                .validation_pipeline()
                .network_validator()
                .check_connectivity(package)
                .await?;

            if connectivity.is_reachable {
                println!("  ├─ 🌐 Network connectivity test ✅");
            } else {
                println!("  └─ ❌ Network connection failed");
                return Err(anyhow::anyhow!(
                    "Network connectivity test failed for {}",
                    package,
                ));
            }

            // Fingerprint check
            println!("  ├─ 🔐 Fingerprint integrity verification ✅");

            // Create dependency spec with resolved info
            let resolved_spec = DependencySpec {
                alias: package.clone(),
                actr_type: Some(service_details.info.actr_type.clone()),
                name: package.clone(),
                fingerprint: Some(service_details.info.fingerprint.clone()),
            };
            resolved_specs.push(resolved_spec);
            println!("  └─ ✅ Added to installation plan");
            println!();
        }

        if resolved_specs.is_empty() {
            return Ok(CommandResult::Success("No packages to install".to_string()));
        }

        // Phase 2: Atomic installation
        println!("📝 Phase 2: Atomic Installation");

        // Execute installation for all packages
        match install_pipeline.install_dependencies(&resolved_specs).await {
            Ok(result) => {
                println!("  ├─ 💾 Backing up current configuration");
                println!("  ├─ 📝 Updating Actr.toml configuration ✅");
                println!("  ├─ 📦 Caching proto files ✅");
                println!("  ├─ 🔒 Updating Actr.lock.toml ✅");
                println!("  └─ ✅ Installation completed");
                println!();
                self.display_install_success(&result);
                Ok(CommandResult::Install(result))
            }
            Err(e) => {
                println!("  └─ 🔄 Restoring backup (due to installation failure)");
                let cli_error = ActrCliError::InstallFailed {
                    reason: e.to_string(),
                };
                eprintln!("{}", ErrorReporter::format_error(&cli_error));
                Err(e)
            }
        }
    }

    /// Execute Mode 1b: Add dependency with explicit alias and actr_type
    /// - Discover service by actr_type
    /// - Use provided alias
    /// - Modify Actr.toml (add dependency with alias)
    /// - Update Actr.lock.toml
    async fn execute_add_with_alias(
        &self,
        context: &CommandContext,
        alias: &str,
        actr_type: &ActrType,
        fingerprint: Option<&str>,
    ) -> Result<CommandResult> {
        use actr_protocol::ActrTypeExt;

        println!(
            "actr install {} --actr-type {}",
            alias,
            actr_type.to_string_repr()
        );

        let install_pipeline = {
            let mut container = context.container.lock().unwrap();
            container.get_install_pipeline()?
        };

        println!("🔍 Phase 1: Complete Validation");
        println!("  ├─ 📋 Alias: {}", alias);
        println!("  ├─ 🏷️  Actor Type: {}", actr_type.to_string_repr());

        // Discover service by actr_type
        let service_discovery = install_pipeline.validation_pipeline().service_discovery();

        // Find service matching the actr_type
        let services = service_discovery.discover_services(None).await?;
        let matching_service = services
            .iter()
            .find(|s| s.actr_type == *actr_type)
            .ok_or_else(|| ActrCliError::ServiceNotFound {
                name: actr_type.to_string_repr(),
            })?;

        let service_name = matching_service.name.clone();
        println!("  ├─ 🔍 Service discovered: {}", service_name);

        // Get full service details
        let service_details = service_discovery.get_service_details(&service_name).await?;

        println!(
            "  ├─ 🔍 Service fingerprint: {}",
            service_details.info.fingerprint
        );

        // Verify fingerprint if provided
        if let Some(expected_fp) = fingerprint {
            if service_details.info.fingerprint != expected_fp {
                println!("  └─ ❌ Fingerprint mismatch");
                return Err(ActrCliError::FingerprintMismatch {
                    expected: expected_fp.to_string(),
                    actual: service_details.info.fingerprint.clone(),
                }
                .into());
            }
            println!("  ├─ 🔐 Fingerprint verification ✅");
        }

        // Connectivity check
        let connectivity = install_pipeline
            .validation_pipeline()
            .network_validator()
            .check_connectivity(&service_name)
            .await?;

        if connectivity.is_reachable {
            println!("  ├─ 🌐 Network connectivity test ✅");
        } else {
            println!("  └─ ❌ Network connection failed");
            return Err(anyhow::anyhow!(
                "Network connectivity test failed for {}",
                service_name,
            ));
        }

        // Create dependency spec with alias
        let resolved_spec = DependencySpec {
            alias: alias.to_string(),
            actr_type: Some(service_details.info.actr_type.clone()),
            name: service_name.clone(),
            fingerprint: Some(
                fingerprint
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| service_details.info.fingerprint.clone()),
            ),
        };

        println!("  └─ ✅ Added to installation plan");
        println!();

        // Phase 2: Atomic installation
        println!("📝 Phase 2: Atomic Installation");

        // Execute installation
        match install_pipeline
            .install_dependencies(&[resolved_spec])
            .await
        {
            Ok(result) => {
                println!("  ├─ 💾 Backing up current configuration");
                println!("  ├─ 📝 Updating Actr.toml configuration ✅");
                println!("  ├─ 📦 Caching proto files ✅");
                println!("  ├─ 🔒 Updating Actr.lock.toml ✅");
                println!("  └─ ✅ Installation completed");
                println!();
                self.display_install_success(&result);
                Ok(CommandResult::Install(result))
            }
            Err(e) => {
                println!("  └─ 🔄 Restoring backup (due to installation failure)");
                let cli_error = ActrCliError::InstallFailed {
                    reason: e.to_string(),
                };
                eprintln!("{}", ErrorReporter::format_error(&cli_error));
                Err(e)
            }
        }
    }

    /// Execute Mode 2: Install from config (actr install)
    /// - Do NOT modify Actr.toml
    /// - Use lock file versions if available
    /// - Check for compatibility conflicts when lock file exists
    /// - Only update Actr.lock.toml
    async fn execute_install_from_config(
        &self,
        context: &CommandContext,
        force_update: bool,
    ) -> Result<CommandResult> {
        if force_update || self.force {
            println!("📦 Force updating all service dependencies");
        } else {
            println!("📦 Installing service dependencies from config");
        }
        println!();

        // Load dependencies from Actr.toml
        let dependency_specs = self.load_dependencies_from_config(context).await?;

        if dependency_specs.is_empty() {
            println!("ℹ️ No dependencies configured, generating empty lock file");

            // Generate empty lock file with metadata
            let install_pipeline = {
                let mut container = context.container.lock().unwrap();
                container.get_install_pipeline()?
            };
            let project_root = install_pipeline.config_manager().get_project_root();
            let lock_file_path = project_root.join("Actr.lock.toml");

            let mut lock_file = LockFile::new();
            lock_file.update_timestamp();
            lock_file
                .save_to_file(&lock_file_path)
                .map_err(|e| ActrCliError::InstallFailed {
                    reason: format!("Failed to save lock file: {}", e),
                })?;

            println!("  └─ 🔒 Generated Actr.lock.toml");
            return Ok(CommandResult::Success(
                "Generated empty lock file".to_string(),
            ));
        }

        // Check for duplicate actr_type conflicts
        let conflicts = self.check_actr_type_conflicts(&dependency_specs);
        if !conflicts.is_empty() {
            println!("❌ Dependency conflict detected:");
            for conflict in &conflicts {
                println!("   • {}", conflict);
            }
            println!();
            println!(
                "💡 Tip: Each actr_type can only be used once. Please use different aliases for different services or remove duplicate dependencies."
            );
            return Err(ActrCliError::DependencyConflict {
                message: format!(
                    "{} dependency conflict(s) detected. Each actr_type must be unique.",
                    conflicts.len()
                ),
            }
            .into());
        }

        println!("🔍 Phase 1: Full Validation");
        for spec in &dependency_specs {
            println!("  ├─ 📋 Parsing dependency: {}", spec.alias);
        }

        // Get install pipeline
        let install_pipeline = {
            let mut container = context.container.lock().unwrap();
            container.get_install_pipeline()?
        };

        // Check for compatibility conflicts when lock file exists (unless force_update)
        if !force_update && !self.force {
            let project_root = install_pipeline.config_manager().get_project_root();
            let lock_file_path = project_root.join("Actr.lock.toml");
            if lock_file_path.exists() {
                println!("  ├─ 🔒 Lock file found, checking compatibility...");

                // Perform compatibility check
                let conflicts = self
                    .check_lock_file_compatibility(
                        &lock_file_path,
                        &dependency_specs,
                        &install_pipeline,
                    )
                    .await?;

                if !conflicts.is_empty() {
                    println!("  └─ ❌ Compatibility conflicts detected");
                    println!();
                    println!("⚠️  Breaking changes detected:");
                    for conflict in &conflicts {
                        println!("   • {}", conflict);
                    }
                    println!();
                    println!(
                        "💡 Tip: Use --force-update to override and update to the latest versions"
                    );
                    return Err(ActrCliError::CompatibilityConflict {
                        message: format!(
                            "{} breaking change(s) detected. Use --force-update to override.",
                            conflicts.len()
                        ),
                    }
                    .into());
                }
                println!("  ├─ ✅ Compatibility check passed");
            }
        }

        // Verify fingerprints match registered services (unless --force is used)
        println!("  ├─ ✅ Verifying fingerprints...");
        let fingerprint_mismatches = self
            .verify_fingerprints(&dependency_specs, &install_pipeline)
            .await?;

        if !fingerprint_mismatches.is_empty() && !self.force {
            println!("  └─ ❌ Fingerprint mismatch detected");
            println!();
            println!("⚠️  Fingerprint mismatch:");
            for mismatch in &fingerprint_mismatches {
                println!("   • {}", mismatch);
            }
            println!();
            println!(
                "💡 Tip: Use --force to update Actr.toml with the current service fingerprints"
            );
            return Err(ActrCliError::FingerprintValidation {
                message: format!(
                    "{} fingerprint mismatch(es) detected. Use --force to update.",
                    fingerprint_mismatches.len()
                ),
            }
            .into());
        }

        // If --force is used and there are mismatches, update Actr.toml
        if !fingerprint_mismatches.is_empty() && self.force {
            println!("  ├─ ⚠️  Fingerprint mismatch detected, updating Actr.toml...");
            self.update_config_fingerprints(context, &dependency_specs, &install_pipeline)
                .await?;
            println!("  ├─ ✅ Actr.toml updated with current fingerprints");

            // Reload dependency specs with updated fingerprints
            let dependency_specs = self.load_dependencies_from_config(context).await?;

            println!("  ├─ 🔍 Service discovery (DiscoveryRequest)");
            println!("  ├─ 🌐 Network connectivity test");
            println!("  └─ ✅ Installation plan generated");
            println!();

            // Execute installation with updated specs
            println!("📝 Phase 2: Atomic Installation");
            return match install_pipeline
                .install_dependencies(&dependency_specs)
                .await
            {
                Ok(install_result) => {
                    println!("  ├─ 📚 Caching proto files ✅");
                    println!("  ├─ 🔒 Updating Actr.lock.toml ✅");
                    println!("  └─ ✅ Installation completed");
                    println!();
                    println!(
                        "📝 Note: Actr.toml fingerprints were updated to match current services"
                    );
                    self.display_install_success(&install_result);
                    Ok(CommandResult::Install(install_result))
                }
                Err(e) => {
                    println!("  └─ ❌ Installation failed");
                    let cli_error = ActrCliError::InstallFailed {
                        reason: e.to_string(),
                    };
                    eprintln!("{}", ErrorReporter::format_error(&cli_error));
                    Err(e)
                }
            };
        }

        println!("  ├─ ✅ Fingerprint verification passed");
        println!("  ├─ 🔍 Service discovery (DiscoveryRequest)");
        println!("  ├─ 🌐 Network connectivity test");
        println!("  └─ ✅ Installation plan generated");
        println!();

        // Execute check-first install flow (Mode 2: no config update)
        println!("📝 Phase 2: Atomic Installation");
        match install_pipeline
            .install_dependencies(&dependency_specs)
            .await
        {
            Ok(install_result) => {
                println!("  ├─ 📦 Caching proto files ✅");
                println!("  ├─ 🔒 Updating Actr.lock.toml ✅");
                println!("  └─ ✅ Installation completed");
                println!();
                self.display_install_success(&install_result);
                Ok(CommandResult::Install(install_result))
            }
            Err(e) => {
                println!("  └─ ❌ Installation failed");
                let cli_error = ActrCliError::InstallFailed {
                    reason: e.to_string(),
                };
                eprintln!("{}", ErrorReporter::format_error(&cli_error));
                Err(e)
            }
        }
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

        let specs: Vec<DependencySpec> = config
            .dependencies
            .into_iter()
            .map(|dependency| DependencySpec {
                alias: dependency.alias,
                actr_type: dependency.actr_type,
                name: dependency.name,
                fingerprint: dependency.fingerprint,
            })
            .collect();

        Ok(specs)
    }

    /// Check for duplicate actr_type conflicts in dependencies
    fn check_actr_type_conflicts(&self, specs: &[DependencySpec]) -> Vec<String> {
        use std::collections::HashMap;

        let mut actr_type_map: HashMap<String, Vec<&str>> = HashMap::new();
        let mut conflicts = Vec::new();

        for spec in specs {
            if let Some(ref actr_type) = spec.actr_type {
                let type_str = actr_type.to_string_repr();
                actr_type_map.entry(type_str).or_default().push(&spec.alias);
            }
        }

        for (actr_type, aliases) in actr_type_map {
            if aliases.len() > 1 {
                conflicts.push(format!(
                    "actr_type '{}' is used by multiple dependencies: {}",
                    actr_type,
                    aliases.join(", ")
                ));
            }
        }

        conflicts
    }

    /// Verify that fingerprints in Actr.toml match the currently registered services
    async fn verify_fingerprints(
        &self,
        specs: &[DependencySpec],
        install_pipeline: &std::sync::Arc<crate::core::InstallPipeline>,
    ) -> Result<Vec<String>> {
        let mut mismatches = Vec::new();
        let service_discovery = install_pipeline.validation_pipeline().service_discovery();

        for spec in specs {
            // Only check if fingerprint is specified in Actr.toml
            let expected_fingerprint = match &spec.fingerprint {
                Some(fp) => fp,
                None => continue,
            };

            // Get current service details
            let current_service = match service_discovery.get_service_details(&spec.name).await {
                Ok(s) => s,
                Err(e) => {
                    mismatches.push(format!(
                        "{}: Service not found or unavailable ({})",
                        spec.alias, e
                    ));
                    continue;
                }
            };

            let current_fingerprint = &current_service.info.fingerprint;

            // Compare fingerprints
            if expected_fingerprint != current_fingerprint {
                mismatches.push(format!(
                    "{}: Expected fingerprint '{}', but service has '{}'",
                    spec.alias, expected_fingerprint, current_fingerprint
                ));
            }
        }

        Ok(mismatches)
    }

    /// Update Actr.toml with current service fingerprints
    async fn update_config_fingerprints(
        &self,
        _context: &CommandContext,
        specs: &[DependencySpec],
        install_pipeline: &std::sync::Arc<crate::core::InstallPipeline>,
    ) -> Result<()> {
        let service_discovery = install_pipeline.validation_pipeline().service_discovery();
        let config_manager = install_pipeline.config_manager();

        // Update fingerprints for each dependency that has one specified
        for spec in specs {
            if spec.fingerprint.is_none() {
                continue;
            }

            // Get current service fingerprint
            let current_service = match service_discovery.get_service_details(&spec.name).await {
                Ok(s) => s,
                Err(_) => continue,
            };

            let old_fingerprint = spec
                .fingerprint
                .clone()
                .unwrap_or_else(|| "none".to_string());
            let new_fingerprint = current_service.info.fingerprint.clone();

            // Create updated spec with new fingerprint
            let updated_spec = DependencySpec {
                alias: spec.alias.clone(),
                name: spec.name.clone(),
                actr_type: spec.actr_type.clone(),
                fingerprint: Some(new_fingerprint.clone()),
            };

            // Use update_dependency to modify Actr.toml directly
            config_manager.update_dependency(&updated_spec).await?;

            println!(
                "   📝 Updated '{}' fingerprint: {} → {}",
                spec.alias, old_fingerprint, new_fingerprint
            );
        }

        Ok(())
    }

    /// Check compatibility between locked dependencies and currently registered services
    ///
    /// This method compares the fingerprints stored in the lock file with the fingerprints
    /// of the services currently registered on the signaling server. If a service's proto
    /// definition has breaking changes compared to the locked version, a conflict is reported.
    async fn check_lock_file_compatibility(
        &self,
        lock_file_path: &std::path::Path,
        dependency_specs: &[DependencySpec],
        install_pipeline: &std::sync::Arc<crate::core::InstallPipeline>,
    ) -> Result<Vec<String>> {
        use actr_protocol::ServiceSpec;

        let mut conflicts = Vec::new();

        // Load lock file
        let lock_file = match LockFile::from_file(lock_file_path) {
            Ok(lf) => lf,
            Err(e) => {
                tracing::warn!("Failed to parse lock file: {}", e);
                return Ok(conflicts); // If we can't parse lock file, skip compatibility check
            }
        };

        // For each dependency, check if the currently registered service is compatible
        for spec in dependency_specs {
            // Find the locked dependency by name
            let locked_dep = lock_file.dependencies.iter().find(|d| d.name == spec.name);

            let locked_dep = match locked_dep {
                Some(d) => d,
                None => {
                    // Dependency not in lock file, skip (will be newly installed)
                    tracing::debug!("Dependency '{}' not in lock file, skipping", spec.name);
                    continue;
                }
            };

            let locked_fingerprint = &locked_dep.fingerprint;

            // Get current service details from the registry
            let service_discovery = install_pipeline.validation_pipeline().service_discovery();
            let current_service = match service_discovery.get_service_details(&spec.name).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("Failed to get service details for '{}': {}", spec.name, e);
                    continue;
                }
            };

            let current_fingerprint = &current_service.info.fingerprint;

            // If fingerprints match, no need for deep analysis
            if locked_fingerprint == current_fingerprint {
                tracing::debug!(
                    "Fingerprint match for '{}', no compatibility check needed",
                    spec.name
                );
                continue;
            }

            // Fingerprints differ - perform deep compatibility analysis using actr-version
            tracing::info!(
                "Fingerprint mismatch for '{}': locked={}, current={}",
                spec.name,
                locked_fingerprint,
                current_fingerprint
            );

            // Build ServiceSpec from locked proto content for comparison
            // Note: Since lock file only stores metadata (not full proto content),
            // we need to use semantic fingerprint comparison for compatibility check

            // Convert current service proto files to actr-version ProtoFile format
            let current_proto_files: Vec<ProtoFile> = current_service
                .proto_files
                .iter()
                .map(|pf| ProtoFile {
                    name: pf.name.clone(),
                    content: pf.content.clone(),
                    path: Some(pf.path.to_string_lossy().to_string()),
                })
                .collect();

            // Calculate current service's semantic fingerprint
            let current_semantic_fp =
                match Fingerprint::calculate_service_semantic_fingerprint(&current_proto_files) {
                    Ok(fp) => fp,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to calculate semantic fingerprint for '{}': {}",
                            spec.name,
                            e
                        );
                        // If we can't calculate fingerprint, report as potential conflict
                        conflicts.push(format!(
                            "{}: Unable to verify compatibility (fingerprint calculation failed)",
                            spec.name
                        ));
                        continue;
                    }
                };

            // Compare fingerprints using semantic analysis
            // The locked fingerprint should be a service_semantic fingerprint
            let locked_semantic = if locked_fingerprint.starts_with("service_semantic:") {
                locked_fingerprint
                    .strip_prefix("service_semantic:")
                    .unwrap_or(locked_fingerprint)
            } else {
                locked_fingerprint.as_str()
            };

            if current_semantic_fp != locked_semantic {
                // Semantic fingerprints differ - this indicates breaking changes
                // Build ServiceSpec structures for detailed comparison
                let locked_spec = ServiceSpec {
                    name: spec.name.clone(),
                    description: locked_dep.description.clone(),
                    fingerprint: locked_fingerprint.clone(),
                    protobufs: locked_dep
                        .files
                        .iter()
                        .map(|pf| actr_protocol::service_spec::Protobuf {
                            package: pf.path.clone(),
                            content: String::new(), // Lock file doesn't store content
                            fingerprint: pf.fingerprint.clone(),
                        })
                        .collect(),
                    published_at: locked_dep.published_at,
                    tags: locked_dep.tags.clone(),
                };

                let current_spec = ServiceSpec {
                    name: spec.name.clone(),
                    description: Some(current_service.info.description.clone().unwrap_or_default()),
                    fingerprint: format!("service_semantic:{}", current_semantic_fp),
                    protobufs: current_proto_files
                        .iter()
                        .map(|pf| actr_protocol::service_spec::Protobuf {
                            package: pf.name.clone(),
                            content: pf.content.clone(),
                            fingerprint: String::new(),
                        })
                        .collect(),
                    published_at: current_service.info.published_at,
                    tags: current_service.info.tags.clone(),
                };

                // Attempt to analyze compatibility
                match ServiceCompatibility::analyze_compatibility(&locked_spec, &current_spec) {
                    Ok(analysis) => {
                        match analysis.level {
                            CompatibilityLevel::BreakingChanges => {
                                let change_summary = analysis
                                    .breaking_changes
                                    .iter()
                                    .map(|c| c.message.clone())
                                    .collect::<Vec<_>>()
                                    .join("; ");

                                conflicts.push(format!(
                                    "{}: Breaking changes detected - {}",
                                    spec.name, change_summary
                                ));
                            }
                            CompatibilityLevel::BackwardCompatible => {
                                tracing::info!(
                                    "Service '{}' has backward compatible changes",
                                    spec.name
                                );
                                // Backward compatible is allowed, no conflict
                            }
                            CompatibilityLevel::FullyCompatible => {
                                // This shouldn't happen if fingerprints differ, but handle it
                                tracing::debug!(
                                    "Service '{}' is fully compatible despite fingerprint difference",
                                    spec.name
                                );
                            }
                        }
                    }
                    Err(e) => {
                        // If detailed analysis fails, report based on fingerprint difference
                        tracing::warn!("Compatibility analysis failed for '{}': {}", spec.name, e);
                        conflicts.push(format!(
                            "{}: Service definition changed (locked: {}, current: {})",
                            spec.name, locked_fingerprint, current_fingerprint
                        ));
                    }
                }
            }
        }

        Ok(conflicts)
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
        Self::new(Vec::new(), None, None, false, false, false)
    }
}
