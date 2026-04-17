use crate::migration::MigrationState;
use crate::phase;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use jiff::Timestamp;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/status", get(api_status))
        .route("/api/health", get(api_health))
        .route("/api/migration", get(api_migration))
        .route("/api/cutover", get(api_cutover))
        .route("/api/nodes", get(api_nodes))
}

#[derive(Serialize)]
struct StatusResponse {
    phase: String,
    cutover: CutoverInfo,
    migration: MigrationState,
    services: BTreeMap<String, ServiceStatus>,
    contracts: ContractsInfo,
    server: ServerInfoResponse,
    versions: VersionsResponse,
    endpoints: BTreeMap<String, String>,
    dashboards: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct ServiceStatus {
    state: String,
    status: String,
    up: bool,
}

#[derive(Serialize)]
struct CutoverInfo {
    timestamp_ns: Option<u64>,
    timestamp_s: Option<u64>,
    scheduled_utc: Option<String>,
    elapsed_since_cutover_s: Option<u64>,
    time_until_teardown_s: Option<u64>,
}

#[derive(Serialize)]
struct ContractsInfo {
    paused: bool,
}

#[derive(Serialize)]
struct ServerInfoResponse {
    ip: String,
    domain: String,
    region: String,
    #[serde(rename = "type")]
    server_type: String,
    tls: bool,
    uptime_s: u64,
}

#[derive(Serialize)]
struct VersionsResponse {
    xmtpd: String,
    node_go: String,
    contracts: String,
}

#[derive(Serialize)]
struct CutoverResponse {
    timestamp_ns: Option<u64>,
    timestamp_s: Option<u64>,
    phase: String,
    scheduled_utc: Option<String>,
    timezones: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct NodeInfo {
    url: String,
    migrator: bool,
    healthy: bool,
}

#[derive(Serialize)]
struct NodesResponse {
    nodes: BTreeMap<String, NodeInfo>,
}

fn build_endpoints(domain: &str, use_tls: bool) -> BTreeMap<String, String> {
    let scheme = if use_tls { "https" } else { "http" };
    let mut m = BTreeMap::new();
    m.insert(
        "xmtpd".to_string(),
        format!("{}://xnet-100.{}", scheme, domain),
    );
    m.insert(
        "node_go".to_string(),
        format!("{}://node-go.{}", scheme, domain),
    );
    m.insert(
        "gateway".to_string(),
        format!("{}://gateway.{}", scheme, domain),
    );
    m
}

fn build_dashboards(domain: &str, use_tls: bool) -> BTreeMap<String, String> {
    let scheme = if use_tls { "https" } else { "http" };
    let mut m = BTreeMap::new();
    m.insert(
        "grafana".to_string(),
        format!("{}://grafana.{}", scheme, domain),
    );
    m.insert(
        "prometheus".to_string(),
        format!("{}://prometheus.{}", scheme, domain),
    );
    m.insert(
        "otterscan".to_string(),
        format!("{}://otterscan.{}", scheme, domain),
    );
    m
}

fn build_versions(health: &crate::health::HealthMap) -> VersionsResponse {
    let tag = |name: &str| {
        health
            .get(name)
            .map(|c| c.image_tag.clone())
            .unwrap_or_else(|| "unknown".to_string())
    };
    VersionsResponse {
        xmtpd: tag("xnet-100"),
        node_go: tag("xnet-node"),
        contracts: tag("xnet-anvil"),
    }
}

const NS_PER_S: u64 = 1_000_000_000;

fn build_cutover_info(cutover_ns: Option<u64>, now_ns: u64) -> CutoverInfo {
    match cutover_ns {
        Some(ns) => {
            let teardown_ns = ns + phase::TEARDOWN_OFFSET_NS;
            let ts_s = ns / NS_PER_S;
            let scheduled_utc = Timestamp::from_nanosecond(ns as i128)
                .ok()
                .map(|t| t.strftime("%Y-%m-%dT%H:%M:%SZ").to_string());
            let elapsed = if now_ns >= ns {
                Some((now_ns - ns) / NS_PER_S)
            } else {
                None
            };
            let until_teardown = if now_ns < teardown_ns {
                Some((teardown_ns - now_ns) / NS_PER_S)
            } else {
                Some(0)
            };
            CutoverInfo {
                timestamp_ns: Some(ns),
                timestamp_s: Some(ts_s),
                scheduled_utc,
                elapsed_since_cutover_s: elapsed,
                time_until_teardown_s: until_teardown,
            }
        }
        None => CutoverInfo {
            timestamp_ns: None,
            timestamp_s: None,
            scheduled_utc: None,
            elapsed_since_cutover_s: None,
            time_until_teardown_s: None,
        },
    }
}

async fn api_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let now = phase::now_ns();
    let migration = state.migration_progress.read().await.clone();
    let health = state.container_health.read().await.clone();

    let current_phase = phase::compute_phase_at(now, state.cutover_ns, &migration);

    let services: BTreeMap<String, ServiceStatus> = health
        .iter()
        .map(|(name, c)| {
            (
                name.clone(),
                ServiceStatus {
                    up: c.up,
                    state: c.state.clone(),
                    status: c.status.clone(),
                },
            )
        })
        .collect();

    let cfg = &state.config;
    let endpoints = build_endpoints(&cfg.server.domain, cfg.server.use_tls);
    let dashboards = build_dashboards(&cfg.server.domain, cfg.server.use_tls);
    let cutover = build_cutover_info(state.cutover_ns, now);

    Json(StatusResponse {
        phase: current_phase.api_name().to_string(),
        cutover,
        migration,
        services,
        contracts: ContractsInfo { paused: false },
        server: ServerInfoResponse {
            ip: cfg.server.ip.clone(),
            domain: cfg.server.domain.clone(),
            region: cfg.server.region.clone(),
            server_type: cfg.server.server_type.clone(),
            tls: cfg.server.use_tls,
            uptime_s: 0,
        },
        versions: build_versions(&health),
        endpoints,
        dashboards,
    })
}

async fn api_health(
    State(state): State<Arc<AppState>>,
) -> Json<crate::health::HealthMap> {
    let health = state.container_health.read().await.clone();
    Json(health)
}

async fn api_migration(
    State(state): State<Arc<AppState>>,
) -> Json<MigrationState> {
    let migration = state.migration_progress.read().await.clone();
    Json(migration)
}

async fn api_cutover(State(state): State<Arc<AppState>>) -> Json<CutoverResponse> {
    let migration = state.migration_progress.read().await.clone();
    let current_phase = phase::compute_phase(state.cutover_ns, &migration);

    let (timestamp_ns, timestamp_s, scheduled_utc, timezones) =
        match state.cutover_ns {
            Some(ns) => {
                let ts_s = ns / NS_PER_S;
                let scheduled = Timestamp::from_nanosecond(ns as i128)
                    .ok()
                    .map(|t| t.strftime("%Y-%m-%dT%H:%M:%SZ").to_string());
                (
                    Some(ns),
                    Some(ts_s),
                    scheduled,
                    crate::cutover::format_cutover_times(ns).into_iter().collect(),
                )
            }
            None => (None, None, None, BTreeMap::new()),
        };

    Json(CutoverResponse {
        timestamp_ns,
        timestamp_s,
        phase: current_phase.api_name().to_string(),
        scheduled_utc,
        timezones,
    })
}

async fn api_nodes(State(state): State<Arc<AppState>>) -> Json<NodesResponse> {
    let cfg = &state.config;
    let domain = &cfg.server.domain;
    let scheme = if cfg.server.use_tls { "https" } else { "http" };

    let health = state.container_health.read().await.clone();

    let nodes: BTreeMap<String, NodeInfo> = health
        .iter()
        .filter_map(|(name, c)| {
            let id = name.strip_prefix("xnet-")?;
            if !id.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            Some((
                id.to_string(),
                NodeInfo {
                    url: format!("{}://{}.{}", scheme, name, domain),
                    migrator: false,
                    healthy: c.up,
                },
            ))
        })
        .collect();

    Json(NodesResponse { nodes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_and_dashboards() {
        let ep = build_endpoints("xmtp.run", true);
        assert_eq!(ep["xmtpd"], "https://xnet-100.xmtp.run");
        assert_eq!(ep["node_go"], "https://node-go.xmtp.run");
        assert_eq!(ep["gateway"], "https://gateway.xmtp.run");
        let db = build_dashboards("xmtp.run", true);
        assert_eq!(db["grafana"], "https://grafana.xmtp.run");
        assert_eq!(db["otterscan"], "https://otterscan.xmtp.run");
    }

    #[test]
    fn cutover_info() {
        let cutover_ns = 1_776_443_504_000_000_000u64;
        let now_ns = cutover_ns + 3600 * NS_PER_S;
        let info = build_cutover_info(Some(cutover_ns), now_ns);
        assert_eq!(info.timestamp_ns, Some(cutover_ns));
        assert_eq!(info.timestamp_s, Some(cutover_ns / NS_PER_S));
        assert_eq!(info.elapsed_since_cutover_s, Some(3600));
        assert!(info.time_until_teardown_s.unwrap() > 0);
        let none = build_cutover_info(None, 1000);
        assert!(none.timestamp_s.is_none());
    }
}
