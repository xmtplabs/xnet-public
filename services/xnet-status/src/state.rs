use crate::config::StatusSection;
use crate::events::{PhaseEvent, TransitionTracker};
use crate::health::{self, HealthMap};
use crate::migration::MigrationState;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Broadcast channel capacity. Only transitions flow through this channel,
/// so 32 is plenty. Lagging subscribers get dropped and are expected to reconnect.
pub const PHASE_CHANNEL_CAPACITY: usize = 32;

pub struct AppState {
    pub container_health: RwLock<HealthMap>,
    pub migration_progress: RwLock<MigrationState>,
    pub cutover_ns: Option<u64>,
    pub config: StatusSection,
    pub phase_tx: broadcast::Sender<PhaseEvent>,
    pub transition_tracker: RwLock<TransitionTracker>,
}

impl AppState {
    pub fn new(config: StatusSection) -> Arc<Self> {
        let cutover_ts = config
            .cutover_env_path
            .as_ref()
            .and_then(|path| read_cutover_timestamp(path));

        let (phase_tx, _) = broadcast::channel(PHASE_CHANNEL_CAPACITY);

        Arc::new(Self {
            container_health: RwLock::new(HealthMap::new()),
            migration_progress: RwLock::new(MigrationState::default()),
            cutover_ns: cutover_ts,
            config,
            phase_tx,
            transition_tracker: RwLock::new(TransitionTracker::default()),
        })
    }
}

fn read_cutover_timestamp(path: &std::path::Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("XNET_CUTOVER_TIMESTAMP=") {
            return val.trim().parse().ok();
        }
    }
    None
}

pub fn spawn_background_tasks(state: Arc<AppState>) {
    let docker_state = state.clone();
    tokio::spawn(async move {
        poll_docker_loop(docker_state).await;
    });

    let prom_state = state.clone();
    tokio::spawn(async move {
        poll_migration_loop(prom_state).await;
    });

    let phase_state = state.clone();
    tokio::spawn(async move {
        detect_phase_transitions_loop(phase_state).await;
    });
}

async fn poll_docker_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    let mut docker: Option<bollard::Docker> = None;
    loop {
        interval.tick().await;
        let client = match &docker {
            Some(d) => d,
            None => {
                match bollard::Docker::connect_with_unix(
                    &state.config.docker_socket,
                    120,
                    bollard::API_DEFAULT_VERSION,
                ) {
                    Ok(d) => {
                        docker = Some(d);
                        docker.as_ref().unwrap()
                    }
                    Err(e) => {
                        tracing::warn!("failed to connect to Docker, will retry: {}", e);
                        continue;
                    }
                }
            }
        };
        match health::poll_docker_health(client).await {
            Ok(health) => {
                *state.container_health.write().await = health;
            }
            Err(e) => {
                tracing::warn!("docker health poll failed, reconnecting: {}", e);
                docker = None;
            }
        }
    }
}

async fn poll_migration_loop(state: Arc<AppState>) {
    let client = reqwest::Client::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        match crate::prometheus::query_migration_progress(
            &client,
            &state.config.prometheus_url,
        )
        .await
        {
            Ok(mut migration) => {
                let _ = crate::prometheus::query_migration_sequences(
                    &client,
                    &state.config.prometheus_url,
                    &mut migration,
                )
                .await;
                *state.migration_progress.write().await = migration;
            }
            Err(e) => {
                tracing::warn!("migration poll failed: {}", e);
            }
        }
    }
}

/// Piggybacks on existing state polls to emit PhaseEvent transitions
/// onto the broadcast channel. Runs at 2s cadence — tighter than the
/// migration poll, but each tick is cheap (just enum comparison).
async fn detect_phase_transitions_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;

        let now_ns = crate::phase::now_ns();
        let migration = state.migration_progress.read().await.clone();
        let health = state.container_health.read().await.clone();
        let phase = crate::phase::compute_phase_at(now_ns, state.cutover_ns, &migration);
        let domain = state.config.server.domain.clone();

        let mut tracker = state.transition_tracker.write().await;
        let events = crate::events::compute_transitions(
            &mut tracker,
            now_ns,
            state.cutover_ns,
            &phase,
            &migration,
            &health,
            &domain,
        );
        drop(tracker);

        for event in events {
            // SendError only fires when there are no subscribers; harmless.
            let _ = state.phase_tx.send(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn read_cutover_timestamp_cases() {
        let p = write_tmp("xnet-test/cutover-env", "XNET_CUTOVER_TIMESTAMP=1776443504000000000\nOTHER=x\n");
        assert_eq!(read_cutover_timestamp(&p), Some(1776443504000000000));

        let p2 = write_tmp("xnet-test/no-key", "SOME_OTHER_KEY=123\n");
        assert_eq!(read_cutover_timestamp(&p2), None);

        assert_eq!(read_cutover_timestamp(std::path::Path::new("/nonexistent")), None);

        std::fs::remove_dir_all(std::env::temp_dir().join("xnet-test")).ok();
    }
}
