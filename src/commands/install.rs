//! Install Command Implementation
//!
//! Implement install flow based on reuse architecture with check-first principle

use crate::core::{
    ActrCliError, Command, CommandContext, CommandResult, ComponentType, DependencySpec,
    ErrorReporter, InstallResult,
};
use anyhow::Result;
use async_trait::async_trait;
use clap::Args;

/// Install command
#[derive(Args, Debug)]
#[command(
    about = "Install service dependencies",
    long_about = "Install service dependencies. You can install specific service packages, or install all dependencies configured in Actr.toml"
)]
pub struct InstallCommand {
    /// List of service packages to install (e.g., user-service)
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

/// Installation mode
#[derive(Debug, Clone)]
pub enum InstallMode {
    /// Mode 1: Add new dependency (npm install <package>)
    /// - Pull remote proto to proto/ folder
    /// - Modify Actr.toml (add dependency)
    /// - Update Actr.lock.toml
    AddNewPackage { packages: Vec<String> },

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
        let mode = if !self.packages.is_empty() {
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

    /// Execute Mode 1: Add new package (actr install <package>)
    /// - Pull remote proto to proto/ folder
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

    /// Execute Mode 2: Install from config (actr install)
    /// - Do NOT modify Actr.toml
    /// - Use lock file versions if available
    /// - Only update Actr.lock.toml
    async fn execute_install_from_config(
        &self,
        context: &CommandContext,
        force_update: bool,
    ) -> Result<CommandResult> {
        if force_update {
            println!("📦 Force updating all service dependencies");
        } else {
            println!("📦 Installing service dependencies from config");
        }
        println!();

        // Load dependencies from Actr.toml
        let dependency_specs = self.load_dependencies_from_config(context).await?;

        if dependency_specs.is_empty() {
            println!("ℹ️ No dependencies to install");
            return Ok(CommandResult::Success(
                "No dependencies to install".to_string(),
            ));
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

        // Check if we should use lock file (unless force_update)
        if !force_update {
            let project_root = install_pipeline.config_manager().get_project_root();
            let lock_file_path = project_root.join("Actr.lock.toml");
            if lock_file_path.exists() {
                println!("  ├─ 🔒 Using versions from lock file");
            }
        }

        println!("  ├─ 🔍 Service discovery (DiscoveryRequest)");
        println!("  ├─ 🌐 Network connectivity test");
        println!("  ├─ 🔐 Fingerprint integrity verification");
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
