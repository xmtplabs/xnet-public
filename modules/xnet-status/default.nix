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

  logoBase64 = pkgs.runCommand "logo-base64" { } ''
    ${pkgs.coreutils}/bin/base64 -w0 ${logo} > $out
  '';

  xnet-status-pkg = pkgs.rustPlatform.buildRustPackage {
    pname = "xnet-status";
    version = "0.1.0";
    src = ../../services/xnet-status;
    cargoLock.lockFile = ../../services/xnet-status/Cargo.lock;

    postInstall = ''
      mkdir -p $out/share/xnet-status/static
      cp ${../../services/xnet-status/static/style.css} $out/share/xnet-status/static/style.css
      cp ${logoBase64} $out/share/xnet-status/static/logo.b64
    '';
  };

  xnet-status-image = pkgs.dockerTools.buildImage {
    name = "xnet-status";
    tag = "latest";
    copyToRoot = pkgs.buildEnv {
      name = "xnet-status-root";
      paths = [
        xnet-status-pkg
        pkgs.cacert
        (pkgs.runCommand "xnet-status-config" { } ''
          mkdir -p $out/etc/xnet
          cp ${configFile} $out/etc/xnet/status.toml
        '')
      ];
    };
    config = {
      Cmd = [ "${xnet-status-pkg}/bin/xnet-status" "--config" "/etc/xnet/status.toml" ];
      WorkingDirectory = "${xnet-status-pkg}/share/xnet-status";
      Env = [
        "XNET_STATUS_SHARE=${xnet-status-pkg}/share/xnet-status"
      ];
      ExposedPorts = {
        "8899/tcp" = {};
      };
    };
  };

  configFile = pkgs.writeText "xnet-status.toml" ''
    [status]
    listen = "0.0.0.0:8899"
    prometheus_url = "http://xnet-prometheus:9090"
    docker_socket = "/var/run/docker.sock"
    cutover_env_path = "/host/xnet/cutover-env"

    [status.server]
    domain = "${if xnetCfg.settings.remote_domain != null then xnetCfg.settings.remote_domain else ""}"
    region = "${cfg.region}"
    server_type = "${cfg.serverType}"
    use_tls = ${if xnetCfg.settings.useTls || xnetCfg.settings.publicScheme == "https" then "true" else "false"}
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
    # Rust-based status page and API server (Docker container on xnet network)
    systemd.services.xnet-status = {
      description = "xnet status page and API";
      after = [ "xnet.service" "docker.service" "network-online.target" ];
      wants = [ "xnet.service" "network-online.target" ];
      requires = [ "docker.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [ pkgs.docker ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        SupplementaryGroups = [ "docker" ];
      };
      script = ''
        # Load image from Nix store
        docker load < ${xnet-status-image}

        # Remove old container if exists
        docker rm -f xnet-status 2>/dev/null || true

        # Run on xnet network so Traefik can reach it by container name
        docker run -d \
          --name xnet-status \
          --network xnet \
          --restart unless-stopped \
          -v /etc/xnet:/host/xnet:ro \
          -v /var/run/docker.sock:/var/run/docker.sock:ro \
          xnet-status:latest
      '';
      preStop = ''
        docker rm -f xnet-status 2>/dev/null || true
      '';
    };

    # Route domains to services via Traefik
    # Status page runs as Docker container on xnet network
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
