# Ephemeral Xnet Deploy Tool

[![xnet-check](https://github.com/xmtplabs/xnet-public/actions/workflows/check.yml/badge.svg)](https://github.com/xmtplabs/xnet-public/actions/workflows/check.yml)
[![xnet-start](https://github.com/xmtplabs/xnet-public/actions/workflows/start.yml/badge.svg)](https://github.com/xmtplabs/xnet-public/actions/workflows/start.yml)
[![xnet-teardown](https://github.com/xmtplabs/xnet-public/actions/workflows/teardown.yml/badge.svg)](https://github.com/xmtplabs/xnet-public/actions/workflows/teardown.yml)

Deploys an ephemeral [xnet](https://github.com/xmtp/xmtpd) instance to Hetzner for testing the v3 to d14n migration. Provisions a full network stack, runs for ~8 hours, executes the cutover, then tears down and repeats.

Live status: [migrate.xmtp.run](http://migrate.xmtp.run)

## Schedule

3 cycles/day, 8 hours each, cutover at +4h into each cycle.

![Schedule](assets/schedule.png)

## Development

### Prerequisites

- [Nix](https://nixos.org/download.html) with flakes enabled
- SSH key registered on the Hetzner account (default name: `insipx-hetzner`)
- `HCLOUD_TOKEN` environment variable set

### Enter nix environment

```bash
nix develop
```

This provides `nixos-anywhere`, `jq`, and `hcloud`.

### Test in QEMU VM

```bash
nix run .#vm
```

Requires `sudo` for port 80 binding. Status page accessible at `http://localhost:8080`. Cutover set to 5 minutes from boot.

### Provision

```bash
SSH_KEY_PATH=~/.ssh/your_key ./dev/provision
```

| Variable | Default | Description |
|---|---|---|
| `SSH_KEY_PATH` | *required* | Path to SSH private key (must match key on Hetzner account) |
| `CUTOVER_DELAY_MINUTES` | `240` (4h) | Minutes after provision before cutover triggers |
| `SLACK_WEBHOOK_URL` | *(optional)* | Slack incoming webhook for migration notifications |
| `LOCATION` | `hil` | Hetzner datacenter location |
| `SERVER_TYPE` | `cpx51` | Hetzner server type |

Example with 15-minute cutover for testing:

```bash
SSH_KEY_PATH=~/.ssh/hetzner_id CUTOVER_DELAY_MINUTES=15 ./dev/provision
```

### Teardown

```bash
SSH_KEY_PATH=~/.ssh/your_key ./dev/teardown
```

Skip log collection:

```bash
NO_LOGS=true SSH_KEY_PATH=~/.ssh/your_key ./dev/teardown
```

### GitHub Actions

Workflows run automatically on a cron schedule. Trigger manually:

```bash
gh workflow run start.yml
gh workflow run teardown.yml
```
