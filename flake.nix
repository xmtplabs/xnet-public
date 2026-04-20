{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    foundry.url = "github:shazow/foundry.nix/stable";
    libxmtp.url = "github:xmtp/libxmtp/insipx/service-toggles";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{
      nixpkgs,
      disko,
      libxmtp,
      self,
      flake-parts,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } (
      _:
      let
        common = [
          disko.nixosModules.disko
          ./configuration.nix
          ./modules/xnet.nix
          ./modules/xnet-cutover.nix
          ./modules/xnet-status
          (_: {
            nixpkgs.overlays = [
              libxmtp.overlays.default
              inputs.foundry.overlay
            ];
            services.xnet = {
              enable = true;
              settings = {
                publicScheme = "https";
                paused = true;
                xmtpd = {
                  version = "v1.3.0";
                  nodes = [
                    {
                      enable = true;
                      migrator = true;
                      port = 5050;
                    }
                  ];
                };
                migration.enable = true;
                contracts.version = "v2026.02.10-1";
                v3.version = "main";
              };
            };
            services.xnet-status = {
              enable = true;
              domain = "migrate.xmtp.run";
              region = "hil (Hillsboro, OR)";
              serverType = "cpx51";
            };
          })
        ];
      in
      {
        systems = [
          "aarch64-darwin"
          "aarch64-linux"
          "x86_64-linux"
        ];
        flake = {
          nixosConfigurations.public-xnet = nixpkgs.lib.nixosSystem {
            system = "x86_64-linux";
            modules = common ++ [
              ./disks.nix
              (
                { pkgs, config, ... }:
                {
                  services.xnet.settings.remote_domain = "xmtp.run";
                  services.xnet-status.serverIp = "5.78.25.67";
                  # Add floating IP as an alias without disrupting DHCP
                  systemd.services.floating-ip = {
                    description = "Configure Hetzner floating IP";
                    after = [ "network-online.target" ];
                    wants = [ "network-online.target" ];
                    wantedBy = [ "multi-user.target" ];
                    path = [
                      pkgs.iproute2
                      pkgs.gawk
                    ];
                    serviceConfig = {
                      Type = "oneshot";
                      RemainAfterExit = true;
                    };
                    script = ''
                      DEV=$(ip route show default | awk '{print $5; exit}')
                      ip addr add ${config.services.xnet-status.serverIp}/32 dev "$DEV"
                    '';
                    preStop = ''
                      DEV=$(ip route show default | awk '{print $5; exit}')
                      ip addr del ${config.services.xnet-status.serverIp}/32 dev "$DEV"
                    '';
                  };
                }
              )
            ];
          };
          # configuration that just tests the behavior of this nixos config in a local QemuVM
          # all services are forwarded to `localhost`
          # and cutover is set to 5min from vm boot
          nixosConfigurations.public-xnet-vm = nixpkgs.lib.nixosSystem {
            system = "x86_64-linux";
            modules = common ++ [
              ./modules/set-vm-cutover.nix
              # VM-specific overrides
              (
                { modulesPath, lib, ... }:
                let
                  ports = [
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
                    9090
                    5600
                    5100
                    5354
                  ];
                  portRange = lib.range 8100 8119;
                  allPorts = ports ++ portRange;
                  fwds = builtins.concatStringsSep "," (
                    map (p: "hostfwd=tcp::${toString p}-:${toString p}") allPorts
                    ++ [
                      "hostfwd=tcp::80-:80"
                      "hostfwd=tcp::8080-:80"
                      "hostfwd=udp::5354-:5354"
                    ]
                  );
                in
                {
                  imports = [
                    (modulesPath + "/virtualisation/qemu-vm.nix")
                  ];
                  # Override disko disk device for VM
                  disko.devices.disk.main.device = "/dev/vda";
                  services.xnet.settings.enableDebugPorts = true;
                  users.users.root.initialPassword = "root";
                  users.users.test = {
                    isNormalUser = true;
                    initialPassword = "test";
                    extraGroups = [
                      "wheel"
                      "docker"
                    ];
                  };
                  # Enable NAT so the guest can reach the internet
                  networking.nameservers = [ "8.8.8.8" ];
                  networking.firewall.enable = false;
                  virtualisation = {
                    graphics = false;
                    qemu.networkingOptions = [
                      "-nic user,model=virtio-net-pci,${fwds}"
                    ];
                    diskSize = 32768;
                    memorySize = 4096;
                    vmVariant = { };
                  };
                }
              )
            ];
          };
        };
        perSystem =
          { pkgs, ... }:
          {
            packages.vm = self.nixosConfigurations.public-xnet-vm.config.system.build.vm;
            packages.xnet-status = pkgs.rustPlatform.buildRustPackage {
              pname = "xnet-status";
              version = "0.1.0";
              src = ./services/xnet-status;
              cargoLock.lockFile = ./services/xnet-status/Cargo.lock;
            };
            packages.xnet-status-image =
              let
                logo = pkgs.fetchurl {
                  url = "https://raw.githubusercontent.com/xmtp/libxmtp/main/apps/xnet/gui/assets/logo.png";
                  sha256 = "sha256-+X/sYIPIec7MN2/xFh08I3yE3fEuMq5vllqS+ZpzWmA=";
                };
                logoBase64 = pkgs.runCommand "logo-base64" { } ''
                  ${pkgs.coreutils}/bin/base64 -w0 ${logo} > $out
                '';
                pkg = pkgs.rustPlatform.buildRustPackage {
                  pname = "xnet-status";
                  version = "0.1.0";
                  src = ./services/xnet-status;
                  cargoLock.lockFile = ./services/xnet-status/Cargo.lock;
                  postInstall = ''
                    mkdir -p $out/share/xnet-status/static
                    cp ${./services/xnet-status/static/style.css} $out/share/xnet-status/static/style.css
                    cp ${logoBase64} $out/share/xnet-status/static/logo.b64
                  '';
                };
                testConfig = pkgs.writeText "xnet-status.toml" ''
                  [status]
                  listen = "0.0.0.0:8899"
                  prometheus_url = "http://localhost:9090"
                  docker_socket = "/var/run/docker.sock"
                  cutover_env_path = "/etc/xnet/cutover-env"

                  [status.server]
                  ip = "127.0.0.1"
                  domain = ""
                  region = "local"
                  server_type = "test"
                  use_tls = false
                '';
              in
              pkgs.dockerTools.buildImage {
                name = "xnet-status";
                tag = "latest";
                copyToRoot = pkgs.buildEnv {
                  name = "xnet-status-root";
                  paths = [
                    pkg
                    pkgs.cacert
                    (pkgs.runCommand "xnet-status-config" { } ''
                      mkdir -p $out/etc/xnet
                      cp ${testConfig} $out/etc/xnet/status.toml
                    '')
                  ];
                };
                config = {
                  Cmd = [ "${pkg}/bin/xnet-status" "--config" "/etc/xnet/status.toml" ];
                  WorkingDirectory = "${pkg}/share/xnet-status";
                  Env = [ "XNET_STATUS_SHARE=${pkg}/share/xnet-status" ];
                  ExposedPorts = { "8899/tcp" = { }; };
                };
              };
            devShells.default = pkgs.mkShell {
              nativeBuildInputs = with pkgs; [
                nixos-anywhere
                jq
                hcloud
                cargo
              ];
            };
          };
      }
    );
}
