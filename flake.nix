{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    libxmtp.url = "github:xmtp/libxmtp/main";
  };

  outputs =
    {
      nixpkgs,
      disko,
      libxmtp,
      self,
      ...
    }:
    let
      common = [
        disko.nixosModules.disko
        ./configuration.nix
        ./modules/xnet.nix
        (_: {
          nixpkgs.overlays = [
            libxmtp.overlays.default
          ];
          services.xnet = {
            enable = true;
          };
        })
      ];
      ports = [
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
        9090
        5600
      ];
      fwds = builtins.concatStringsSep "," (map (p: "hostfwd=tcp::${toString p}-:${toString p}") ports);
    in
    {
      nixosConfigurations.public-xnet = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = common ++ [
          ./disks.nix
        ];
      };
      nixosConfigurations.public-xnet-vm = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = common ++ [
          # VM-specific overrides
          (
            { modulesPath, ... }:
            {
              imports = [
                (modulesPath + "/virtualisation/qemu-vm.nix")
              ];
              virtualisation.qemu.networkingOptions = [
                "-nic user,model=virtio-net-pci,${fwds}"
              ];
              # Override disko disk device for VM
              disko.devices.disk.main.device = "/dev/vda";

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
              virtualisation = {
                diskSize = 16384;
                memorySize = 4096;
                vmVariant = { };
                forwardPorts = [
                  {
                    from = "host";
                    host.port = 8080;
                    guest.port = 80;
                  }
                  {
                    from = "host";
                    host.port = 8443;
                    guest.port = 443;
                  }
                  # add more as needed
                ];
              };
            }
          )
        ];
      };
      packages.x86_64-linux.vm = self.nixosConfigurations.public-xnet-vm.config.system.build.vm;
    };
}
