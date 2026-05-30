#!/usr/bin/env bash
#
# Cargo `runner` for macOS — wired in `.cargo/config.toml`.
#
# Cargo hands this script the freshly built binary as $1 for every
# `cargo run` / `cargo test` / `cargo bench` on macOS. For everything except
# the desktop binary it's a transparent passthrough (`exec "$@"`).
#
# For `openlogi-gui` it launches the build from inside a throwaway
# `OpenLogi.app` so macOS shows the real app name (the bold menu-bar title)
# and the Dock icon during development. Both are read from the bundle's
# `Info.plist` / `Resources` — a bare `target/debug/openlogi-gui` has neither,
# so macOS falls back to the executable name and a generic icon.
#
# Set OPENLOGI_DEV_BUNDLE=0 to skip the wrapper and run the raw binary.
set -euo pipefail

bin="$1"
shift

if [ "${bin##*/}" != "openlogi-gui" ] || [ "${OPENLOGI_DEV_BUNDLE:-1}" = "0" ]; then
  exec "$bin" "$@"
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/target/dev/OpenLogi.app"
MACOS="$APP/Contents/MacOS"
RES="$APP/Contents/Resources"
ICON_SRC="$ROOT/crates/openlogi-gui/icon/AppIcon.icns"

mkdir -p "$MACOS" "$RES"

# App icon — gitignored, generated from the master SVG on demand. Mirror it
# into the bundle whenever the source is newer (or the bundle copy is missing).
if [ ! -f "$ICON_SRC" ]; then
  bash "$ROOT/scripts/macos-icns.sh"
fi
if [ "$ICON_SRC" -nt "$RES/AppIcon.icns" ]; then
  cp -f "$ICON_SRC" "$RES/AppIcon.icns"
fi

# Info.plist — minimal, dev-only. Mirrors the `[package.metadata.bundle]` keys
# that drive the app name + icon. A distinct `.dev` identifier keeps this
# target/ artifact from registering as the production app in LaunchServices.
PLIST="$APP/Contents/Info.plist"
if [ ! -f "$PLIST" ]; then
  cat > "$PLIST" <<'PLIST_EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenLogi</string>
  <key>CFBundleDisplayName</key><string>OpenLogi</string>
  <key>CFBundleExecutable</key><string>openlogi-gui</string>
  <key>CFBundleIdentifier</key><string>org.openlogi.openlogi.dev</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.1</string>
  <key>CFBundleVersion</key><string>0.0.1</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST_EOF
fi

# Hardlink the freshly built binary into the bundle — instant, no 95 MB copy.
# A hardlink (not a symlink) is required: both NSBundle.mainBundle and Rust's
# current_exe() realpath() the executable, which would resolve a symlink back
# to target/debug/ and break the bundle association. cargo rewrites the binary
# atomically on rebuild (new inode), so relink every run; `ln -f` repoints a
# stale link. Fall back to a copy if the bundle ever lands on another volume.
ln -f "$bin" "$MACOS/openlogi-gui" 2>/dev/null || cp -f "$bin" "$MACOS/openlogi-gui"

exec "$MACOS/openlogi-gui" "$@"
