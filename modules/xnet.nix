{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.xnet;
in
{
  options.services.xnet = {
    enable = lib.mkEnableOption "xnet local XMTP network";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.xnet-cli;
      description = "The xnet-cli package";
    };

    # Add whatever flags your CLI needs
    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [
        "--nodes"
        "3"
        "--public-ip"
        "1.2.3.4"
      ];
    };
  };

  config = lib.mkIf cfg.enable {
    virtualisation.docker.enable = true;

    systemd.services.xnet = {
      description = "xnet-cli local network for testing";
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
        ExecStart = "${cfg.package}/bin/xnet-cli up ${lib.escapeShellArgs cfg.extraArgs}";
        ExecStop = "${cfg.package}/bin/xnet-cli delete";
        Restart = "on-failure";
        RestartSec = 5;

        SupplementaryGroups = [ "docker" ];
      };
    };

    # Open the ports Traefik needs
    networking.firewall.allowedTCPPorts = [
      80
      443
      3000
      5050
      5052
      5432
      5556
      5558
      6379
      8474
      8545
      8555
      50051
      443
      9090
      5600
    ];
  };
}
