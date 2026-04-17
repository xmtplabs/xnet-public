use crate::migration::{MessageType, MessageTypeProgress, MigrationState};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct PromResponse {
    data: PromData,
}

#[derive(Debug, Deserialize)]
struct PromData {
    result: Vec<PromResult>,
}

#[derive(Debug, Deserialize)]
struct PromResult {
    metric: PromMetric,
    value: (f64, String),
}

#[derive(Debug, Deserialize)]
struct PromMetric {
    table: Option<String>,
}

fn parse_message_type(s: &str) -> Option<MessageType> {
    match s {
        "commit_messages" => Some(MessageType::CommitMessages),
        "group_messages" => Some(MessageType::GroupMessages),
        "key_packages" => Some(MessageType::KeyPackages),
        "inbox_log" => Some(MessageType::InboxLog),
        _ => None,
    }
}

pub async fn query_migration_progress(
    client: &reqwest::Client,
    prometheus_url: &str,
) -> anyhow::Result<MigrationState> {
    let pct_query = "clamp_max(clamp_min(100*(max by (table)(xmtp_migrator_destination_last_sequence_id)/clamp_min(max by (table)(xmtp_migrator_source_last_sequence_id),1)),0),100)";

    let resp = client
        .get(format!("{}/api/v1/query", prometheus_url))
        .query(&[("query", pct_query)])
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?
        .json::<PromResponse>()
        .await?;

    let mut message_types = BTreeMap::new();

    for result in &resp.data.result {
        let msg_type = match result.metric.table.as_deref().and_then(parse_message_type) {
            Some(t) => t,
            None => continue,
        };
        let percent: f64 = result.value.1.parse().unwrap_or(0.0);
        message_types.insert(
            msg_type,
            MessageTypeProgress {
                source_seq: 0,
                dest_seq: 0,
                percent,
            },
        );
    }

    Ok(MigrationState { message_types })
}

pub async fn query_migration_sequences(
    client: &reqwest::Client,
    prometheus_url: &str,
    state: &mut MigrationState,
) -> anyhow::Result<()> {
    let src_query = "max by (table)(xmtp_migrator_source_last_sequence_id)";
    let dest_query = "max by (table)(xmtp_migrator_destination_last_sequence_id)";

    let (src_resp, dest_resp) = tokio::try_join!(
        async {
            client
                .get(format!("{}/api/v1/query", prometheus_url))
                .query(&[("query", src_query)])
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await?
                .json::<PromResponse>()
                .await
                .map_err(anyhow::Error::from)
        },
        async {
            client
                .get(format!("{}/api/v1/query", prometheus_url))
                .query(&[("query", dest_query)])
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await?
                .json::<PromResponse>()
                .await
                .map_err(anyhow::Error::from)
        }
    )?;

    for result in &src_resp.data.result {
        if let Some(msg_type) = result.metric.table.as_deref().and_then(parse_message_type) {
            if let Some(entry) = state.message_types.get_mut(&msg_type) {
                entry.source_seq = result.value.1.parse::<f64>().unwrap_or(0.0) as u64;
            }
        }
    }

    for result in &dest_resp.data.result {
        if let Some(msg_type) = result.metric.table.as_deref().and_then(parse_message_type) {
            if let Some(entry) = state.message_types.get_mut(&msg_type) {
                entry.dest_seq = result.value.1.parse::<f64>().unwrap_or(0.0) as u64;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_prom_response() {
        let json = r#"{"data":{"resultType":"vector","result":[
            {"metric":{"table":"commit_messages"},"value":[1234567890.0,"78.8"]},
            {"metric":{"table":"group_messages"},"value":[1234567890.0,"100"]}
        ]}}"#;
        let r: PromResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.data.result.len(), 2);
        assert_eq!(r.data.result[0].metric.table.as_deref(), Some("commit_messages"));
        assert_eq!(r.data.result[0].value.1, "78.8");
    }

    #[test]
    fn parse_message_types() {
        assert_eq!(parse_message_type("commit_messages"), Some(MessageType::CommitMessages));
        assert_eq!(parse_message_type("group_messages"), Some(MessageType::GroupMessages));
        assert_eq!(parse_message_type("key_packages"), Some(MessageType::KeyPackages));
        assert_eq!(parse_message_type("inbox_log"), Some(MessageType::InboxLog));
        assert_eq!(parse_message_type("unknown_table"), None);
    }
}
