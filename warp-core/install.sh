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
systemctl --user daemon-reload
# Live at once (Y, 2026-11-27: every improvement goes live when it's done;
# a restart with each one is fine).
if systemctl --user is-active --quiet warp-core; then
    systemctl --user restart warp-core
    # Survival first: if the new one won't stay up, put the old one back.
    sleep 8
    if ! systemctl --user is-active --quiet warp-core && [ -x "$lib/warp-core.prev" ]; then
        mv -f "$lib/warp-core.prev" "$lib/warp-core"
        [ -n "$prev_mods" ] && ln -sfn "$prev_mods" "$HOME/.config/warp-core/mods"
        echo "$prev_it" > "$it"
        systemctl --user reset-failed warp-core
        systemctl --user restart warp-core
        echo "warp core 1.$((prev_it + 1)) would not stay up: rolled back to 1.$prev_it" >&2
        exit 1
    fi
    echo "installed and restarted: warp core 1.$(cat "$it")"
else
    echo "installed warp core 1.$(cat "$it"). start it with: systemctl --user enable --now warp-core"
fi
