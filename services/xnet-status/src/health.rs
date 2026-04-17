use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct ContainerHealth {
    pub display_name: String,
    pub port: Option<u16>,
    pub state: String,
    pub status: String,
    pub up: bool,
    pub image_tag: String,
}

pub type HealthMap = BTreeMap<String, ContainerHealth>;

/// Derive a human-friendly display name from a container name.
/// e.g. "xnet-100" → "100", "xnet-gateway" → "gateway"
fn display_name_for(container_name: &str) -> String {
    container_name
        .strip_prefix("xnet-")
        .unwrap_or(container_name)
        .to_string()
}

/// Extract the tag from a Docker image reference.
/// e.g. "ghcr.io/xmtp/xmtpd:v1.3.0" → "v1.3.0", "myimage" → "latest"
fn parse_image_tag(image: &str) -> String {
    // Strip digest if present (e.g. "image@sha256:...")
    let without_digest = image.split('@').next().unwrap_or(image);
    // Tag is after the last ':', but only if it's after the last '/'
    // (to avoid matching registry port like "registry:5000/image")
    match without_digest.rfind(':') {
        Some(colon_pos) => {
            let after_last_slash = without_digest.rfind('/').unwrap_or(0);
            if colon_pos > after_last_slash {
                without_digest[colon_pos + 1..].to_string()
            } else {
                "latest".to_string()
            }
        }
        None => "latest".to_string(),
    }
}

pub async fn poll_docker_health(
    docker: &bollard::Docker,
) -> anyhow::Result<HealthMap> {
    use bollard::query_parameters::ListContainersOptions;
    use std::collections::HashMap;

    let opts = ListContainersOptions {
        all: true,
        filters: Some(HashMap::from([("name".to_string(), vec!["xnet-".to_string()])])),
        ..Default::default()
    };

    let containers = docker.list_containers(Some(opts)).await?;
    let mut health = HealthMap::new();

    for container in containers {
        let names = container.names.unwrap_or_default();
        for name in &names {
            let clean_name = name.trim_start_matches('/').to_string();
            if clean_name.starts_with("xnet-") {
                let state_enum = container.state;
                let up = state_enum == Some(bollard::models::ContainerSummaryStateEnum::RUNNING);
                let state = state_enum.map(|s| format!("{:?}", s).to_lowercase()).unwrap_or_default();
                let status = container.status.clone().unwrap_or_default();
                let image_tag = container
                    .image
                    .as_deref()
                    .map(parse_image_tag)
                    .unwrap_or_else(|| "unknown".to_string());
                let port = container
                    .ports
                    .as_ref()
                    .and_then(|ports| ports.iter().find_map(|p| p.public_port));
                let display_name = display_name_for(&clean_name);
                health.insert(clean_name, ContainerHealth {
                    display_name,
                    port,
                    state,
                    status,
                    up,
                    image_tag,
                });
            }
        }
    }

    Ok(health)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names() {
        assert_eq!(display_name_for("xnet-100"), "100");
        assert_eq!(display_name_for("xnet-gateway"), "gateway");
        assert_eq!(display_name_for("other"), "other");
    }

    #[test]
    fn image_tag_parsing() {
        for (input, expected) in [
            ("ghcr.io/xmtp/xmtpd:v1.3.0", "v1.3.0"),
            ("myimage:latest", "latest"),
            ("myimage", "latest"),
            ("registry:5000/myimage:v2", "v2"),
            ("registry:5000/myimage", "latest"),
            ("img@sha256:abc123", "latest"),
            ("img:v1@sha256:abc123", "v1"),
        ] {
            assert_eq!(parse_image_tag(input), expected, "input: {input}");
        }
    }
}
