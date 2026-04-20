use serde::{Deserialize, Serialize};
use worker::*;

#[derive(Deserialize)]
struct StatusResponse {
    phase: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Phase unchanged; do nothing.
    NoOp,
    /// Phase changed but the new phase is not a trigger; just persist.
    UpdateOnly { to: String },
    /// Phase changed and new phase is a trigger; dispatch + persist.
    Dispatch { from: String, to: String },
}

/// Pure decision function — unit-tested without any network or env.
pub fn decide(previous: Option<&str>, current: &str, triggers: &[String]) -> Decision {
    if let Some(p) = previous {
        if p == current {
            return Decision::NoOp;
        }
    }
    let from = previous.unwrap_or("none").to_string();
    if triggers.iter().any(|t| t == current) {
        Decision::Dispatch {
            from,
            to: current.to_string(),
        }
    } else {
        Decision::UpdateOnly {
            to: current.to_string(),
        }
    }
}

fn parse_triggers(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if let Err(e) = run(&env).await {
        console_error!("phase-watcher failed: {e}");
    }
}

async fn run(env: &Env) -> Result<()> {
    let status_url = env.var("STATUS_URL")?.to_string();
    let triggers = parse_triggers(&env.var("TRIGGER_PHASES")?.to_string());

    let current = fetch_phase(&status_url).await?;

    let kv = env.kv("PHASE_STATE")?;
    let previous = kv.get("last_phase").text().await?;

    let action = decide(previous.as_deref(), &current, &triggers);
    console_log!(
        "phase check: previous={:?} current={} action={:?}",
        previous,
        current,
        action
    );

    match &action {
        Decision::NoOp => {}
        Decision::UpdateOnly { to } => {
            kv.put("last_phase", to)?.execute().await?;
        }
        Decision::Dispatch { from, to } => {
            dispatch_workflow(env, from, to).await?;
            kv.put("last_phase", to)?.execute().await?;
        }
    }
    Ok(())
}

async fn fetch_phase(status_url: &str) -> Result<String> {
    let req = Request::new(status_url, Method::Get)?;
    let mut resp = Fetch::Request(req).send().await?;
    let status_code = resp.status_code();
    if status_code < 200 || status_code >= 300 {
        return Err(Error::from(format!(
            "status endpoint returned HTTP {}",
            status_code
        )));
    }
    let body: StatusResponse = resp.json().await?;
    Ok(body.phase)
}

#[derive(Serialize)]
struct DispatchInputs<'a> {
    phase: &'a str,
    previous_phase: &'a str,
    triggered_at: String,
}

#[derive(Serialize)]
struct DispatchBody<'a> {
    #[serde(rename = "ref")]
    git_ref: &'a str,
    inputs: DispatchInputs<'a>,
}

async fn dispatch_workflow(env: &Env, from: &str, to: &str) -> Result<()> {
    let owner = env.var("GH_OWNER")?.to_string();
    let repo = env.var("GH_REPO")?.to_string();
    let workflow = env.var("GH_WORKFLOW")?.to_string();
    let git_ref = env.var("GH_REF")?.to_string();
    let token = env.secret("GITHUB_TOKEN")?.to_string();

    let url = format!(
        "https://api.github.com/repos/{}/{}/actions/workflows/{}/dispatches",
        owner, repo, workflow
    );
    let body = DispatchBody {
        git_ref: &git_ref,
        inputs: DispatchInputs {
            phase: to,
            previous_phase: from,
            triggered_at: Date::now().to_string(),
        },
    };
    let body_json = serde_json::to_string(&body)?;

    let headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", token))?;
    headers.set("Accept", "application/vnd.github+json")?;
    headers.set("X-GitHub-Api-Version", "2022-11-28")?;
    headers.set("User-Agent", "xnet-phase-watcher")?;
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(body_json.into()));

    let req = Request::new_with_init(&url, &init)?;
    let mut resp = Fetch::Request(req).send().await?;
    let status_code = resp.status_code();
    if status_code != 204 {
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::from(format!(
            "github dispatch failed: HTTP {} — {}",
            status_code, text
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn noop_on_same_phase() {
        let t = trigs(&["awaiting_cutover", "d14n_active"]);
        assert_eq!(decide(Some("awaiting_cutover"), "awaiting_cutover", &t), Decision::NoOp);
    }

    #[test]
    fn dispatch_on_first_seen_trigger_phase() {
        let t = trigs(&["awaiting_cutover"]);
        assert_eq!(
            decide(None, "awaiting_cutover", &t),
            Decision::Dispatch { from: "none".to_string(), to: "awaiting_cutover".to_string() }
        );
    }

    #[test]
    fn update_only_on_first_seen_non_trigger() {
        let t = trigs(&["awaiting_cutover"]);
        assert_eq!(
            decide(None, "unknown", &t),
            Decision::UpdateOnly { to: "unknown".to_string() }
        );
    }

    #[test]
    fn update_only_on_transition_to_non_trigger() {
        let t = trigs(&["awaiting_cutover", "d14n_active"]);
        assert_eq!(
            decide(Some("awaiting_cutover"), "migrating", &t),
            Decision::UpdateOnly { to: "migrating".to_string() }
        );
    }

    #[test]
    fn dispatch_on_transition_to_trigger() {
        let t = trigs(&["awaiting_cutover", "d14n_active"]);
        assert_eq!(
            decide(Some("migrating"), "d14n_active", &t),
            Decision::Dispatch { from: "migrating".to_string(), to: "d14n_active".to_string() }
        );
    }

    #[test]
    fn parse_triggers_trims_and_drops_empty() {
        let v = parse_triggers("  a , b ,, c,");
        assert_eq!(v, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert!(parse_triggers("").is_empty());
    }

    #[test]
    fn empty_trigger_list_never_dispatches() {
        let t: Vec<String> = vec![];
        assert_eq!(
            decide(None, "awaiting_cutover", &t),
            Decision::UpdateOnly { to: "awaiting_cutover".to_string() }
        );
    }
}
