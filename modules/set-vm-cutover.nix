{ pkgs, ... }:
{
  # Write a boot-time script that sets cutover to 5 minutes from now
  systemd.services.xnet-write-cutover = {
    description = "Write cutover timestamp";
    before = [ "xnet-cutover.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = pkgs.writeShellScript "write-cutover" ''
        mkdir -p /etc/xnet
        echo "XNET_CUTOVER_TIMESTAMP=$(( $(date +%s%N) + (300 * 1000000000) ))" > /etc/xnet/cutover-env
        echo "XNET_PUBLIC_IP=127.0.0.1" >> /etc/xnet/cutover-env
      '';
    };
  };
}
