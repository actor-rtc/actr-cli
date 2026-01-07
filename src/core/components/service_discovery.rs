use actr_config::Config;
use actr_protocol::{
    AIdCredential, ActrId, ActrToSignaling, ActrType, DiscoveryRequest, ErrorResponse,
    GetServiceSpecRequest, PeerToSignaling, RegisterRequest, ServiceSpec, SignalingEnvelope,
    actr_to_signaling, discovery_response, get_service_spec_response, peer_to_signaling,
    register_response, signaling_envelope, signaling_to_actr,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::time::SystemTime;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::core::{
    AvailabilityStatus, HealthStatus, ProtoFile, ServiceDetails, ServiceDiscovery, ServiceFilter,
    ServiceInfo,
};

type SignalingSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct SignalingState {
    socket: SignalingSocket,
    actr_id: ActrId,
    credential: AIdCredential,
}

pub struct NetworkServiceDiscovery {
    config: Config,
    state: Mutex<Option<SignalingState>>,
}

impl NetworkServiceDiscovery {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: Mutex::new(None),
        }
    }

    async fn ensure_connected(&self) -> Result<()> {
        let mut state_guard = self.state.lock().await;
        if state_guard.is_some() {
            return Ok(());
        }

        let state = self.connect_and_register().await?;
        *state_guard = Some(state);
        Ok(())
    }

    // TODO: add filter support
    async fn discover_entries(
        &self,
        _filter: Option<&ServiceFilter>,
    ) -> Result<Vec<discovery_response::TypeEntry>> {
        self.ensure_connected().await?;
        let mut state_guard = self.state.lock().await;
        let state = state_guard
            .as_mut()
            .context("Signaling state not initialized")?;

        // TODO: add filter support
        let request = DiscoveryRequest {
            manufacturer: None,
            limit: None,
        };
        let payload = actr_to_signaling::Payload::DiscoveryRequest(request);
        let envelope =
            Self::build_envelope(signaling_envelope::Flow::ActrToServer(ActrToSignaling {
                source: state.actr_id.clone(),
                credential: state.credential.clone(),
                payload: Some(payload),
            }))?;

        let result = match Self::send_envelope(&mut state.socket, envelope).await {
            Ok(()) => loop {
                let envelope = Self::read_envelope(&mut state.socket).await?;
                match envelope.flow {
                    Some(signaling_envelope::Flow::ServerToActr(server)) => match server.payload {
                        Some(signaling_to_actr::Payload::DiscoveryResponse(response)) => {
                            break Self::handle_discovery_response(response);
                        }
                        Some(signaling_to_actr::Payload::Error(error)) => {
                            break Err(Self::as_error("Discovery failed", &error));
                        }
                        _ => {}
                    },
                    Some(signaling_envelope::Flow::EnvelopeError(error)) => {
                        break Err(Self::as_error("Discovery failed", &error));
                    }
                    _ => {}
                }
            },
            Err(err) => Err(err),
        };
        if result.is_err() {
            *state_guard = None;
        }
        result
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

    /// Get ServiceSpec via GetServiceSpecRequest
    async fn get_service_spec(
        &self,
        actr_type: &ActrType,
        fingerprint: Option<&str>,
    ) -> Result<ServiceSpec> {
        self.ensure_connected().await?;
        let mut state_guard = self.state.lock().await;
        let state = state_guard
            .as_mut()
            .context("Signaling state not initialized")?;

        let request = GetServiceSpecRequest {
            actr_type: actr_type.clone(),
            fingerprint: fingerprint.map(|s| s.to_string()),
        };
        let payload = actr_to_signaling::Payload::GetServiceSpecRequest(request);
        let envelope =
            Self::build_envelope(signaling_envelope::Flow::ActrToServer(ActrToSignaling {
                source: state.actr_id.clone(),
                credential: state.credential.clone(),
                payload: Some(payload),
            }))?;

        let result = match Self::send_envelope(&mut state.socket, envelope).await {
            Ok(()) => loop {
                let envelope = Self::read_envelope(&mut state.socket).await?;
                match envelope.flow {
                    Some(signaling_envelope::Flow::ServerToActr(server)) => match server.payload {
                        Some(signaling_to_actr::Payload::GetServiceSpecResponse(response)) => {
                            break Self::handle_get_service_spec_response(response);
                        }
                        Some(signaling_to_actr::Payload::Error(error)) => {
                            break Err(Self::as_error("GetServiceSpec failed", &error));
                        }
                        _ => {}
                    },
                    Some(signaling_envelope::Flow::EnvelopeError(error)) => {
                        break Err(Self::as_error("GetServiceSpec failed", &error));
                    }
                    _ => {}
                }
            },
            Err(err) => Err(err),
        };
        if result.is_err() {
            *state_guard = None;
        }
        result
    }

    fn handle_get_service_spec_response(
        response: actr_protocol::GetServiceSpecResponse,
    ) -> Result<ServiceSpec> {
        match response.result {
            Some(get_service_spec_response::Result::Success(spec)) => Ok(spec),
            Some(get_service_spec_response::Result::Error(error)) => {
                Err(Self::as_error("GetServiceSpec failed", &error))
            }
            None => Err(anyhow!("GetServiceSpec response is missing result")),
        }
    }

    async fn connect_and_register(&self) -> Result<SignalingState> {
        let signaling_url = self.config.signaling_url.as_str();
        let (mut socket, _) = connect_async(signaling_url)
            .await
            .with_context(|| format!("Failed to connect to signaling: {signaling_url}"))?;

        let register_request = RegisterRequest {
            actr_type: self.config.package.actr_type.clone(),
            realm: self.config.realm,
            service_spec: None,
            acl: None,
        };

        let envelope =
            Self::build_envelope(signaling_envelope::Flow::PeerToServer(PeerToSignaling {
                payload: Some(peer_to_signaling::Payload::RegisterRequest(
                    register_request,
                )),
            }))?;

        Self::send_envelope(&mut socket, envelope).await?;

        let (actr_id, credential) = loop {
            let envelope = Self::read_envelope(&mut socket).await?;
            match envelope.flow {
                Some(signaling_envelope::Flow::ServerToActr(server)) => match server.payload {
                    Some(signaling_to_actr::Payload::RegisterResponse(response)) => {
                        match response.result {
                            Some(register_response::Result::Success(success)) => {
                                break (success.actr_id, success.credential);
                            }
                            Some(register_response::Result::Error(error)) => {
                                return Err(Self::as_error("Register failed", &error));
                            }
                            None => return Err(anyhow!("Register response is missing result")),
                        }
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
        };

        Ok(SignalingState {
            socket,
            actr_id,
            credential,
        })
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
        Ok(SignalingEnvelope {
            envelope_version: 1,
            envelope_id: uuid::Uuid::new_v4().to_string(),
            reply_for: None,
            timestamp: prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            },
            traceparent: None,
            tracestate: None,
            flow: Some(flow),
        })
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

        // Handle format: actr://realm:manufacturer+name@version/
        // or: actr://manufacturer+name/
        // or: actr://name/
        let name_end = without_scheme
            .find(|c| ['/', '?'].contains(&c))
            .unwrap_or(without_scheme.len());
        let mut name = without_scheme[..name_end].trim();

        // Remove @version suffix if present
        if let Some(at_pos) = name.find('@') {
            name = &name[..at_pos];
        }

        // Remove realm: prefix if present (e.g., "5:acme+EchoService" -> "acme+EchoService")
        if let Some(colon_pos) = name.find(':') {
            name = &name[colon_pos + 1..];
        }

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
        let entries = self.discover_entries(filter).await?;
        let services = entries
            .into_iter()
            .filter(|entry| match filter {
                Some(filter) => Self::matches_filter(entry, filter),
                None => true,
            })
            .map(ServiceInfo::from)
            .collect();
        Ok(services)
    }

    async fn get_service_details(&self, uri: &str) -> Result<ServiceDetails> {
        let (manufacturer, name) = self.parse_actr_uri(uri)?;
        let entries = self.discover_entries(None).await?;
        let entry = entries.into_iter().find(|entry| {
            entry.actr_type.name == name
                && manufacturer
                    .as_ref()
                    .is_none_or(|m| entry.actr_type.manufacturer == *m)
        });

        let entry = entry.ok_or_else(|| anyhow!("Service not found: {uri}"))?;
        let info = ServiceInfo::from(entry.clone());

        // Try to get ServiceSpec with proto files
        let proto_files = match self
            .get_service_spec(&entry.actr_type, Some(entry.service_fingerprint.as_str()))
            .await
        {
            Ok(spec) => spec
                .protobufs
                .into_iter()
                .map(|proto| ProtoFile {
                    name: proto.package.clone(),
                    path: std::path::PathBuf::from(format!("{}.proto", proto.package)),
                    content: proto.content,
                    services: Vec::new(),
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to get ServiceSpec for {uri}: {e}");
                Vec::new()
            }
        };

        Ok(ServiceDetails {
            info,
            proto_files,
            dependencies: Vec::new(),
        })
    }

    async fn check_service_availability(&self, uri: &str) -> Result<AvailabilityStatus> {
        let (manufacturer, name) = self.parse_actr_uri(uri)?;
        let entries = self.discover_entries(None).await?;
        let available = entries.iter().any(|entry| {
            entry.actr_type.name == name
                && manufacturer
                    .as_ref()
                    .is_none_or(|m| entry.actr_type.manufacturer == *m)
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
        let (manufacturer, name) = self.parse_actr_uri(uri)?;

        // Build ActrType for the request
        let actr_type = ActrType {
            manufacturer: manufacturer.unwrap_or_default(),
            name,
        };

        // Get ServiceSpec
        let spec = self.get_service_spec(&actr_type, None).await?;

        // Convert protobufs to ProtoFile
        let proto_files = spec
            .protobufs
            .into_iter()
            .map(|proto| ProtoFile {
                name: proto.package.clone(),
                path: std::path::PathBuf::from(format!("{}.proto", proto.package)),
                content: proto.content,
                services: Vec::new(),
            })
            .collect();

        Ok(proto_files)
    }
}
