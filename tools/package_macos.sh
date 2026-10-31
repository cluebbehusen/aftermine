#!/bin/sh
# Packages the shell as a macOS .app bundle under dist/Aftermine.app.
#
# Layout: Contents/MacOS/Aftermine beside Contents/Resources/{assets,
# scenarios} — the shell resolves resources relative to the executable
# when bundled (assets::resource_root), cwd otherwise, so the same
# binary serves both lives. Run from the workspace root:
#
#   sh tools/package_macos.sh
#
# Then smoke it:  dist/Aftermine.app/Contents/MacOS/Aftermine --debug-server
set -eu

cargo build --release -p aftermine-shell

# The bundle wears the workspace version, and its build number is the commit
# count, which only rises.
VERSION="$(cargo pkgid -p aftermine-shell)"
VERSION="${VERSION##*[@#]}"
BUILD="$(git rev-list --count HEAD)"

APP=dist/Aftermine.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp target/release/Aftermine "$APP/Contents/MacOS/"
cp -R assets "$APP/Contents/Resources/assets"
cp -R scenarios "$APP/Contents/Resources/scenarios"

# NOTE: unquoted heredoc — $VERSION interpolates, so any future plist
# edit must shell-escape $, backticks, and backslashes.
cat > "$APP/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Aftermine</string>
    <key>CFBundleDisplayName</key><string>Aftermine</string>
    <key>CFBundleIdentifier</key><string>dev.luebbehusen.aftermine</string>
    <key>CFBundleExecutable</key><string>Aftermine</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSLocalNetworkUsageDescription</key><string>Aftermine connects to other players on your network for LAN matches.</string>
    <key>CFBundleIconFile</key><string>aftermine.icns</string>
</dict>
</plist>
PLIST

# Icon, best effort: an iconset from the real mark (tools/gen_icon.py).
# Missing tools or assets just mean a default-icon app.
if command -v sips > /dev/null && command -v iconutil > /dev/null \
    && [ -f assets/icon/oxide_desktop_1024.png ]; then
    ICONSET=$(mktemp -d)/aftermine.iconset
    mkdir -p "$ICONSET"
    for SIZE in 16 32 128 256 512; do
        sips -z $SIZE $SIZE assets/icon/oxide_desktop_1024.png \
            --out "$ICONSET/icon_${SIZE}x${SIZE}.png" > /dev/null
        DOUBLE=$((SIZE * 2))
        sips -z $DOUBLE $DOUBLE assets/icon/oxide_desktop_1024.png \
            --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" > /dev/null
    done
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/aftermine.icns"
fi

# Smoke: the bundle must carry its pieces and tell the truth about its
# version. set -eu turns any miss into a packaging failure.
test -x "$APP/Contents/MacOS/Aftermine"
test -f "$APP/Contents/Resources/assets/sprites/atlas.png"
test -f "$APP/Contents/Resources/assets/sprites/atlas.json"
test -f "$APP/Contents/Resources/assets/sounds/music_menu.wav"
test -f "$APP/Contents/Resources/scenarios/basalt-spine.json"
grep -q "<string>$VERSION</string>" "$APP/Contents/Info.plist"
grep -q "<string>$BUILD</string>" "$APP/Contents/Info.plist"

echo "packaged Aftermine $VERSION ($BUILD) -> $APP"
