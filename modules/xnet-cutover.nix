{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.xnet;

  cutoverScript = pkgs.writeShellScript "xnet-cutover" ''
    set -euo pipefail
    export PATH="${lib.makeBinPath [ pkgs.curl pkgs.jq pkgs.bc ]}:$PATH"

    if [ ! -f /etc/xnet/cutover-env ]; then
      echo "No /etc/xnet/cutover-env found, skipping cutover"
      exit 0
    fi
    source /etc/xnet/cutover-env
    if [ -z "''${XNET_CUTOVER_TIMESTAMP:-}" ]; then
      echo "XNET_CUTOVER_TIMESTAMP not set, skipping cutover"
      exit 0
    fi

    NS=1000000000
    TS_S=$((XNET_CUTOVER_TIMESTAMP / NS))

    ${cfg.package}/bin/xnet-cli migrate -c $XNET_CUTOVER_TIMESTAMP -vvv
    echo ":: Cutover scheduled for:"
    echo "::   UTC: $(date -u -d @$TS_S '+%Y-%m-%d %H:%M')"
    echo "::   EST: $(TZ='America/New_York' date -d @$TS_S '+%Y-%m-%d %H:%M %Z')"
    echo "::   CST: $(TZ='America/Chicago' date -d @$TS_S '+%Y-%m-%d %H:%M %Z')"
    echo "::   PST: $(TZ='America/Los_Angeles' date -d @$TS_S '+%Y-%m-%d %H:%M %Z')"
    echo "::   CET: $(TZ='Europe/Berlin' date -d @$TS_S '+%Y-%m-%d %H:%M %Z')"
    echo "::   JST: $(TZ='Asia/Tokyo' date -d @$TS_S '+%Y-%m-%d %H:%M %Z')"

    NOW=$(date +%s%N)
    DELAY_NS=$((XNET_CUTOVER_TIMESTAMP - NOW))
    DELAY_S=$((DELAY_NS / NS))
    if [ "$DELAY_S" -gt 0 ]; then
      echo "Sleeping $DELAY_S seconds ($(( DELAY_S / 3600 ))h $(( (DELAY_S % 3600) / 60 ))m)..."
      sleep "$DELAY_S"
    else
      echo "Cutover time already passed ($(( -DELAY_S ))s ago), proceeding immediately"
    fi

    echo "Verifying cutover timestamp..."
    ACTUAL_TS=$(${cfg.package}/bin/xnet-cli cutover --unix --grpc-url http://localhost:5556)
    if [ "$ACTUAL_TS" != "$TS_S" ]; then
      echo "ERROR: timestamp mismatch! env=$TS_S xnet-cli=$ACTUAL_TS"
      exit 1
    fi
    echo "Timestamps match: $ACTUAL_TS"

    # Wait for migration to complete — all tables must reach 100%
    # If source has no data (seq_id = 0 or no metric), table counts as done
    echo ":: Waiting for v3 → d14n data migration to complete..."
    TABLES="commit_messages group_messages inbox_log key_packages welcome_messages"
    while true; do
      ALL_DONE=true
      for TABLE in $TABLES; do
        # Check if source has data
        SRC=$(curl -s --fail "http://localhost:9090/api/v1/query" \
          --data-urlencode "query=max(xmtp_migrator_source_last_sequence_id{table=\"$TABLE\"})" \
          | jq -r '.data.result[0].value[1] // "0"' 2>/dev/null || echo "0")
        if [ "$SRC" = "0" ] || [ -z "$SRC" ]; then
          echo "  $TABLE: no source data (done)"
          continue
        fi
        QUERY="clamp_max(clamp_min(100*(max(xmtp_migrator_destination_last_sequence_id{table=\"$TABLE\"})/clamp_min(max(xmtp_migrator_source_last_sequence_id{table=\"$TABLE\"}),1)),0),100)"
        PCT=$(curl -s --fail "http://localhost:9090/api/v1/query" --data-urlencode "query=$QUERY" \
          | jq -r '.data.result[0].value[1] // "0"' 2>/dev/null || echo "0")
        echo "  $TABLE: $PCT%"
        if [ "$(echo "$PCT < 100" | bc -l)" = "1" ]; then
          ALL_DONE=false
        fi
      done

      if [ "$ALL_DONE" = "true" ]; then
        echo ":: All tables migrated to 100%!"
        break
      fi
      echo ":: Waiting... (checking again in 10s)"
      sleep 10
    done

    echo "Activating d14n..."
    ${cfg.package}/bin/xnet-cli activate-d14n
    echo "d14n activated at $(date -u '+%Y-%m-%d %H:%M UTC')"
  '';

in
{
  config = lib.mkIf (cfg.enable && cfg.settings.migration.enable) {
    systemd.services.xnet-cutover = {
      description = "XMTP d14n cutover activation";
      after = [ "xnet.service" ];
      wants = [ "xnet.service" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = cutoverScript;
        SupplementaryGroups = [ "docker" ];
        TimeoutStartSec = "5h";
        Restart = "on-failure";
        RestartSec = 10;
      };
    };
  };
}
