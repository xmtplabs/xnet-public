{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.xnet-status;
  xnetCfg = config.services.xnet;

  logo = pkgs.fetchurl {
    url = "https://raw.githubusercontent.com/xmtp/libxmtp/main/apps/xnet/gui/assets/logo.png";
    sha256 = "sha256-+X/sYIPIec7MN2/xFh08I3yE3fEuMq5vllqS+ZpzWmA=";
  };

  logoBase64 = builtins.readFile (
    pkgs.runCommand "logo-base64" { } ''
      ${pkgs.coreutils}/bin/base64 -w0 ${logo} > $out
    ''
  );

  services = [
    { name = "xmtpd"; port = 5050; }
    { name = "node-go (v3)"; port = 5556; }
    { name = "gateway"; port = 5052; }
    { name = "validation"; port = 50051; }
    { name = "contracts"; port = 8545; }
    { name = "history"; port = 5558; }
  ];

  dashboards = [
    { name = "Grafana"; subdomain = "grafana"; }
    { name = "Prometheus"; subdomain = "prometheus"; }
    { name = "Otterscan"; subdomain = "otterscan"; }
    { name = "pgAdmin"; subdomain = "pgadmin"; }
  ];

  endpoints = [
    { label = "node-go (v3)"; sslipName = "node-go"; port = null; }
    { label = "xmtpd (d14n)"; sslipName = "xnet-100"; port = null; }
    { label = "gateway"; sslipName = "gateway"; port = null; }
  ];

  serviceRows = lib.concatMapStringsSep "\n" (svc: ''
    <div class="service-row">
      <span class="service-name">${svc.name}</span>
      <span class="service-port">:${toString svc.port}</span>
      <span class="service-status" data-port="${toString svc.port}">
        <span class="dot unknown"></span>
        <span class="status-text" style="color:#555">CHECKING...</span>
      </span>
    </div>
  '') services;

  dashboardRows = lib.concatMapStringsSep "\n" (d: ''
    <div class="link-row">
      <span class="link-name">${d.name}</span>
      <a class="link-url dashboard-link" href="#" data-subdomain="${d.subdomain}">loading...</a>
    </div>
  '') dashboards;

  endpointRows = lib.concatMapStringsSep "\n" (ep: ''
    <div class="endpoint-row">
      <span class="endpoint-name">${ep.label}</span>
      <span class="endpoint-url" ${if ep.sslipName != null then ''data-name="${ep.sslipName}"'' else ''data-port="${toString ep.port}"''}>loading...</span>
      <button class="copy-btn" onclick="copyEndpoint(this)" title="Copy">&#9112;</button>
    </div>
  '') endpoints;

  cssContent = builtins.readFile ./style.css;
  jsContent = builtins.readFile ./script.js;

  # Write substitution values to files to avoid "Argument list too long"
  logoFile = pkgs.writeText "logo-b64" logoBase64;
  cssFile = pkgs.writeText "style.css" cssContent;
  jsFile = pkgs.writeText "script.js" jsContent;
  serviceRowsFile = pkgs.writeText "service-rows" serviceRows;
  dashboardRowsFile = pkgs.writeText "dashboard-rows" dashboardRows;
  endpointRowsFile = pkgs.writeText "endpoint-rows" endpointRows;

  statusPage = pkgs.runCommand "xnet-status-page" { } ''
    mkdir -p $out
    cp ${./index.html} $out/index.html
    chmod +w $out/index.html

    substituteInPlace $out/index.html \
      --replace-fail '@logoBase64@' "$(cat ${logoFile})" \
      --replace-fail '@cssContent@' "$(cat ${cssFile})" \
      --replace-fail '@jsContent@' "$(cat ${jsFile})" \
      --replace-fail '@serviceRows@' "$(cat ${serviceRowsFile})" \
      --replace-fail '@dashboardRows@' "$(cat ${dashboardRowsFile})" \
      --replace-fail '@endpointRows@' "$(cat ${endpointRowsFile})" \
      --replace-fail '@xmtpdVersion@' '${xnetCfg.settings.xmtpd.version}' \
      --replace-fail '@v3Version@' '${xnetCfg.settings.v3.version}' \
      --replace-fail '@contractsVersion@' '${xnetCfg.settings.contracts.version}' \
      --replace-fail '@remoteIp@' '${if cfg.serverIp != null then cfg.serverIp else if xnetCfg.settings.remote_ip != null then toString xnetCfg.settings.remote_ip else ""}' \
      --replace-fail '@remoteDomain@' '${if xnetCfg.settings.remote_domain != null then xnetCfg.settings.remote_domain else ""}' \
      --replace-fail '@traefikPort@' '${if xnetCfg.settings.traefik.port != null then toString xnetCfg.settings.traefik.port else "80"}' \
      --replace-fail '@region@' '${cfg.region}' \
      --replace-fail '@serverType@' '${cfg.serverType}'
  '';

  statusPageConfig = ''
    handle /api/health {
      root * /tmp/xnet
      rewrite * /health.json
      file_server
      header Content-Type application/json
    }
    handle /cutover-env {
      root * /etc/xnet
      rewrite * /cutover-env
      file_server
    }
    handle {
      root * ${statusPage}
      file_server
    }
  '';

in
{
  options.services.xnet-status = {
    enable = lib.mkEnableOption "xnet status page";
    domain = lib.mkOption {
      type = lib.types.str;
      default = "localhost";
      description = "Domain name for the status page virtual host";
    };
    acmeEmail = lib.mkOption {
      type = lib.types.str;
      default = "ops@xmtp.com";
      description = "Email address for Let's Encrypt certificate notifications";
    };
    serverIp = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Server IP for dashboard links (port-based services)";
    };
    region = lib.mkOption {
      type = lib.types.str;
      default = "unknown";
      description = "Server region displayed on status page";
    };
    serverType = lib.mkOption {
      type = lib.types.str;
      default = "unknown";
      description = "Server type displayed on status page";
    };
  };

  config = lib.mkIf cfg.enable {
    # Status page served via Docker container on xnet network
    # so Traefik can reach it directly
    systemd.services.xnet-status-server = {
      description = "xnet status page server";
      after = [ "xnet.service" "docker.service" ];
      wants = [ "xnet.service" ];
      requires = [ "docker.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        SupplementaryGroups = [ "docker" ];
      };
      path = [ pkgs.docker ];
      script = let
        caddyfile = pkgs.writeText "status-Caddyfile" ''
          :8899 {
            handle /api/health {
              root * /tmp/xnet
              rewrite * /health.json
              file_server
              header Content-Type application/json
            }
            handle /api/migration {
              root * /tmp/xnet
              rewrite * /migration.json
              file_server
              header Content-Type application/json
            }
            handle /cutover-env {
              root * /etc/xnet
              rewrite * /cutover-env
              file_server
            }
            handle {
              root * /srv
              file_server
            }
          }
        '';
      in ''
        # Copy status page to a volume dir
        mkdir -p /tmp/xnet/status
        cp ${statusPage}/index.html /tmp/xnet/status/index.html
        cp ${caddyfile} /tmp/xnet/status/Caddyfile

        # Remove old container if exists
        docker rm -f xnet-status 2>/dev/null || true

        # Run Caddy with our config on xnet network
        docker run -d \
          --name xnet-status \
          --network xnet \
          --restart unless-stopped \
          -v /tmp/xnet/status/index.html:/srv/index.html:ro \
          -v /tmp/xnet/status/Caddyfile:/etc/caddy/Caddyfile:ro \
          -v /etc/xnet:/etc/xnet:ro \
          -v /tmp/xnet:/tmp/xnet:ro \
          caddy:2-alpine
      '';
      preStop = ''
        docker rm -f xnet-status 2>/dev/null || true
      '';
    };

    # Health check writer — polls Docker container status every 10s
    systemd.services.xnet-health = {
      description = "Write xnet service health to JSON";
      after = [ "xnet.service" "docker.service" ];
      wants = [ "xnet.service" ];
      serviceConfig = {
        Type = "oneshot";
        SupplementaryGroups = [ "docker" ];
      };
      path = [ pkgs.docker pkgs.jq pkgs.curl ];
      script = ''
        mkdir -p /tmp/xnet

        # Container health
        docker ps --format '{"name":"{{.Names}}","status":"{{.Status}}","state":"{{.State}}"}' \
          | jq -s 'map(select(.name | startswith("xnet-")))
                   | map({(.name): {status: .status, state: .state, up: (.state == "running")}})
                   | add // {}' \
          > /tmp/xnet/health.json.tmp
        mv /tmp/xnet/health.json.tmp /tmp/xnet/health.json

        # Migration progress — query prometheus for per-table percentage
        QUERY='clamp_max(clamp_min(100*(max by (table)(xmtp_migrator_destination_last_sequence_id)/clamp_min(max by (table)(xmtp_migrator_source_last_sequence_id),1)),0),100)'
        RESULT=$(curl -sf "http://localhost:9090/api/v1/query" --data-urlencode "query=$QUERY" 2>/dev/null || echo '{"data":{"result":[]}}')
        echo "$RESULT" | jq '{
          tables: [.data.result[] | {table: .metric.table, percent: (.value[1] | tonumber)}],
          all_complete: ([.data.result[] | .value[1] | tonumber] | min // 0) >= 100,
          min_percent: ([.data.result[] | .value[1] | tonumber] | min // 0),
          has_data: (.data.result | length) > 0
        }' > /tmp/xnet/migration.json.tmp 2>/dev/null || echo '{"tables":[],"all_complete":false,"min_percent":0,"has_data":false}' > /tmp/xnet/migration.json.tmp
        mv /tmp/xnet/migration.json.tmp /tmp/xnet/migration.json
      '';
    };

    systemd.timers.xnet-health = {
      description = "Poll xnet service health";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "30s";
        OnUnitActiveSec = "2s";
        AccuracySec = "1s";
      };
    };

    # Route domains to services via Traefik
    services.xnet.settings.extraTraefikRoutes = [
      { name = "status-page"; rule = "Host(`${cfg.domain}`)"; url = "http://xnet-status:8899"; priority = 100; }
      { name = "status-page-fallback"; rule = "PathPrefix(`/`)"; url = "http://xnet-status:8899"; priority = 1; }
      { name = "grafana"; rule = "Host(`grafana.xmtp.run`)"; url = "http://xnet-grafana:3000"; priority = null; }
      { name = "prometheus"; rule = "Host(`prometheus.xmtp.run`)"; url = "http://xnet-prometheus:9090"; priority = null; }
      { name = "otterscan"; rule = "Host(`otterscan.xmtp.run`)"; url = "http://xnet-otterscan:80"; priority = null; }
      { name = "pgadmin"; rule = "Host(`pgadmin.xmtp.run`)"; url = "http://xnet-pgadmin:80"; priority = null; }
      { name = "gateway"; rule = "Host(`gateway.xmtp.run`)"; url = "h2c://xnet-toxiproxy:5052"; priority = null; }
      { name = "node-go"; rule = "Host(`node-go.xmtp.run`)"; url = "h2c://xnet-toxiproxy:5556"; priority = null; }
    ];
  };
}
