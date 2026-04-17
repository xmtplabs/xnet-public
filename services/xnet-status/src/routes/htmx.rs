use crate::migration::MigrationState;
use crate::phase;
use crate::state::AppState;
use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/htmx/phase", get(htmx_phase))
        .route("/htmx/health", get(htmx_health))
        .route("/htmx/migration", get(htmx_migration))
        .route("/htmx/timeline", get(htmx_timeline))
}

// --- Shared helpers ---

pub struct MigrationTableRow {
    pub name: String,
    pub bar: String,
    pub color: String,
    pub percent: String,
}

fn build_migration_rows(migration: &MigrationState) -> Vec<MigrationTableRow> {
    migration
        .message_types
        .iter()
        .map(|(msg_type, progress)| {
            MigrationTableRow {
                name: msg_type.label().to_string(),
                bar: MigrationState::progress_bar(progress.percent),
                color: MigrationState::bar_color(progress.percent).to_string(),
                percent: format!("{:.1}", progress.percent),
            }
        })
        .collect()
}

// --- Phase partial ---

#[derive(Template)]
#[template(path = "partials/phase.html")]
struct PhaseTemplate {
    phase_label: String,
    countdown_text: String,
    phase_description: String,
    show_migration: bool,
    has_migration_data: bool,
    migration_tables: Vec<MigrationTableRow>,
}

async fn htmx_phase(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, StatusCode> {
    let migration = state.migration_progress.read().await.clone();
    let current_phase = phase::compute_phase(state.cutover_ns, &migration);

    let countdown_text = match &current_phase {
        phase::Phase::AwaitingCutover { countdown_ns } => {
            phase::format_countdown(*countdown_ns)
        }
        phase::Phase::D14nActive { countdown_ns } => {
            phase::format_countdown(*countdown_ns)
        }
        phase::Phase::Migrating { min_percent } => {
            format!("{:.1}%", min_percent)
        }
        _ => "--:--:--".to_string(),
    };

    let show_migration = matches!(current_phase, phase::Phase::Migrating { .. });

    let migration_tables = build_migration_rows(&migration);

    let tmpl = PhaseTemplate {
        phase_label: current_phase.label().to_string(),
        countdown_text,
        phase_description: current_phase.description().to_string(),
        show_migration,
        has_migration_data: migration.progress(),
        migration_tables,
    };

    tmpl.render()
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// --- Health partial ---

pub struct ServiceRow {
    pub display_name: String,
    pub port: String,
    pub up: bool,
    pub state_text: String,
}

#[derive(Template)]
#[template(path = "partials/health.html")]
struct HealthTemplate {
    services: Vec<ServiceRow>,
}

async fn htmx_health(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, StatusCode> {
    let health = state.container_health.read().await.clone();

    let services: Vec<ServiceRow> = health
        .values()
        .map(|c| ServiceRow {
            display_name: c.display_name.clone(),
            port: c.port.map(|p| format!(":{}", p)).unwrap_or_default(),
            up: c.up,
            state_text: c.state.to_uppercase(),
        })
        .collect();

    let tmpl = HealthTemplate { services };
    tmpl.render()
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// --- Migration partial ---

#[derive(Template)]
#[template(path = "partials/migration.html")]
struct MigrationTemplate {
    has_progress: bool,
    tables: Vec<MigrationTableRow>,
}

async fn htmx_migration(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, StatusCode> {
    let migration = state.migration_progress.read().await.clone();

    let tables = build_migration_rows(&migration);

    let tmpl = MigrationTemplate {
        has_progress: migration.progress(),
        tables,
    };

    tmpl.render()
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// --- Timeline partial ---

#[derive(Template)]
#[template(path = "partials/timeline.html")]
struct TimelineTemplate {
    timeline_pct: String,
    pct: f64,
}

async fn htmx_timeline(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, StatusCode> {
    let now = phase::now_ns();

    let pct = match state.cutover_ns {
        Some(cutover_ts) => {
            let cycle_start = cutover_ts.saturating_sub(phase::TEARDOWN_OFFSET_NS);
            let cycle_end = cutover_ts + phase::TEARDOWN_OFFSET_NS;
            let cycle_duration = cycle_end - cycle_start;

            if now <= cycle_start {
                0.0
            } else if now >= cycle_end {
                100.0
            } else {
                let elapsed = now - cycle_start;
                (elapsed as f64 / cycle_duration as f64) * 100.0
            }
        }
        None => 0.0,
    };

    let tmpl = TimelineTemplate {
        timeline_pct: format!("{:.1}", pct),
        pct,
    };

    tmpl.render()
        .map(Html)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
