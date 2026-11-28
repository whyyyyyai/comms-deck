#!/bin/sh
# Install warp-core as a user service. Run from anywhere; build first:
#   cargo build --release -j 2
#   ./install.sh            # copies the binary and the mods, installs the unit,
#                           # restarts it, and rolls back if it won't stay up
#
# The binary and the mods are copied, so a rebuild or an edit in a checkout
# never changes the running copy until you install again. Edits to
# ~/.config/warp-core/mods (this version's copy) stay live.
#
# Written by: claude
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="$here/target/release/warp-core"
[ -x "$bin" ] || { echo "no $bin: run cargo build --release -j 2 first" >&2; exit 1; }
mkdir -p "$HOME/.local/lib/warp-core" "$HOME/.config/warp-core" "$HOME/.config/systemd/user"
# The version is 1.x, x the iteration: every install counts one more (Y,
# 2026-11-27). The first count starts from the warp core's commits so far.
it="$HOME/.local/lib/warp-core/iteration"
n=$(cat "$it" 2>/dev/null || git -C "$here" rev-list --count HEAD -- . 2>/dev/null || echo 0)
echo $((n + 1)) > "$it.new" && mv -f "$it.new" "$it"
lib="$HOME/.local/lib/warp-core"
# Keep what runs now, to roll back to.
[ -x "$lib/warp-core" ] && cp "$lib/warp-core" "$lib/warp-core.prev"
prev_mods=$(readlink "$HOME/.config/warp-core/mods" 2>/dev/null || true)
prev_it=$n
cp "$here/gpu-ok.sh" "$lib/gpu-ok.sh"
cp "$bin" "$HOME/.local/lib/warp-core/warp-core.new"
mv -f "$HOME/.local/lib/warp-core/warp-core.new" "$HOME/.local/lib/warp-core/warp-core"
# The mods are copied with each version, not linked to the checkout: a
# shader edited in a working tree for the next build once broke the running
# one (2026-11-27 05:16, filed in quantum-kittens). Live tweaks still work:
# edit ~/.config/warp-core/mods, which is this version's copy.
mods="$HOME/.local/lib/warp-core/mods-1.$(cat "$it")"
rm -rf "$mods.new" && cp -a "$here/mods" "$mods.new" && rm -rf "$mods" && mv "$mods.new" "$mods"
ln -sfn "$mods" "$HOME/.config/warp-core/mods"
cp "$here/warp-core.service" "$HOME/.config/systemd/user/warp-core.service"
# The price feed for the core ticker (t122): its own small service, so the
# warp core never waits on the network.
cp "$here/pricefeed" "$lib/pricefeed.new" && mv -f "$lib/pricefeed.new" "$lib/pricefeed"
cp "$here/warp-core-prices.service" "$HOME/.config/systemd/user/warp-core-prices.service"
systemctl --user daemon-reload
systemctl --user enable warp-core-prices >/dev/null 2>&1 || true
# Restart it so a new pricefeed (e.g. the seats' stocks) runs at once.
systemctl --user restart warp-core-prices >/dev/null 2>&1 || true
# Live at once (Y, 2026-11-27: every improvement goes live when it's done;
# a restart with each one is fine).
# A restart opens a new GPU window: never on a GPU that has just faulted
# (2026-11-27 05:19-05:31), and never more than once a minute.
if ! "$here/gpu-ok.sh"; then
    echo "installed warp core 1.$(cat "$it") but did not restart it: the GPU is unwell (see above)" >&2
    exit 1
fi
since=$(systemctl --user show warp-core -p ActiveEnterTimestampMonotonic --value 2>/dev/null || echo 0)
now=$(cut -d' ' -f1 /proc/uptime | cut -d. -f1)
if [ "${since:-0}" -gt 0 ] && [ $((now - since / 1000000)) -lt 60 ]; then
    echo "installed warp core 1.$(cat "$it"); it restarted under a minute ago, so this one goes live on its next start" >&2
    exit 0
fi
if systemctl --user is-active --quiet warp-core || systemctl --user is-failed --quiet warp-core; then
    # A deliberate install isn't a crash loop: clear the start counter so
    # systemd's 3-in-15-minutes limit is left for real crashes (installs are
    # held to one a minute above, and only on a healthy GPU).
    systemctl --user reset-failed warp-core 2>/dev/null || true
    systemctl --user restart warp-core
    # Survival first: if the new one won't stay up, put the old one back.
    sleep 8
    if ! systemctl --user is-active --quiet warp-core && [ -x "$lib/warp-core.prev" ]; then
        mv -f "$lib/warp-core.prev" "$lib/warp-core"
        [ -n "$prev_mods" ] && ln -sfn "$prev_mods" "$HOME/.config/warp-core/mods"
        echo "$prev_it" > "$it"
        # One try with the old one, and only on a healthy GPU; no
        # reset-failed, so systemd's start limit still holds.
        "$here/gpu-ok.sh" && systemctl --user restart warp-core
        echo "warp core 1.$((prev_it + 1)) would not stay up: rolled back to 1.$prev_it" >&2
        exit 1
    fi
    echo "installed and restarted: warp core 1.$(cat "$it")"
else
    echo "installed warp core 1.$(cat "$it"). start it with: systemctl --user enable --now warp-core"
fi
