use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct StatusConfig {
    pub status: StatusSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusSection {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_prometheus_url")]
    pub prometheus_url: String,
    #[serde(default = "default_docker_socket")]
    pub docker_socket: String,
    #[serde(default)]
    pub cutover_env_path: Option<PathBuf>,
    pub server: ServerInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub ip: String,
    pub domain: String,
    pub region: String,
    pub server_type: String,
    #[serde(default)]
    pub use_tls: bool,
}

fn default_listen() -> String {
    "0.0.0.0:8899".to_string()
}

fn default_prometheus_url() -> String {
    "http://localhost:9090".to_string()
}

fn default_docker_socket() -> String {
    "/var/run/docker.sock".to_string()
}

impl StatusConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: StatusConfig = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[status]
listen = "0.0.0.0:8899"
prometheus_url = "http://localhost:9090"
cutover_env_path = "/etc/xnet/cutover-env"
[status.server]
ip = "5.78.25.67"
domain = "xmtp.run"
region = "hil (Hillsboro, OR)"
server_type = "cpx51"
use_tls = true
"#;

    const MINIMAL: &str = r#"
[status]
[status.server]
ip = "1.2.3.4"
domain = "test.run"
region = "test"
server_type = "cx21"
"#;

    #[test]
    fn parse_full_config() {
        let c: StatusConfig = toml::from_str(FULL).unwrap();
        assert_eq!(c.status.server.ip, "5.78.25.67");
        assert!(c.status.server.use_tls);
        assert_eq!(c.status.cutover_env_path.unwrap().to_str().unwrap(), "/etc/xnet/cutover-env");
    }

    #[test]
    fn defaults_applied() {
        let c: StatusConfig = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.status.listen, "0.0.0.0:8899");
        assert_eq!(c.status.prometheus_url, "http://localhost:9090");
        assert_eq!(c.status.docker_socket, "/var/run/docker.sock");
        assert!(c.status.cutover_env_path.is_none());
    }
}
