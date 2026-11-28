#!/bin/sh
# Install warp-core as a user service. Run from anywhere; build first:
#   cargo build --release -j 2
#   ./install.sh            # copies the binary, links the mods, installs the unit
#   systemctl --user enable --now warp-core
#
# The binary is copied (not linked), so a rebuild never changes the running
# copy until you install again. The mods folder is linked, so edits to
# mods/warp-core.toml and mods/shaders/*.wgsl stay live. If the link breaks,
# the warp core runs on its built-in defaults.
#
# Written by: claude
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="$here/target/release/warp-core"
[ -x "$bin" ] || { echo "no $bin: run cargo build --release -j 2 first" >&2; exit 1; }
mkdir -p "$HOME/.local/lib/warp-core" "$HOME/.config/warp-core" "$HOME/.config/systemd/user"
cp "$bin" "$HOME/.local/lib/warp-core/warp-core.new"
mv -f "$HOME/.local/lib/warp-core/warp-core.new" "$HOME/.local/lib/warp-core/warp-core"
ln -sfn "$here/mods" "$HOME/.config/warp-core/mods"
cp "$here/warp-core.service" "$HOME/.config/systemd/user/warp-core.service"
systemctl --user daemon-reload
echo "installed. start it with: systemctl --user enable --now warp-core"
