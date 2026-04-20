use crate::migration::MigrationState;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TableProgress {
    pub table: String,
    pub percent: f64,
}

impl TableProgress {
    pub fn from_migration(migration: &MigrationState) -> Vec<Self> {
        migration
            .message_types
            .iter()
            .map(|(kind, p)| Self {
                table: kind.label().to_string(),
                percent: p.percent,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationSnapshot {
    pub overall_percent: f64,
    pub tables: Vec<TableProgress>,
}

impl MigrationSnapshot {
    pub fn from_migration(migration: &MigrationState) -> Self {
        Self {
            overall_percent: migration.min_percent(),
            tables: TableProgress::from_migration(migration),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhaseEvent {
    /// Sent as the first frame on connection. Gives the subscriber current state.
    Connected {
        phase: String,
        timestamp_s: u64,
        cutover_ts_s: Option<u64>,
        domain: String,
        migration: MigrationSnapshot,
    },
    V3Running {
        timestamp_s: u64,
        cutover_ts_s: Option<u64>,
        domain: String,
    },
    AwaitingCutover {
        timestamp_s: u64,
        cutover_ts_s: u64,
        domain: String,
        seconds_until_cutover: u64,
    },
    CutoverStarted {
        timestamp_s: u64,
        domain: String,
    },
    Migrating {
        timestamp_s: u64,
        domain: String,
        tables: Vec<TableProgress>,
    },
    MigrationComplete {
        timestamp_s: u64,
        domain: String,
        duration_s: u64,
    },
    D14nActive {
        timestamp_s: u64,
        domain: String,
    },
    TeardownImminent {
        timestamp_s: u64,
        domain: String,
    },
}

/// Tracks which transitions have been emitted so we don't fire duplicates.
#[derive(Debug, Clone, Default)]
pub struct TransitionTracker {
    pub last_phase_kind: Option<crate::phase::PhaseKind>,
    pub v3_running_emitted: bool,
    pub cutover_started_emitted: bool,
    pub migration_complete_emitted: bool,
    /// Set when we first see any migration progress after cutover.
    pub migration_start_ns: Option<u64>,
}

/// Internal container name used to detect "v3 running".
pub const V3_CONTAINER: &str = "xnet-node";

/// Compute the set of events that should fire given the new state.
/// Updates `tracker` in place. Returns events in the order they should be emitted.
pub fn compute_transitions(
    tracker: &mut TransitionTracker,
    now_ns: u64,
    cutover_ns: Option<u64>,
    phase: &crate::phase::Phase,
    migration: &MigrationState,
    health: &crate::health::HealthMap,
    domain: &str,
) -> Vec<PhaseEvent> {
    const NS_PER_S: u64 = 1_000_000_000;
    let timestamp_s = now_ns / NS_PER_S;
    let kind = phase.kind();
    let mut events = Vec::new();

    // v3_running: fires once when v3 container first reports healthy, pre-cutover.
    if !tracker.v3_running_emitted {
        if let Some(c) = health.get(V3_CONTAINER) {
            if c.up {
                tracker.v3_running_emitted = true;
                events.push(PhaseEvent::V3Running {
                    timestamp_s,
                    cutover_ts_s: cutover_ns.map(|ns| ns / NS_PER_S),
                    domain: domain.to_string(),
                });
            }
        }
    }

    let prev_kind = tracker.last_phase_kind;
    if prev_kind != Some(kind) {
        tracker.last_phase_kind = Some(kind);

        match phase {
            crate::phase::Phase::AwaitingCutover { countdown_ns } => {
                if let Some(cutover) = cutover_ns {
                    events.push(PhaseEvent::AwaitingCutover {
                        timestamp_s,
                        cutover_ts_s: cutover / NS_PER_S,
                        domain: domain.to_string(),
                        seconds_until_cutover: countdown_ns / NS_PER_S,
                    });
                }
            }
            crate::phase::Phase::Migrating { .. } => {
                if !tracker.cutover_started_emitted {
                    tracker.cutover_started_emitted = true;
                    events.push(PhaseEvent::CutoverStarted {
                        timestamp_s,
                        domain: domain.to_string(),
                    });
                }
                tracker.migration_start_ns.get_or_insert(now_ns);
                events.push(PhaseEvent::Migrating {
                    timestamp_s,
                    domain: domain.to_string(),
                    tables: TableProgress::from_migration(migration),
                });
            }
            crate::phase::Phase::D14nActive { .. } => {
                // Reaching d14n means migration finished — emit completion once.
                if !tracker.migration_complete_emitted {
                    tracker.migration_complete_emitted = true;
                    let duration_s = cutover_ns
                        .map(|c| now_ns.saturating_sub(c) / NS_PER_S)
                        .unwrap_or(0);
                    events.push(PhaseEvent::MigrationComplete {
                        timestamp_s,
                        domain: domain.to_string(),
                        duration_s,
                    });
                }
                events.push(PhaseEvent::D14nActive {
                    timestamp_s,
                    domain: domain.to_string(),
                });
            }
            crate::phase::Phase::TeardownImminent => {
                events.push(PhaseEvent::TeardownImminent {
                    timestamp_s,
                    domain: domain.to_string(),
                });
            }
            crate::phase::Phase::Unknown => {}
        }
    }

    events
}

pub fn build_connected_event(
    now_ns: u64,
    cutover_ns: Option<u64>,
    phase: &crate::phase::Phase,
    migration: &MigrationState,
    domain: &str,
) -> PhaseEvent {
    const NS_PER_S: u64 = 1_000_000_000;
    PhaseEvent::Connected {
        phase: phase.api_name().to_string(),
        timestamp_s: now_ns / NS_PER_S,
        cutover_ts_s: cutover_ns.map(|ns| ns / NS_PER_S),
        domain: domain.to_string(),
        migration: MigrationSnapshot::from_migration(migration),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::ContainerHealth;
    use crate::migration::{MessageType, MessageTypeProgress, MigrationState};
    use crate::phase::{Phase, PhaseKind, TEARDOWN_OFFSET_NS};

    const S: u64 = 1_000_000_000;

    fn health_with_v3_up() -> crate::health::HealthMap {
        let mut h = crate::health::HealthMap::new();
        h.insert(
            V3_CONTAINER.to_string(),
            ContainerHealth {
                display_name: "xnet-node".to_string(),
                port: None,
                image_tag: "main".to_string(),
                up: true,
                state: "running".to_string(),
                status: "healthy".to_string(),
            },
        );
        h
    }

    fn migration_complete() -> MigrationState {
        let mut m = MigrationState::default();
        m.message_types.insert(
            MessageType::GroupMessages,
            MessageTypeProgress { source_seq: 100, dest_seq: 100, percent: 100.0 },
        );
        m
    }

    #[test]
    fn v3_running_fires_once() {
        let mut t = TransitionTracker::default();
        let empty = MigrationState::default();
        let health = health_with_v3_up();
        let phase = Phase::AwaitingCutover { countdown_ns: 1000 * S };

        let events = compute_transitions(&mut t, 0, Some(1000 * S), &phase, &empty, &health, "xmtp.run");
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::V3Running { .. })));
        assert!(t.v3_running_emitted);

        // Second call, same health — no duplicate v3_running.
        let events2 = compute_transitions(&mut t, 0, Some(1000 * S), &phase, &empty, &health, "xmtp.run");
        assert!(!events2.iter().any(|e| matches!(e, PhaseEvent::V3Running { .. })));
    }

    #[test]
    fn cutover_transition_fires_cutover_started_and_migrating() {
        let mut t = TransitionTracker::default();
        t.last_phase_kind = Some(PhaseKind::AwaitingCutover);
        let migration = MigrationState::default();
        let health = crate::health::HealthMap::new();
        let phase = Phase::Migrating { min_percent: 0.0 };

        let events = compute_transitions(&mut t, 2000 * S, Some(1000 * S), &phase, &migration, &health, "xmtp.run");
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::CutoverStarted { .. })));
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::Migrating { .. })));
        assert!(t.cutover_started_emitted);
    }

    #[test]
    fn d14n_transition_fires_migration_complete_with_duration() {
        let mut t = TransitionTracker::default();
        t.last_phase_kind = Some(PhaseKind::Migrating);
        t.cutover_started_emitted = true;
        let migration = migration_complete();
        let health = crate::health::HealthMap::new();
        let cutover_ns = 1000 * S;
        let now_ns = cutover_ns + 120 * S;
        let phase = Phase::D14nActive { countdown_ns: TEARDOWN_OFFSET_NS - 120 * S };

        let events = compute_transitions(&mut t, now_ns, Some(cutover_ns), &phase, &migration, &health, "xmtp.run");
        let complete = events.iter().find_map(|e| match e {
            PhaseEvent::MigrationComplete { duration_s, .. } => Some(*duration_s),
            _ => None,
        });
        assert_eq!(complete, Some(120));
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::D14nActive { .. })));
    }

    #[test]
    fn connected_event_serializes_with_type_tag() {
        let migration = MigrationState::default();
        let phase = Phase::AwaitingCutover { countdown_ns: 100 * S };
        let ev = build_connected_event(0, Some(1000 * S), &phase, &migration, "xmtp.run");
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"connected\""));
        assert!(json.contains("\"phase\":\"awaiting_cutover\""));
    }
}
