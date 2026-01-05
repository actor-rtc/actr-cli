use actr_protocol::{
    AIdCredential, ActrId, ActrToSignaling, ActrType, DiscoveryRequest, ErrorResponse,
    PeerToSignaling, Realm, RegisterRequest, SignalingEnvelope, actr_to_signaling,
    discovery_response, peer_to_signaling, register_response, signaling_envelope,
    signaling_to_actr,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use prost_types::Timestamp;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::core::{
    AvailabilityStatus, ConfigManager, HealthStatus, ProtoFile, ServiceDetails, ServiceDiscovery,
    ServiceFilter, ServiceInfo,
};

type SignalingSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone)]
struct SignalingSession {
    actr_id: ActrId,
    credential: AIdCredential,
}

struct SignalingClient {
    signaling_url: String,
    actr_type: ActrType,
    realm: Realm,
}

impl SignalingClient {
    fn new(signaling_url: String, actr_type: ActrType, realm: Realm) -> Self {
        Self {
            signaling_url,
            actr_type,
            realm,
        }
    }

    async fn discover_entries(
        &self,
        filter: Option<&ServiceFilter>,
    ) -> Result<Vec<discovery_response::TypeEntry>> {
        let (mut socket, _) = connect_async(self.signaling_url.as_str())
            .await
            .with_context(|| format!("Failed to connect to signaling: {}", self.signaling_url))?;

        let session = self.register(&mut socket).await?;

        let request = self.build_discovery_request(filter);
        let payload = actr_to_signaling::Payload::DiscoveryRequest(request);
        let envelope =
            Self::build_envelope(signaling_envelope::Flow::ActrToServer(ActrToSignaling {
                source: session.actr_id.clone(),
                credential: session.credential.clone(),
                payload: Some(payload),
            }))?;

        Self::send_envelope(&mut socket, envelope).await?;
        Self::receive_discovery_response(&mut socket).await
    }

    fn build_discovery_request(&self, filter: Option<&ServiceFilter>) -> DiscoveryRequest {
        let manufacturer = filter
            .and_then(|f| f.name_pattern.as_deref())
            .and_then(Self::extract_manufacturer)
            .or_else(|| {
                if self.actr_type.manufacturer.trim().is_empty() {
                    None
                } else {
                    Some(self.actr_type.manufacturer.clone())
                }
            });

        DiscoveryRequest {
            manufacturer,
            limit: None,
        }
    }

    fn extract_manufacturer(pattern: &str) -> Option<String> {
        if let Some((manufacturer, _)) = pattern.split_once('+') {
            if !manufacturer.contains('*') && !manufacturer.trim().is_empty() {
                return Some(manufacturer.trim().to_string());
            }
        }
        None
    }

    async fn register(&self, socket: &mut SignalingSocket) -> Result<SignalingSession> {
        let register_request = RegisterRequest {
            actr_type: self.actr_type.clone(),
            realm: self.realm.clone(),
            service_spec: None,
            acl: None,
        };

        let envelope =
            Self::build_envelope(signaling_envelope::Flow::PeerToServer(PeerToSignaling {
                payload: Some(peer_to_signaling::Payload::RegisterRequest(
                    register_request,
                )),
            }))?;

        Self::send_envelope(socket, envelope).await?;

        loop {
            let envelope = Self::read_envelope(socket).await?;
            match envelope.flow {
                Some(signaling_envelope::Flow::ServerToActr(server)) => match server.payload {
                    Some(signaling_to_actr::Payload::RegisterResponse(response)) => {
                        return Self::handle_register_response(response);
                    }
                    Some(signaling_to_actr::Payload::Error(error)) => {
                        return Err(Self::as_error("Register failed", &error));
                    }
                    _ => {}
                },
                Some(signaling_envelope::Flow::EnvelopeError(error)) => {
                    return Err(Self::as_error("Register failed", &error));
                }
                _ => {}
            }
        }
    }

    fn handle_register_response(
        response: actr_protocol::RegisterResponse,
    ) -> Result<SignalingSession> {
        match response.result {
            Some(register_response::Result::Success(success)) => Ok(SignalingSession {
                actr_id: success.actr_id,
                credential: success.credential,
            }),
            Some(register_response::Result::Error(error)) => {
                Err(Self::as_error("Register failed", &error))
            }
            None => Err(anyhow!("Register response is missing result")),
        }
    }

    async fn receive_discovery_response(
        socket: &mut SignalingSocket,
    ) -> Result<Vec<discovery_response::TypeEntry>> {
        loop {
            let envelope = Self::read_envelope(socket).await?;
            match envelope.flow {
                Some(signaling_envelope::Flow::ServerToActr(server)) => match server.payload {
                    Some(signaling_to_actr::Payload::DiscoveryResponse(response)) => {
                        return Self::handle_discovery_response(response);
                    }
                    Some(signaling_to_actr::Payload::Error(error)) => {
                        return Err(Self::as_error("Discovery failed", &error));
                    }
                    _ => {}
                },
                Some(signaling_envelope::Flow::EnvelopeError(error)) => {
                    return Err(Self::as_error("Discovery failed", &error));
                }
                _ => {}
            }
        }
    }

    fn handle_discovery_response(
        response: actr_protocol::DiscoveryResponse,
    ) -> Result<Vec<discovery_response::TypeEntry>> {
        match response.result {
            Some(discovery_response::Result::Success(success)) => Ok(success.entries),
            Some(discovery_response::Result::Error(error)) => {
                Err(Self::as_error("Discovery failed", &error))
            }
            None => Err(anyhow!("Discovery response is missing result")),
        }
    }

    fn as_error(context: &str, error: &ErrorResponse) -> anyhow::Error {
        anyhow!("{context}: {} ({})", error.message, error.code)
    }

    async fn send_envelope(
        socket: &mut SignalingSocket,
        envelope: SignalingEnvelope,
    ) -> Result<()> {
        let mut buf = Vec::new();
        envelope
            .encode(&mut buf)
            .context("Failed to encode signaling envelope")?;
        socket
            .send(WsMessage::Binary(buf.into()))
            .await
            .context("Failed to send signaling envelope")?;
        Ok(())
    }

    async fn read_envelope(socket: &mut SignalingSocket) -> Result<SignalingEnvelope> {
        while let Some(message) = socket.next().await {
            match message.context("Failed to read signaling response")? {
                WsMessage::Binary(bytes) => {
                    return SignalingEnvelope::decode(bytes)
                        .context("Failed to decode signaling envelope");
                }
                WsMessage::Close(_) => {
                    return Err(anyhow!("Signaling connection closed"));
                }
                WsMessage::Ping(_) | WsMessage::Pong(_) => {}
                WsMessage::Text(text) => {
                    return Err(anyhow!("Unexpected text message from signaling: {text}"));
                }
                WsMessage::Frame(_) => {}
            }
        }

        Err(anyhow!("Signaling connection closed"))
    }

    fn build_envelope(flow: signaling_envelope::Flow) -> Result<SignalingEnvelope> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("System time is before UNIX_EPOCH")?;
        let timestamp = Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        };
        let envelope_id = format!("cli-{}-{}", now.as_secs(), now.subsec_nanos());

        Ok(SignalingEnvelope {
            envelope_version: 1,
            envelope_id,
            reply_for: None,
            timestamp,
            traceparent: None,
            tracestate: None,
            flow: Some(flow),
        })
    }
}

pub struct NetworkServiceDiscovery {
    config_manager: Arc<dyn ConfigManager>,
}

impl NetworkServiceDiscovery {
    pub fn new(config_manager: Arc<dyn ConfigManager>) -> Self {
        Self { config_manager }
    }

    async fn load_config(&self) -> Result<crate::core::Config> {
        let path = self.config_manager.get_project_root().join("Actr.toml");
        if !path.exists() {
            return Err(anyhow!(
                "Actr.toml not found in current directory. Run 'actr init' first or ensure you are in an Actor-RTC project."
            ));
        }
        self.config_manager.load_config(&path).await
    }

    async fn fetch_entries(
        &self,
        filter: Option<&ServiceFilter>,
    ) -> Result<Vec<discovery_response::TypeEntry>> {
        let config = self.load_config().await?;
        let client = SignalingClient::new(
            config.signaling_url.to_string(),
            config.package.actr_type.clone(),
            config.realm.clone(),
        );
        client.discover_entries(filter).await
    }

    fn entry_to_service_info(entry: &discovery_response::TypeEntry) -> ServiceInfo {
        let uri = Self::build_actr_uri(&entry.actr_type);
        ServiceInfo {
            name: entry.actr_type.name.clone(),
            uri,
            version: Self::select_version(entry),
            fingerprint: entry.service_fingerprint.clone(),
            description: entry.description.clone(),
            methods: Vec::new(),
        }
    }

    fn build_actr_uri(actr_type: &ActrType) -> String {
        if actr_type.manufacturer.trim().is_empty() {
            format!("actr://{}/", actr_type.name)
        } else {
            format!("actr://{}+{}/", actr_type.manufacturer, actr_type.name)
        }
    }

    fn select_version(entry: &discovery_response::TypeEntry) -> String {
        entry
            .tags
            .iter()
            .find(|tag| tag.as_str() == "latest")
            .cloned()
            .or_else(|| entry.tags.first().cloned())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn parse_actr_uri(&self, uri: &str) -> Result<(Option<String>, String)> {
        let without_scheme = uri
            .strip_prefix("actr://")
            .ok_or_else(|| anyhow!("Invalid actr:// URI: {uri}"))?;
        let name_end = without_scheme
            .find(|c| ['/', '?'].contains(&c))
            .unwrap_or(without_scheme.len());
        let name = without_scheme[..name_end].trim();
        if name.is_empty() {
            return Err(anyhow!("Invalid actr:// URI: {uri}"));
        }

        if let Some((manufacturer, service_name)) = name.split_once('+') {
            let manufacturer = manufacturer.trim();
            let service_name = service_name.trim();
            if manufacturer.is_empty() || service_name.is_empty() {
                return Err(anyhow!("Invalid actr:// URI: {uri}"));
            }
            Ok((Some(manufacturer.to_string()), service_name.to_string()))
        } else {
            Ok((None, name.to_string()))
        }
    }

    fn matches_filter(entry: &discovery_response::TypeEntry, filter: &ServiceFilter) -> bool {
        if let Some(pattern) = &filter.name_pattern {
            let full_name = format!("{}+{}", entry.actr_type.manufacturer, entry.actr_type.name);
            let matches = if pattern.contains('+') {
                Self::matches_pattern(&full_name, pattern)
            } else {
                Self::matches_pattern(&entry.actr_type.name, pattern)
            };
            if !matches {
                return false;
            }
        }

        if let Some(version_range) = &filter.version_range
            && Self::select_version(entry) != *version_range
            && !entry.tags.iter().any(|tag| tag == version_range)
        {
            return false;
        }

        if let Some(tags) = &filter.tags {
            let has_all = tags.iter().all(|tag| entry.tags.iter().any(|t| t == tag));
            if !has_all {
                return false;
            }
        }

        true
    }

    fn matches_pattern(value: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        let segments: Vec<&str> = pattern.split('*').collect();
        if segments.len() == 1 {
            return value == pattern;
        }

        if !pattern.starts_with('*')
            && let Some(first) = segments.first()
            && !value.starts_with(first)
        {
            return false;
        }

        if !pattern.ends_with('*')
            && let Some(last) = segments.last()
            && !value.ends_with(last)
        {
            return false;
        }

        let mut search_start = 0;
        let end_limit = if !pattern.ends_with('*') {
            value
                .len()
                .saturating_sub(segments.last().unwrap_or(&"").len())
        } else {
            value.len()
        };

        for (index, segment) in segments.iter().enumerate() {
            if segment.is_empty() {
                continue;
            }
            if index == 0 && !pattern.starts_with('*') {
                search_start = segment.len();
                continue;
            }
            if index == segments.len() - 1 && !pattern.ends_with('*') {
                continue;
            }
            if let Some(found) = value[search_start..end_limit].find(segment) {
                search_start += found + segment.len();
            } else {
                return false;
            }
        }

        true
    }
}

#[async_trait]
impl ServiceDiscovery for NetworkServiceDiscovery {
    async fn discover_services(&self, filter: Option<&ServiceFilter>) -> Result<Vec<ServiceInfo>> {
        let entries = self.fetch_entries(filter).await?;
        let services = entries
            .into_iter()
            .filter(|entry| match filter {
                Some(filter) => Self::matches_filter(entry, filter),
                None => true,
            })
            .map(|entry| Self::entry_to_service_info(&entry))
            .collect();
        Ok(services)
    }

    async fn get_service_details(&self, uri: &str) -> Result<ServiceDetails> {
        let (manufacturer, name) = self.parse_actr_uri(uri)?;
        let entries = self.fetch_entries(None).await?;
        let entry = entries.into_iter().find(|entry| {
            entry.actr_type.name == name
                && manufacturer
                    .as_ref()
                    .map_or(true, |m| entry.actr_type.manufacturer == *m)
        });

        let entry = entry.ok_or_else(|| anyhow!("Service not found: {uri}"))?;
        let info = Self::entry_to_service_info(&entry);

        Ok(ServiceDetails {
            info,
            proto_files: Vec::new(),
            dependencies: Vec::new(),
        })
    }

    async fn check_service_availability(&self, uri: &str) -> Result<AvailabilityStatus> {
        let (manufacturer, name) = self.parse_actr_uri(uri)?;
        let entries = self.fetch_entries(None).await?;
        let available = entries.iter().any(|entry| {
            entry.actr_type.name == name
                && manufacturer
                    .as_ref()
                    .map_or(true, |m| entry.actr_type.manufacturer == *m)
        });

        Ok(AvailabilityStatus {
            is_available: available,
            last_seen: available.then(SystemTime::now),
            health: if available {
                HealthStatus::Healthy
            } else {
                HealthStatus::Unknown
            },
        })
    }

    async fn get_service_proto(&self, uri: &str) -> Result<Vec<ProtoFile>> {
        let _ = self.parse_actr_uri(uri)?;
        Err(anyhow!(
            "Proto export is not available via signaling discovery yet"
        ))
    }
}
