{ modulesPath, pkgs, ... }:
{
  imports = [
    (modulesPath + "/profiles/qemu-guest.nix")
  ];

  boot.loader.grub = {
    enable = true;
    efiSupport = true;
    efiInstallAsRemovable = true;
  };

  nix = {
    settings.experimental-features = [
      "nix-command"
      "flakes"
    ];
  };
  networking.hostName = "xmtplabs-migration-test";

  nix.settings = {
    extra-substituters = [ "https://xmtp.cachix.org" ];
    extra-trusted-public-keys = [ "xmtp.cachix.org-1:nFPFrqLQ9kjYQKiWL7gKq6llcNEeaV4iI+Ka1F+Tmq0=" ];
  };

  # TCP tuning for gRPC/h2c through Traefik
  boot.kernel.sysctl = {
    "net.core.somaxconn" = 4096;
    "net.ipv4.tcp_max_syn_backlog" = 4096;
    "net.core.netdev_max_backlog" = 4096;
    "net.ipv4.tcp_tw_reuse" = 1;
    "net.ipv4.tcp_fin_timeout" = 15;
  };

  # Docker
  virtualisation.docker.enable = true;

  networking.firewall.allowedTCPPorts = [ 22 ];

  # Allow root SSH for nixos-anywhere and CI access
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "no";
      PasswordAuthentication = false;
    };
  };

  users.users.insipx = {
    isNormalUser = true;
    extraGroups = [
      "wheel"
      "input"
      "docker"
    ];
    users.defaultUserShell = pkgs.fish;
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILUArrr4oix6p/bSjeuXKi2crVzsuSqSYoz//YJMsTlo cardno:14_836_775"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJXIgq273dJuJYSshYwk96GL/W3u1elMWPDZHVYXY+Jg andrew@xmtp.com"
    ];
  };
  environment.systemPackages = with pkgs; [
    toxiproxy
    ghostty.terminfo
    htop
    foundry-bin
    fishPlugins.grc
    fishPlugins.done
  ];

  programs.fish = {
    enable = true;
    interactiveShellInit = ''
      fish_vi_key_bindings
    '';
  };
  system.stateVersion = "25.11";
}
