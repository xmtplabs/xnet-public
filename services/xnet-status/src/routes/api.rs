use crate::events::{build_connected_event, PhaseEvent};
use crate::migration::MigrationState;
use crate::phase;
use crate::state::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures::stream::{self, Stream, StreamExt};
use jiff::Timestamp;
use serde::Serialize;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/status", get(api_status))
        .route("/api/health", get(api_health))
        .route("/api/migration", get(api_migration))
        .route("/api/cutover", get(api_cutover))
        .route("/api/nodes", get(api_nodes))
        .route("/api/events", get(api_events))
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
    let migrator_ports: std::collections::BTreeSet<u16> =
        cfg.migrator_nodes.iter().map(|n| n.port).collect();

    let health = state.container_health.read().await.clone();

    let nodes: BTreeMap<String, NodeInfo> = health
        .iter()
        .filter_map(|(name, c)| {
            let id = name.strip_prefix("xnet-")?;
            if !id.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            let migrator = c.port.is_some_and(|p| migrator_ports.contains(&p));
            Some((
                id.to_string(),
                NodeInfo {
                    url: format!("{}://{}.{}", scheme, name, domain),
                    migrator,
                    healthy: c.up,
                },
            ))
        })
        .collect();

    Json(NodesResponse { nodes })
}

async fn api_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Build a `connected` event from current state so mid-lifecycle
    // subscribers know where we are before the next transition fires.
    let migration = state.migration_progress.read().await.clone();
    let now = phase::now_ns();
    let current_phase = phase::compute_phase_at(now, state.cutover_ns, &migration);
    let initial: PhaseEvent = build_connected_event(
        now,
        state.cutover_ns,
        &current_phase,
        &migration,
        &state.config.server.domain,
    );

    let rx = state.phase_tx.subscribe();
    let broadcast = BroadcastStream::new(rx).filter_map(|r| async move { r.ok() });

    let stream = stream::once(async move { initial })
        .chain(broadcast)
        .map(|event| {
            let data = serde_json::to_string(&event).unwrap_or_default();
            Ok(Event::default().event("phase").data(data))
        });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
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

/// End-to-end tests that exercise the HTTP surface — matches the coverage
/// promised in docs/superpowers/specs/2026-04-20-xnet-status-sse-events.md.
/// No Docker / Prometheus required; AppState is constructed in-process
/// with canned data and served via axum::Router::oneshot.
#[cfg(test)]
mod http_tests {
    use super::*;
    use crate::config::{ServerInfo, StatusSection};
    use crate::events::PhaseEvent;
    use crate::health::ContainerHealth;
    use crate::migration::{MessageType, MessageTypeProgress};
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        let state = AppState::new(StatusSection {
            listen: "127.0.0.1:0".to_string(),
            prometheus_url: "http://unused".to_string(),
            docker_socket: "/unused".to_string(),
            cutover_env_path: None,
            migrator_nodes: vec![],
            server: ServerInfo {
                domain: "xmtp.run".to_string(),
                region: "test".to_string(),
                server_type: "test".to_string(),
                use_tls: true,
            },
        });

        // Seed some container health so /api/health and /api/nodes have data.
        futures::executor::block_on(async {
            let mut h = state.container_health.write().await;
            h.insert(
                "xnet-100".to_string(),
                ContainerHealth {
                    display_name: "100".to_string(),
                    port: Some(5050),
                    state: "running".to_string(),
                    status: "Up 2 hours".to_string(),
                    up: true,
                    image_tag: "v1.3.0".to_string(),
                },
            );
            h.insert(
                "xnet-node".to_string(),
                ContainerHealth {
                    display_name: "node".to_string(),
                    port: Some(5556),
                    state: "running".to_string(),
                    status: "Up 2 hours".to_string(),
                    up: true,
                    image_tag: "main".to_string(),
                },
            );
        });

        // Seed a migration snapshot so /api/migration returns something.
        futures::executor::block_on(async {
            let mut m = state.migration_progress.write().await;
            m.message_types.insert(
                MessageType::GroupMessages,
                MessageTypeProgress { source_seq: 100, dest_seq: 42, percent: 42.0 },
            );
        });

        state
    }

    fn router(state: Arc<AppState>) -> axum::Router {
        axum::Router::new().merge(routes()).with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> Value {
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn status_returns_expected_shape() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/api/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        for field in ["phase", "cutover", "migration", "services", "versions", "endpoints", "dashboards"] {
            assert!(json.get(field).is_some(), "missing field: {field}");
        }
    }

    #[tokio::test]
    async fn health_lists_containers() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let obj = json.as_object().expect("health is an object");
        assert!(!obj.is_empty());
        for (_, v) in obj {
            assert!(v.get("state").is_some());
            assert!(v.get("up").is_some());
        }
    }

    #[tokio::test]
    async fn migration_shape() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/api/migration").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.get("message_types").is_some());
    }

    #[tokio::test]
    async fn cutover_shape() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/api/cutover").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.get("phase").is_some());
    }

    #[tokio::test]
    async fn nodes_shape() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/api/nodes").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let nodes = json.get("nodes").and_then(|v| v.as_object()).unwrap();
        // "xnet-100" → id "100"
        assert!(nodes.contains_key("100"));
    }

    /// Reads SSE frames from a body stream, returning fully-buffered frames.
    async fn read_sse_frames(body: Body, count: usize) -> Vec<String> {
        let mut stream = body.into_data_stream();
        let mut buffer = String::new();
        let mut frames = Vec::new();
        while frames.len() < count {
            match tokio::time::timeout(std::time::Duration::from_secs(2), stream.frame()).await {
                Ok(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        buffer.push_str(&String::from_utf8_lossy(&data));
                    }
                }
                _ => break,
            }
            while let Some(idx) = buffer.find("\n\n") {
                let frame = buffer[..idx].to_string();
                buffer.drain(..idx + 2);
                frames.push(frame);
                if frames.len() >= count {
                    break;
                }
            }
        }
        frames
    }

    fn parse_event_json(frame: &str) -> Option<Value> {
        let data = frame.lines().find(|l| l.starts_with("data: "))?;
        serde_json::from_str(&data[6..]).ok()
    }

    #[tokio::test]
    async fn events_emits_connected_first() {
        let app = router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/api/events").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );
        let frames = read_sse_frames(resp.into_body(), 1).await;
        assert!(!frames.is_empty(), "expected at least one frame");
        let event = parse_event_json(&frames[0]).expect("first frame should be JSON");
        assert_eq!(event["type"], "connected");
        assert!(event.get("phase").is_some());
        assert_eq!(event["domain"], "xmtp.run");
    }

    #[tokio::test]
    async fn events_broadcasts_transition() {
        let state = test_state();
        let app = router(state.clone());

        // Kick off the SSE request in the background so the subscription is live
        // by the time we publish.
        let handle = tokio::spawn(async move {
            let resp = app
                .oneshot(Request::builder().uri("/api/events").body(Body::empty()).unwrap())
                .await
                .unwrap();
            read_sse_frames(resp.into_body(), 2).await
        });

        // Give the handler a moment to subscribe before we publish.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let _ = state.phase_tx.send(PhaseEvent::V3Running {
            timestamp_s: 42,
            cutover_ts_s: Some(1000),
            domain: "xmtp.run".to_string(),
        });

        let frames = handle.await.unwrap();
        assert!(frames.len() >= 2, "expected connected + v3_running, got {}", frames.len());
        let second = parse_event_json(&frames[1]).expect("second frame is JSON");
        assert_eq!(second["type"], "v3_running");
        assert_eq!(second["timestamp_s"], 42);
    }

    #[tokio::test]
    async fn events_multi_subscriber() {
        let state = test_state();

        let spawn_sub = || {
            let app = router(state.clone());
            tokio::spawn(async move {
                let resp = app
                    .oneshot(Request::builder().uri("/api/events").body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                read_sse_frames(resp.into_body(), 2).await
            })
        };

        let a = spawn_sub();
        let b = spawn_sub();
        let c = spawn_sub();

        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = state.phase_tx.send(PhaseEvent::CutoverStarted {
            timestamp_s: 77,
            domain: "xmtp.run".to_string(),
        });

        for handle in [a, b, c] {
            let frames = handle.await.unwrap();
            assert!(frames.len() >= 2);
            let ev = parse_event_json(&frames[1]).unwrap();
            assert_eq!(ev["type"], "cutover_started");
            assert_eq!(ev["timestamp_s"], 77);
        }
    }
}
