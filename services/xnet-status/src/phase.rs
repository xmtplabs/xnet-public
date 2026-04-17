use crate::migration::MigrationState;
use serde::Serialize;

const NS_PER_S: u64 = 1_000_000_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum Phase {
    Unknown,
    AwaitingCutover { countdown_ns: u64 },
    Migrating { min_percent: f64 },
    D14nActive { countdown_ns: u64 },
    TeardownImminent,
}

impl Phase {
    pub fn label(&self) -> &'static str {
        match self {
            Phase::Unknown => "UNKNOWN",
            Phase::AwaitingCutover { .. } => "AWAITING CUTOVER",
            Phase::Migrating { .. } => "MIGRATING",
            Phase::D14nActive { .. } => "D14N ACTIVE",
            Phase::TeardownImminent => "TEARDOWN IMMINENT",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Phase::Unknown => "timestamp unavailable",
            Phase::AwaitingCutover { .. } => "until v3 \u{2192} d14n cutover",
            Phase::Migrating { .. } => "migrating v3 data to d14n...",
            Phase::D14nActive { .. } => "until teardown",
            Phase::TeardownImminent => "teardown imminent",
        }
    }

    pub fn api_name(&self) -> &'static str {
        match self {
            Phase::Unknown => "unknown",
            Phase::AwaitingCutover { .. } => "awaiting_cutover",
            Phase::Migrating { .. } => "migrating",
            Phase::D14nActive { .. } => "d14n_active",
            Phase::TeardownImminent => "teardown_imminent",
        }
    }
}

/// 4 hours from cutover to teardown, in nanoseconds.
pub const TEARDOWN_OFFSET_NS: u64 = 4 * 3600 * NS_PER_S;

pub fn compute_phase(
    cutover_ns: Option<u64>,
    migration: &MigrationState,
) -> Phase {
    compute_phase_at(now_ns(), cutover_ns, migration)
}

pub(crate) fn compute_phase_at(
    now: u64,
    cutover_ns: Option<u64>,
    migration: &MigrationState,
) -> Phase {
    let Some(cutover) = cutover_ns else {
        return Phase::Unknown;
    };
    let teardown_ns = cutover + TEARDOWN_OFFSET_NS;

    if now < cutover {
        Phase::AwaitingCutover {
            countdown_ns: cutover - now,
        }
    } else if !migration.done(cutover, now) {
        Phase::Migrating {
            min_percent: migration.min_percent(),
        }
    } else if now < teardown_ns {
        Phase::D14nActive {
            countdown_ns: teardown_ns - now,
        }
    } else {
        Phase::TeardownImminent
    }
}

/// Current unix timestamp in nanoseconds.
pub fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

/// Format nanoseconds as HH:MM:SS countdown.
pub fn format_countdown(ns: u64) -> String {
    let total_s = ns / NS_PER_S;
    let h = total_s / 3600;
    let m = (total_s % 3600) / 60;
    let s = total_s % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = NS_PER_S;

    #[test]
    fn phase_transitions() {
        let empty = MigrationState::default();
        let incomplete = {
            let mut m = MigrationState::default();
            m.message_types.insert(
                crate::migration::MessageType::GroupMessages,
                crate::migration::MessageTypeProgress { source_seq: 100, dest_seq: 42, percent: 42.5 },
            );
            m
        };
        let complete = {
            let mut m = MigrationState::default();
            m.message_types.insert(
                crate::migration::MessageType::GroupMessages,
                crate::migration::MessageTypeProgress { source_seq: 100, dest_seq: 100, percent: 100.0 },
            );
            m
        };

        assert_eq!(compute_phase_at(1000 * S, None, &empty), Phase::Unknown);
        assert_eq!(compute_phase_at(1000 * S, Some(2000 * S), &empty), Phase::AwaitingCutover { countdown_ns: 1000 * S });
        assert_eq!(compute_phase_at(1500 * S, Some(1000 * S), &incomplete), Phase::Migrating { min_percent: 42.5 });
        assert_eq!(compute_phase_at(2000 * S, Some(1000 * S), &complete), Phase::D14nActive { countdown_ns: 1000 * S + TEARDOWN_OFFSET_NS - 2000 * S });
        assert_eq!(compute_phase_at(1000 * S + TEARDOWN_OFFSET_NS + 1, Some(1000 * S), &complete), Phase::TeardownImminent);
    }

    #[test]
    fn countdown_formatting() {
        for (ns, expected) in [(0, "00:00:00"), (61 * S, "00:01:01"), (3661 * S, "01:01:01"), (86399 * S, "23:59:59")] {
            assert_eq!(format_countdown(ns), expected);
        }
    }

    #[test]
    fn labels() {
        for (phase, label) in [
            (Phase::Unknown, "UNKNOWN"),
            (Phase::AwaitingCutover { countdown_ns: 0 }, "AWAITING CUTOVER"),
            (Phase::Migrating { min_percent: 0.0 }, "MIGRATING"),
            (Phase::D14nActive { countdown_ns: 0 }, "D14N ACTIVE"),
            (Phase::TeardownImminent, "TEARDOWN IMMINENT"),
        ] {
            assert_eq!(phase.label(), label);
        }
    }
}
