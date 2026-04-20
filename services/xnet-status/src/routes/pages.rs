use crate::state::AppState;
use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/", get(index_page))
}

pub struct TzDisplay {
    pub label: String,
    pub time: String,
}

pub struct ServiceEntry {
    pub name: String,
    pub port: String,
}

pub struct EndpointEntry {
    pub label: String,
    pub url: String,
}

pub struct DashboardEntry {
    pub name: String,
    pub url: String,
}

pub struct VersionEntry {
    pub name: String,
    pub tag: String,
}

#[derive(Template)]
#[template(path = "base.html")]
struct BasePage {
    css: String,
    logo_base64: String,
    timezones: Vec<TzDisplay>,
    services: Vec<ServiceEntry>,
    endpoints: Vec<EndpointEntry>,
    dashboards: Vec<DashboardEntry>,
    domain: String,
    region: String,
    server_type: String,
    versions: Vec<VersionEntry>,
}

async fn index_page(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, StatusCode> {
    let css = include_str!("../../static/style.css").to_string();

    let logo_base64 = include_str!("../../static/logo.b64").trim().to_string();

    let cfg = &state.config;
    let health = state.container_health.read().await.clone();

    let timezones = build_timezones(state.cutover_ns);

    let services: Vec<ServiceEntry> = health
        .values()
        .map(|c| ServiceEntry {
            name: c.display_name.clone(),
            port: c.port.map(|p| format!(":{}", p)).unwrap_or_default(),
        })
        .collect();

    let scheme = if cfg.server.use_tls { "https" } else { "http" };
    let domain = &cfg.server.domain;

    let endpoints = vec![
        EndpointEntry {
            label: "node-go (v3)".to_string(),
            url: format!("{}://node-go.{}", scheme, domain),
        },
        EndpointEntry {
            label: "xmtpd (d14n)".to_string(),
            url: format!("{}://xnet-100.{}", scheme, domain),
        },
        EndpointEntry {
            label: "gateway".to_string(),
            url: format!("{}://gateway.{}", scheme, domain),
        },
    ];

    let dashboards = vec![
        DashboardEntry {
            name: "Grafana".to_string(),
            url: format!("{}://grafana.{}", scheme, domain),
        },
        DashboardEntry {
            name: "Prometheus".to_string(),
            url: format!("{}://prometheus.{}", scheme, domain),
        },
        DashboardEntry {
            name: "Otterscan".to_string(),
            url: format!("{}://otterscan.{}", scheme, domain),
        },
        DashboardEntry {
            name: "pgAdmin".to_string(),
            url: format!("{}://pgadmin.{}", scheme, domain),
        },
    ];

    let tmpl = BasePage {
        css,
        logo_base64,
        timezones,
        services,
        endpoints,
        dashboards,
        domain: cfg.server.domain.clone(),
        region: cfg.server.region.clone(),
        server_type: cfg.server.server_type.clone(),
        versions: build_versions(&health),
    };

    tmpl.render()
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn build_versions(health: &crate::health::HealthMap) -> Vec<VersionEntry> {
    [("xmtpd", "xnet-100"), ("node-go", "xnet-node"), ("contracts", "xnet-anvil")]
        .iter()
        .map(|(name, container)| VersionEntry {
            name: name.to_string(),
            tag: health
                .get(*container)
                .map(|c| c.image_tag.clone())
                .unwrap_or_else(|| "unknown".to_string()),
        })
        .collect()
}

fn build_timezones(cutover_ts: Option<u64>) -> Vec<TzDisplay> {
    match cutover_ts {
        None => vec![TzDisplay {
            label: "N/A".to_string(),
            time: "no cutover scheduled".to_string(),
        }],
        Some(ts) => crate::cutover::format_cutover_times(ts)
            .into_iter()
            .map(|(label, time)| TzDisplay { label, time })
            .collect(),
    }
}
