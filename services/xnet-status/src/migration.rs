use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    CommitMessages,
    GroupMessages,
    KeyPackages,
    InboxLog,
}

impl MessageType {
    pub fn label(&self) -> &'static str {
        match self {
            MessageType::CommitMessages => "commit_messages",
            MessageType::GroupMessages => "group_messages",
            MessageType::KeyPackages => "key_packages",
            MessageType::InboxLog => "inbox_log",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageTypeProgress {
    pub source_seq: u64,
    pub dest_seq: u64,
    pub percent: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MigrationState {
    pub message_types: BTreeMap<MessageType, MessageTypeProgress>,
}

impl MigrationState {
    /// Whether the migrator has reported any progress yet.
    pub fn progress(&self) -> bool {
        !self.message_types.is_empty()
    }

    /// Lowest progress across all message types (0.0 if none yet).
    pub fn min_percent(&self) -> f64 {
        self.message_types
            .values()
            .map(|t| t.percent)
            .reduce(f64::min)
            .unwrap_or(0.0)
    }

    /// All message types at 100%.
    pub fn all_complete(&self) -> bool {
        self.progress() && self.min_percent() >= 100.0
    }

    pub fn done(&self, cutover_ns: u64, now_ns: u64) -> bool {
        if self.all_complete() {
            return true;
        }
        // No metrics from the migrator after 5 minutes — assume done.
        if !self.progress() {
            let minutes_past = now_ns.saturating_sub(cutover_ns) / (60 * 1_000_000_000);
            if minutes_past > 5 {
                return true;
            }
        }

        false
    }

    pub fn progress_bar(percent: f64) -> String {
        let pct = percent.clamp(0.0, 100.0) as u32;
        let filled = (pct / 5) as usize;
        let empty = 20 - filled;
        format!(
            "[{}{}]",
            "\u{2588}".repeat(filled),
            "\u{2591}".repeat(empty)
        )
    }

    pub fn bar_color(percent: f64) -> &'static str {
        if percent >= 100.0 {
            "#0f0"
        } else if percent >= 50.0 {
            "#ff0"
        } else {
            "#f66"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(progress: bool, all_complete: bool) -> MigrationState {
        if !progress {
            return MigrationState::default();
        }
        let pct = if all_complete { 100.0 } else { 50.0 };
        let mut message_types = BTreeMap::new();
        message_types.insert(
            MessageType::GroupMessages,
            MessageTypeProgress { source_seq: 100, dest_seq: if all_complete { 100 } else { 50 }, percent: pct },
        );
        MigrationState { message_types }
    }

    #[test]
    fn done_cases() {
        const S: u64 = 1_000_000_000;
        // (progress, all_complete, cutover_ns, now_ns, expected)
        let cases: Vec<(bool, bool, u64, u64, bool)> = vec![
            (true, true, 1000 * S, 1500 * S, true),     // all complete
            (true, false, 1000 * S, 1500 * S, false),   // incomplete
            (false, false, 1000 * S, 1360 * S, true),   // no progress, >5min past
            (false, false, 1000 * S, 1180 * S, false),  // no progress, <5min past
        ];
        for (progress, all_complete, cutover, now, expected) in cases {
            assert_eq!(ms(progress, all_complete).done(cutover, now), expected,
                "progress={progress} all_complete={all_complete} cutover={cutover} now={now}");
        }
    }

    #[test]
    fn min_percent_cases() {
        assert_eq!(MigrationState::default().min_percent(), 0.0);
        let mut m = MigrationState::default();
        m.message_types.insert(MessageType::GroupMessages, MessageTypeProgress { source_seq: 100, dest_seq: 80, percent: 80.0 });
        m.message_types.insert(MessageType::CommitMessages, MessageTypeProgress { source_seq: 100, dest_seq: 50, percent: 50.0 });
        assert_eq!(m.min_percent(), 50.0);
    }

    #[test]
    fn progress_bar_fill_counts() {
        for (pct, filled, empty) in [(0.0, 0, 20), (50.0, 10, 10), (100.0, 20, 0)] {
            let bar = MigrationState::progress_bar(pct);
            assert_eq!(bar.matches('\u{2588}').count(), filled);
            assert_eq!(bar.matches('\u{2591}').count(), empty);
        }
    }
}
