#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${PREFIX:-$HOME/.local}"

bin="$prefix/bin"
applications="$prefix/share/applications"
icons="$prefix/share/icons/hicolor"

echo "Building Acelus. This takes a few minutes the first time."
( cd "$root/ui" && npm install --silent && npx tauri build --no-bundle )
( cd "$root/cli" && go build -o "$root/target/release/acelus" ./cmd/acelus )

mkdir -p "$bin" "$applications"

for program in acelusd acelus-ui acelus; do
    install -m 755 "$root/target/release/$program" "$bin/$program"
done

for size in 32x32 64x64 128x128; do
    mkdir -p "$icons/$size/apps"
    install -m 644 "$root/ui/src-tauri/icons/$size.png" "$icons/$size/apps/acelus.png"
done

cat > "$applications/acelus.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Acelus
GenericName=Minecraft Launcher
Comment=Launch Minecraft: Java Edition
Exec=$bin/acelus-ui
Icon=acelus
Terminal=false
Categories=Game;
StartupWMClass=acelus-ui
DESKTOP

update-desktop-database "$applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$icons" 2>/dev/null || true

echo
echo "Installed to $prefix"
echo "Acelus is now in your applications menu."

case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "Add $bin to PATH to use the acelus command in a terminal." ;;
esac
