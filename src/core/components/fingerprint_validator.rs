//! Default FingerprintValidator implementation

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use super::{Fingerprint, FingerprintValidator, ResolvedDependency, ServiceInfo};

/// Default fingerprint validator
pub struct DefaultFingerprintValidator;

impl DefaultFingerprintValidator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultFingerprintValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FingerprintValidator for DefaultFingerprintValidator {
    async fn compute_service_fingerprint(&self, service: &ServiceInfo) -> Result<Fingerprint> {
        Ok(Fingerprint {
            algorithm: "sha256".to_string(),
            value: service.fingerprint.clone(),
        })
    }

    async fn verify_fingerprint(
        &self,
        expected: &Fingerprint,
        actual: &Fingerprint,
    ) -> Result<bool> {
        Ok(expected.algorithm == actual.algorithm && expected.value == actual.value)
    }

    async fn compute_project_fingerprint(&self, _project_path: &Path) -> Result<Fingerprint> {
        Ok(Fingerprint {
            algorithm: "sha256".to_string(),
            value: "project_fingerprint".to_string(),
        })
    }

    async fn generate_lock_fingerprint(&self, _deps: &[ResolvedDependency]) -> Result<Fingerprint> {
        Ok(Fingerprint {
            algorithm: "sha256".to_string(),
            value: "lock_fingerprint".to_string(),
        })
    }
}
