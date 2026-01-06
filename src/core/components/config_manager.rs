use actr_config::{Config, ConfigParser};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use toml::map::Map;

use crate::core::{ConfigBackup, ConfigManager, ConfigValidation, DependencySpec};

pub struct TomlConfigManager {
    config_path: PathBuf,
    project_root: PathBuf,
}

impl TomlConfigManager {
    pub fn new<P: Into<PathBuf>>(config_path: P) -> Self {
        let config_path = config_path.into();
        let project_root = resolve_project_root(&config_path);
        Self {
            config_path,
            project_root,
        }
    }

    async fn read_config_string(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read config file: {}", path.display()))
    }

    async fn write_config_string(&self, path: &Path, contents: &str) -> Result<()> {
        fs::write(path, contents)
            .await
            .with_context(|| format!("Failed to write config file: {}", path.display()))
    }

    fn dependency_to_value(spec: &DependencySpec) -> toml::Value {
        let mut table = Map::new();
        if let Some(fingerprint) = &spec.fingerprint {
            if let Some(actr_type) = Self::actr_type_from_uri(&spec.uri) {
                table.insert("actr_type".to_string(), toml::Value::String(actr_type));
            }
            table.insert(
                "fingerprint".to_string(),
                toml::Value::String(fingerprint.clone()),
            );
        }
        toml::Value::Table(table)
    }

    fn actr_type_from_uri(uri: &str) -> Option<String> {
        let without_scheme = uri.strip_prefix("actr://")?;
        let name_end = without_scheme
            .find(|c| ['/', '?'].contains(&c))
            .unwrap_or(without_scheme.len());
        let name = without_scheme[..name_end].trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }

    fn build_backup_path(&self) -> Result<PathBuf> {
        let file_name = self
            .config_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Config path is missing file name"))?
            .to_string_lossy();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let backup_name = format!("{file_name}.bak.{timestamp}");
        let parent = self
            .config_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Ok(parent.join(backup_name))
    }
}

#[async_trait]
impl ConfigManager for TomlConfigManager {
    async fn load_config(&self, path: &Path) -> Result<Config> {
        ConfigParser::from_file(path)
            .with_context(|| format!("Failed to parse config: {}", path.display()))
    }

    async fn save_config(&self, _config: &Config, _path: &Path) -> Result<()> {
        Err(anyhow::anyhow!(
            "Saving parsed Config is not supported; update Actr.toml directly"
        ))
    }

    async fn update_dependency(&self, spec: &DependencySpec) -> Result<()> {
        let contents = self.read_config_string(&self.config_path).await?;
        let mut value: toml::Value = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config: {}", self.config_path.display()))?;

        let root = value
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("Config root must be a table"))?;
        let deps_value = root
            .entry("dependencies".to_string())
            .or_insert_with(|| toml::Value::Table(Map::new()));
        let deps_table = deps_value
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("dependencies must be a table"))?;

        deps_table.insert(spec.name.clone(), Self::dependency_to_value(spec));

        let updated = toml::to_string_pretty(&value).context("Failed to serialize config")?;
        self.write_config_string(&self.config_path, &updated).await
    }

    async fn validate_config(&self) -> Result<ConfigValidation> {
        let mut errors = Vec::new();
        let warnings = Vec::new();

        let config = match ConfigParser::from_file(&self.config_path) {
            Ok(config) => config,
            Err(e) => {
                errors.push(format!("Failed to parse config: {e}"));
                return Ok(ConfigValidation {
                    is_valid: false,
                    errors,
                    warnings,
                });
            }
        };

        if config.package.name.trim().is_empty() {
            errors.push("package.name is required".to_string());
        }

        for dependency in &config.dependencies {
            if dependency.alias.trim().is_empty() {
                errors.push("dependency alias is required".to_string());
            }
            if dependency.actr_type.name.trim().is_empty() {
                errors.push(format!(
                    "dependency {} has an empty actr_type name",
                    dependency.alias
                ));
            }
        }

        Ok(ConfigValidation {
            is_valid: errors.is_empty(),
            errors,
            warnings,
        })
    }

    fn get_project_root(&self) -> &Path {
        &self.project_root
    }

    async fn backup_config(&self) -> Result<ConfigBackup> {
        if !self.config_path.exists() {
            return Err(anyhow::anyhow!(
                "Config file not found: {}",
                self.config_path.display()
            ));
        }

        let backup_path = self.build_backup_path()?;
        fs::copy(&self.config_path, &backup_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to backup config from {} to {}",
                    self.config_path.display(),
                    backup_path.display()
                )
            })?;

        Ok(ConfigBackup {
            original_path: self.config_path.clone(),
            backup_path,
            timestamp: SystemTime::now(),
        })
    }

    async fn restore_backup(&self, backup: ConfigBackup) -> Result<()> {
        fs::copy(&backup.backup_path, &backup.original_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to restore config from {} to {}",
                    backup.backup_path.display(),
                    backup.original_path.display()
                )
            })?;
        Ok(())
    }

    async fn remove_backup(&self, backup: ConfigBackup) -> Result<()> {
        if backup.backup_path.exists() {
            fs::remove_file(&backup.backup_path)
                .await
                .with_context(|| {
                    format!(
                        "Failed to remove backup file: {}",
                        backup.backup_path.display()
                    )
                })?;
        }
        Ok(())
    }
}

fn resolve_project_root(config_path: &Path) -> PathBuf {
    let canonical_path =
        std::fs::canonicalize(config_path).expect("Failed to canonicalize config path");
    canonical_path
        .parent()
        .expect("Config path must have a parent directory")
        .to_path_buf()
}
