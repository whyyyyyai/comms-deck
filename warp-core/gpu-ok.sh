#!/bin/sh
# Is the GPU safe to open a warp-core window on? Exit 0 if yes, 1 if not.
#
# Run before every start (the unit's ExecStartPre) and before every install
# restart. On 2026-11-27 the M1 GPU faulted at 05:19 while warp-core was being
# restarted; the restarts that followed (systemd, install.sh rollbacks) hit it
# 42 times until the GPU died at 05:31 and the iMac rebooted. So:
#   - "GPU is crashed" this boot: the GPU won't come back without a reboot.
#   - "GPU timeout" in the last 10 minutes: let it settle, don't pile on.
#
# Written by: claude
log=$(journalctl -k -b --no-pager -q 2>/dev/null) || exit 0  # can't tell: allow
if printf '%s\n' "$log" | grep -q "GPU is crashed"; then
    echo "warp-core: the GPU crashed this boot; not starting until a reboot" >&2
    exit 1
fi
if journalctl -k -b --no-pager -q --since "-10min" 2>/dev/null | grep -q "GPU timeout"; then
    echo "warp-core: the GPU timed out in the last 10 minutes; not starting yet" >&2
    exit 1
fi
exit 0
