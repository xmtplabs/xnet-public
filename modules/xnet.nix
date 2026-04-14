{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.xnet;

  imageSubmodule =
    { defaults }:
    {
      options = {
        image = lib.mkOption {
          type = lib.types.str;
          default = defaults.image;
        };
        version = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = defaults.version;
        };
      };
    };

  imagePortSubmodule =
    { defaults }:
    {
      options = {
        image = lib.mkOption {
          type = lib.types.str;
          default = defaults.image;
        };
        version = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = defaults.version;
        };
        port = lib.mkOption {
          type = lib.types.nullOr lib.types.port;
          default = defaults.port or null;
        };
      };
    };

  nodeSubmodule = {
    options = {
      name = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        description = "Name of the node";
        default = null;
      };
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
      };
      port = lib.mkOption {
        type = lib.types.nullOr lib.types.port;
        default = null;
      };
      migrator = lib.mkOption {
        type = lib.types.bool;
        default = false;
      };
    };
  };

  # Build the TOML-ready attrset
  tomlConfig = {
    xnet = {
      use_standard_ports = cfg.settings.useStandardPorts;
      enable_v3 = cfg.settings.enableV3;
      enable_d14n = cfg.settings.enableD14n;
      enable_monitoring = cfg.settings.enableMonitoring;
    }
    // lib.optionalAttrs (cfg.settings.remote_ip != null) {
      remote_ip = cfg.settings.remote_ip;
    }
    // lib.optionalAttrs (cfg.settings.remote_domain != null) {
      remote_domain = cfg.settings.remote_domain;
    }
    // lib.optionalAttrs cfg.settings.paused {
      paused = true;
    };

    traefik = {
      inherit (cfg.settings.traefik) image version;
    }
    // lib.optionalAttrs (cfg.settings.traefik.port != null) {
      port = cfg.settings.traefik.port;
    }
    // lib.optionalAttrs (cfg.settings.traefik.https_port != null) {
      https_port = cfg.settings.traefik.https_port;
    };

    toxiproxy = {
      inherit (cfg.settings.toxiproxy) image version;
    }
    // lib.optionalAttrs (cfg.settings.toxiproxy.port != null) {
      port = cfg.settings.toxiproxy.port;
    };

    migration = {
      enable = cfg.settings.migration.enable;
      migration_timestamp = cfg.settings.migration.timestamp;
    };

    xmtpd = {
      inherit (cfg.settings.xmtpd) image version;
      nodes = map (node: lib.filterAttrs (_: v: v != null) node) cfg.settings.xmtpd.nodes;
    };

    v3 = cfg.settings.v3;
    gateway = cfg.settings.gateway;
    validation = cfg.settings.validation;
    contracts = cfg.settings.contracts;
    history = cfg.settings.history;
    prometheus = cfg.settings.prometheus;
    grafana = cfg.settings.grafana;
  }
  // lib.optionalAttrs (cfg.settings.extraTraefikRoutes != [ ]) {
    extra_traefik_routes = map (
      r:
      { inherit (r) name rule url; } // lib.optionalAttrs (r.priority != null) { inherit (r) priority; }
    ) cfg.settings.extraTraefikRoutes;
  };

  configFile = (pkgs.formats.toml { }).generate "xnet.toml" tomlConfig;

in
{
  options.services.xnet = {
    enable = lib.mkEnableOption "xnet local XMTP network";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.xnet-cli;
      description = "The xnet-cli package";
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "addresses" ];
    };
    settings = {
      useStandardPorts = lib.mkOption {
        type = lib.types.bool;
        default = true;
      };

      paused = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "whether the network starts off with paused smart contracts. needed for migration";
      };

      enableDebugPorts = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Expose debug ports (toxiproxy, pgadmin, coredns) in the firewall";
      };

      remote_ip = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "if running on a public server, the public ip of the server to route with";
      };

      remote_domain = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Custom domain for remote addressing (e.g. xmtp.run). Mutually exclusive with remote_ip.";
      };

      enableV3 = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable the V3 stack (V3Db, MlsDb, NodeGo)";
      };

      enableD14n = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable the D14n stack (Redis, Gateway, XMTPD nodes)";
      };

      enableMonitoring = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable monitoring services (Prometheus, Grafana, PgAdmin, Otterscan)";
      };

      traefik = {
        image = lib.mkOption {
          type = lib.types.str;
          default = "traefik";
        };
        version = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = "v3.2";
        };
        port = lib.mkOption {
          type = lib.types.nullOr lib.types.port;
          default = null;
          description = "Override the Traefik HTTP host port (default: 80)";
        };
        https_port = lib.mkOption {
          type = lib.types.nullOr lib.types.port;
          default = null;
          description = "Override the Traefik HTTPS host port (default: 443)";
        };
      };

      toxiproxy = {
        image = lib.mkOption {
          type = lib.types.str;
          default = "ghcr.io/shopify/toxiproxy";
        };
        version = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = "2.12.0";
        };
        port = lib.mkOption {
          type = lib.types.nullOr lib.types.port;
          default = null;
          description = "Override the ToxiProxy port (default: 8474)";
        };
      };

      migration = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
        };
        timestamp = lib.mkOption {
          type = lib.types.int;
          default = 2147483647;
        };
      };

      xmtpd = {
        image = lib.mkOption {
          type = lib.types.str;
          default = "ghcr.io/xmtp/xmtpd";
        };
        version = lib.mkOption {
          type = lib.types.str;
          default = "sha-f72e436";
        };
        nodes = lib.mkOption {
          type = lib.types.listOf (lib.types.submodule nodeSubmodule);
          default = [ ];
          example = [
            {
              name = "alice-operator";
              enable = true;
              port = 3000;
              migrator = true;
            }
            {
              name = "bob-operator";
              enable = true;
              port = 3001;
            }
          ];
        };
      };

      v3 = lib.mkOption {
        type = lib.types.submodule (imageSubmodule {
          defaults = {
            image = "ghcr.io/xmtp/node-go";
            version = "main";
          };
        });
        default = { };
      };
      gateway = lib.mkOption {
        type = lib.types.submodule (imageSubmodule {
          defaults = {
            image = "ghcr.io/xmtp/xmtpd-gateway";
            version = "v1.3.0";
          };
        });
        default = { };
      };
      validation = lib.mkOption {
        type = lib.types.submodule (imageSubmodule {
          defaults = {
            image = "ghcr.io/xmtp/mls-validation-service";
            version = "main";
          };
        });
        default = { };
      };
      contracts = lib.mkOption {
        type = lib.types.submodule (imageSubmodule {
          defaults = {
            image = "ghcr.io/xmtp/contracts";
            version = "v2026.02.10-1";
          };
        });
        default = { };
      };
      history = lib.mkOption {
        type = lib.types.submodule (imageSubmodule {
          defaults = {
            image = "ghcr.io/xmtp/message-history-server";
            version = "main";
          };
        });
        default = { };
      };
      prometheus = lib.mkOption {
        type = lib.types.submodule (imageSubmodule {
          defaults = {
            image = "prom/prometheus";
            version = "latest";
          };
        });
        default = { };
      };
      grafana = lib.mkOption {
        type = lib.types.submodule (imageSubmodule {
          defaults = {
            image = "ghcr.io/xmtp/grafana-xmtpd";
            version = "latest";
          };
        });
        default = { };
      };

      extraTraefikRoutes = lib.mkOption {
        type = lib.types.listOf (
          lib.types.submodule {
            options = {
              name = lib.mkOption { type = lib.types.str; };
              rule = lib.mkOption { type = lib.types.str; };
              url = lib.mkOption { type = lib.types.str; };
              priority = lib.mkOption {
                type = lib.types.nullOr lib.types.int;
                default = null;
              };
            };
          }
        );
        default = [ ];
        description = "Additional Traefik routes injected into dynamic config";
      };
    };
  };
  config = lib.mkIf cfg.enable {
    virtualisation.docker.enable = true;
    environment.etc."xnet/xnet.toml".source = configFile;

    systemd.services.xnet = {
      description = "xnet-cli local network for testing XMTP";
      after = [
        "docker.service"
        "network-online.target"
      ];
      requires = [ "docker.service" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${cfg.package}/bin/xnet-cli -c /etc/xnet/xnet.toml up ${lib.escapeShellArgs cfg.extraArgs}";
        ExecStop = "${cfg.package}/bin/xnet-cli delete";
        Restart = "on-failure";
        RestartSec = 5;

        SupplementaryGroups = [ "docker" ];
      };
    };

    environment.systemPackages = [ cfg.package ];

    networking.firewall.allowedTCPPorts = [
      80
      443
    ]
    ++ lib.optional (cfg.settings.traefik.port != null) cfg.settings.traefik.port
    ++ lib.optional (cfg.settings.traefik.https_port != null) cfg.settings.traefik.https_port
    ++ [
      5050 # xmtpd
      5052 # gateway
      5556 # node go
      8100 # node go http
      5558 # history server
      8545 # anvil
      50051 # mls validation
      9090 # prometheus
      5100 # otterscan
      3000 # grafana
    ]
    ++ lib.optionals cfg.settings.enableDebugPorts [
      8474 # toxiproxy
      5600 # pgadmin
      5354 # coredns
    ];
  };
}
