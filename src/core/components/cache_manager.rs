//! Default CacheManager implementation
//!
//! Proto files are cached to the project's `proto/` folder (not ~/.actr/cache)
//! following the documentation spec for dependency management.

use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

use super::{CacheManager, CacheStats, CachedProto, Fingerprint, ProtoFile};

/// Default cache manager (file-based, project-local)
///
/// Caches proto files to `{project_root}/proto/{service_name}/` directory
/// following the documentation spec.
pub struct DefaultCacheManager {
    /// Project root directory (where Actr.toml is located)
    project_root: PathBuf,
}

impl DefaultCacheManager {
    pub fn new() -> Self {
        Self {
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn with_project_root(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Get the proto cache directory for a service
    /// Returns: {project_root}/proto/{service_name}/
    fn get_service_proto_dir(&self, service_name: &str) -> PathBuf {
        self.project_root.join("proto").join(service_name)
    }

    /// Extract service name from actr:// URI
    /// Format: actr://realm:manufacturer+name@version/ -> manufacturer+name
    fn extract_service_name_from_uri(uri: &str) -> String {
        let clean_uri = uri.trim_start_matches("actr://").trim_end_matches('/');

        // Handle realm:manufacturer+name@version format
        if let Some(colon_pos) = clean_uri.find(':') {
            let after_realm = &clean_uri[colon_pos + 1..];
            // Remove @version if present
            if let Some(at_pos) = after_realm.find('@') {
                after_realm[..at_pos].to_string()
            } else {
                after_realm.to_string()
            }
        } else if let Some(at_pos) = clean_uri.find('@') {
            // Handle manufacturer+name@version format (no realm)
            clean_uri[..at_pos].to_string()
        } else {
            clean_uri.to_string()
        }
    }

    /// Generate proto file path for caching
    /// Format: {service_name}/{package_name}.proto
    fn generate_proto_path(service_name: &str, package_name: &str) -> String {
        // Convert package name to file path (e.g., user.v1 -> user.v1.proto)
        format!("{}/{}.proto", service_name, package_name)
    }
}

impl Default for DefaultCacheManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CacheManager for DefaultCacheManager {
    async fn get_cached_proto(&self, uri: &str) -> Result<Option<CachedProto>> {
        let service_name = Self::extract_service_name_from_uri(uri);
        let cache_path = self.get_service_proto_dir(&service_name);

        if !cache_path.exists() {
            return Ok(None);
        }

        let mut files = Vec::new();
        for entry in std::fs::read_dir(&cache_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "proto").unwrap_or(false) {
                let content = std::fs::read_to_string(&path)?;
                files.push(ProtoFile {
                    name: path.file_name().unwrap().to_string_lossy().to_string(),
                    path,
                    content,
                    services: Vec::new(),
                });
            }
        }

        if files.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CachedProto {
                uri: uri.to_string(),
                files,
                fingerprint: Fingerprint {
                    algorithm: "sha256".to_string(),
                    value: "cached".to_string(),
                },
                cached_at: std::time::SystemTime::now(),
                expires_at: None,
            }))
        }
    }

    async fn cache_proto(&self, uri: &str, files: &[ProtoFile]) -> Result<()> {
        let service_name = Self::extract_service_name_from_uri(uri);
        let cache_path = self.get_service_proto_dir(&service_name);
        std::fs::create_dir_all(&cache_path)?;

        for file in files {
            // Use the proto file name directly (e.g., echo.v1.proto)
            let file_name = if file.name.ends_with(".proto") {
                file.name.clone()
            } else {
                format!("{}.proto", file.name)
            };
            let file_path = cache_path.join(&file_name);
            std::fs::write(&file_path, &file.content)?;
            tracing::debug!(
                "Cached proto file: {} -> {}",
                file.name,
                file_path.display()
            );
        }

        tracing::info!(
            "Cached {} proto files to proto/{}/",
            files.len(),
            service_name
        );
        Ok(())
    }

    async fn invalidate_cache(&self, uri: &str) -> Result<()> {
        let service_name = Self::extract_service_name_from_uri(uri);
        let cache_path = self.get_service_proto_dir(&service_name);
        if cache_path.exists() {
            std::fs::remove_dir_all(&cache_path)?;
        }
        Ok(())
    }

    async fn clear_cache(&self) -> Result<()> {
        let proto_dir = self.project_root.join("proto");
        if proto_dir.exists() {
            std::fs::remove_dir_all(&proto_dir)?;
        }
        Ok(())
    }

    async fn get_cache_stats(&self) -> Result<CacheStats> {
        let proto_dir = self.project_root.join("proto");
        let mut total_size = 0u64;
        let mut entry_count = 0usize;

        if proto_dir.exists() {
            for entry in std::fs::read_dir(&proto_dir)? {
                entry_count += 1;
                let entry = entry?;
                if entry.path().is_dir() {
                    for file in std::fs::read_dir(entry.path())? {
                        let file = file?;
                        total_size += file.metadata()?.len();
                    }
                }
            }
        }

        Ok(CacheStats {
            total_entries: entry_count,
            total_size_bytes: total_size,
            hit_rate: 0.0,
            miss_rate: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_service_name_from_uri() {
        // Test realm:manufacturer+name@version format
        assert_eq!(
            DefaultCacheManager::extract_service_name_from_uri("actr://5:acme+EchoService@v1/"),
            "acme+EchoService"
        );

        // Test manufacturer+name@version format (no realm)
        assert_eq!(
            DefaultCacheManager::extract_service_name_from_uri("actr://acme+EchoService@v1/"),
            "acme+EchoService"
        );

        // Test manufacturer+name format (no version)
        assert_eq!(
            DefaultCacheManager::extract_service_name_from_uri("actr://acme+EchoService/"),
            "acme+EchoService"
        );

        // Test simple name
        assert_eq!(
            DefaultCacheManager::extract_service_name_from_uri("actr://EchoService/"),
            "EchoService"
        );
    }
}
