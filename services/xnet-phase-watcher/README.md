# xnet-phase-watcher

Cloudflare Worker (Rust / `workers-rs`) that polls `https://migrate.xmtp.run/api/status` once per minute and fires a GitHub Actions `workflow_dispatch` when the `phase` field transitions into a configured trigger set.

Spec: `docs/superpowers/specs/2026-04-20-xnet-phase-watcher-worker.md`.

## Prerequisites

- `wrangler` (`npm i -g wrangler` or `npx wrangler`)
- Rust toolchain with `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`)
- `worker-build` (`cargo install -q worker-build` — `wrangler deploy` runs this automatically via the `build.command` in `wrangler.jsonc`)

## Deploy

```bash
cd services/xnet-phase-watcher

# 1. Create a KV namespace and paste the id into wrangler.jsonc
wrangler kv:namespace create PHASE_STATE

# 2. Set the GitHub token secret (fine-grained PAT with Actions: write on the target repo)
wrangler secret put GITHUB_TOKEN

# 3. (Optional) Adjust vars in wrangler.jsonc — STATUS_URL, GH_OWNER/REPO/WORKFLOW/REF, TRIGGER_PHASES.

# 4. Deploy
wrangler deploy
```

## Configuration

Vars (in `wrangler.jsonc`):

| Name | Default | Description |
|---|---|---|
| `STATUS_URL` | `https://migrate.xmtp.run/api/status` | Origin poll target |
| `GH_OWNER` | `xmtp` | GitHub repo owner |
| `GH_REPO` | `xmtp-metrics` | GitHub repo name |
| `GH_WORKFLOW` | `xnet-tests.yml` | Workflow filename or numeric ID |
| `GH_REF` | `main` | Ref to run the workflow on |
| `TRIGGER_PHASES` | `awaiting_cutover,d14n_active` | Comma-separated phases that fire dispatch |

Secrets:

| Name | Description |
|---|---|
| `GITHUB_TOKEN` | Bearer token for GitHub API (fine-grained PAT or App installation token) |

## Local test

Unit tests of the decision logic don't need wasm:

```bash
cargo test --target x86_64-unknown-linux-gnu
```

Full wasm build:

```bash
cargo install worker-build
worker-build --release
```

## Target workflow

The target workflow must declare `workflow_dispatch` inputs matching what we send:

```yaml
on:
  workflow_dispatch:
    inputs:
      phase:
        required: true
        type: string
      previous_phase:
        required: true
        type: string
      triggered_at:
        required: true
        type: string
```

## Tuning / ops notes

- Cron minimum is 1 min. For sub-minute detection you'd need a Durable Object with alarms; not worth it for xnet's 8h cycle.
- KV writes free tier = 1000/day. We only write on transitions, so normal operation is well under.
- GitHub dispatch returns 204 on success; anything else fails the tick and we don't persist — next cron retries. At-least-once.
