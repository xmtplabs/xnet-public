{ modulesPath, ... }:

{
  imports = [
    (modulesPath + "/profiles/qemu-guest.nix")
  ];

  boot.loader.grub = {
    enable = true;
    efiSupport = true;
    efiInstallAsRemovable = true;
  };

  networking.hostName = "xmtplabs-migration-test";

  # Docker
  virtualisation.docker.enable = true;

  # Open whatever ports your services need.
  # Adjust these to match what your CLI tool exposes.
  networking.firewall.allowedTCPPorts = [
    22
    # Add your service ports here, e.g.:
    5556
    5050
    8080
  ];

  # Allow root SSH for nixos-anywhere and CI access
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "prohibit-password";
      PasswordAuthentication = false;
    };
  };

  users.users.root.openssh.authorizedKeys.keys = [
    # Populated by the provision script via environment variable
    "@SSH_PUB_KEY@"
  ];

  system.stateVersion = "25.11";
}
