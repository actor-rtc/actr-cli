//! Default NetworkValidator implementation

use anyhow::Result;
use async_trait::async_trait;

use super::{ConnectivityStatus, HealthStatus, LatencyInfo, NetworkCheckResult, NetworkValidator};

/// Default network validator (stub implementation)
pub struct DefaultNetworkValidator;

impl DefaultNetworkValidator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultNetworkValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkValidator for DefaultNetworkValidator {
    async fn check_connectivity(&self, _uri: &str) -> Result<ConnectivityStatus> {
        // For now, assume all connections are reachable
        Ok(ConnectivityStatus {
            is_reachable: true,
            response_time_ms: Some(10),
            error: None,
        })
    }

    async fn verify_service_health(&self, _uri: &str) -> Result<HealthStatus> {
        Ok(HealthStatus::Healthy)
    }

    async fn test_latency(&self, _uri: &str) -> Result<LatencyInfo> {
        Ok(LatencyInfo {
            min_ms: 5,
            max_ms: 20,
            avg_ms: 10,
            samples: 3,
        })
    }

    async fn batch_check(&self, uris: &[String]) -> Result<Vec<NetworkCheckResult>> {
        let mut results = Vec::new();
        for uri in uris {
            results.push(NetworkCheckResult {
                uri: uri.clone(),
                connectivity: ConnectivityStatus {
                    is_reachable: true,
                    response_time_ms: Some(10),
                    error: None,
                },
                health: HealthStatus::Healthy,
                latency: Some(LatencyInfo {
                    min_ms: 5,
                    max_ms: 20,
                    avg_ms: 10,
                    samples: 3,
                }),
            });
        }
        Ok(results)
    }
}
