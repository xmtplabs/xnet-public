# Ephemeral Xnet Deploy Tool


tool to deploys xnet to hetzner for a set amount of time

Test configuration in QEMU (linux only for now): `nix run .#vm`

## Schedule

3 cycles / day · 8 hours each · Cutover at +4h into each cycle

| Event | EST | PST | MST | CEST |
|---|---|---|---|---|
| **Cycle 1 — Start** | 11:00 PM^-1 | 8:00 PM^-1 | 9:00 PM^-1 | 5:00 AM |
| V3 Live | 11:15 PM^-1 | 8:15 PM^-1 | 9:15 PM^-1 | 5:15 AM |
| **Cutover / Migration** | **3:00 AM** | 12:00 AM | 1:00 AM | **9:00 AM** |
| Teardown | 6:55 AM | 3:55 AM | 4:55 AM | 12:55 PM |
| | | | | |
| **Cycle 2 — Start** | **7:00 AM** | 4:00 AM | 5:00 AM | 1:00 PM |
| V3 Live | 7:15 AM | 4:15 AM | 5:15 AM | 1:15 PM |
| **Cutover / Migration** | **11:00 AM** | **8:00 AM** | **9:00 AM** | **5:00 PM** |
| Teardown | 2:55 PM | 11:55 AM | 12:55 PM | 8:55 PM |
| | | | | |
| **Cycle 3 — Start** | **3:00 PM** | 12:00 PM | 1:00 PM | 9:00 PM |
| V3 Live | 3:15 PM | 12:15 PM | 1:15 PM | 9:15 PM |
| **Cutover / Migration** | **7:00 PM** | **4:00 PM** | **5:00 PM** | 1:00 AM^+1 |
| Teardown | 10:55 PM | 7:55 PM | 8:55 PM | 4:55 AM^+1 |

> ^-1 previous day · ^+1 next day
>
> Live status: [migrate.xmtp.run](https://migrate.xmtp.run)
