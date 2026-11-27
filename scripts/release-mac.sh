#!/bin/bash
# Builds the signed, notarized SproutCrew DMG for Apple Silicon and checks it the way a downloaded copy is checked.
# Intel Macs are no longer built for (Nitish, November 17, 2026), so the app carries no Intel code.
# The Apple account settings live outside the repository, in ~/.config/sproutcrew/release.env (docs/V2_PLAN.md, step 4).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"

env_file="$HOME/.config/sproutcrew/release.env"
[ -f "$env_file" ] || { echo "Missing $env_file. See docs/V2_PLAN.md, step 4." >&2; exit 1; }
set -a
. "$env_file"
set +a
for v in APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_KEY_PATH APPLE_API_ISSUER; do
  [ -n "${!v:-}" ] || { echo "$v is empty in $env_file" >&2; exit 1; }
done
[ -f "$APPLE_API_KEY_PATH" ] || { echo "No key file at $APPLE_API_KEY_PATH" >&2; exit 1; }

[ -f "$(dirname "$0")/../crates/bff-app/binaries/typst-aarch64-apple-darwin" ] || "$(dirname "$0")/fetch-helpers.sh"
python3 -I "$(dirname "$0")/notices.py"
cd "$(dirname "$0")/../crates/bff-app"

cargo tauri build --bundles app,dmg --target aarch64-apple-darwin

out="$CARGO_TARGET_DIR/aarch64-apple-darwin/release/bundle"
app="$out/macos/SproutCrew.app"
codesign --verify --deep --strict "$app"
spctl --assess --type execute -vv "$app"
xcrun stapler validate "$app"
for f in "$app/Contents/MacOS/"*; do
  [ "$(lipo -archs "$f")" = "arm64" ] || { echo "$f is not Apple Silicon only" >&2; exit 1; }
done
# The bundled program must carry the same Developer ID and the hardened runtime notarization requires.
team=$(codesign -dv "$app" 2>&1 | sed -n 's/^TeamIdentifier=//p')
for bin in typst; do
  f="$app/Contents/MacOS/$bin"
  [ -x "$f" ] || { echo "Missing bundled program: $bin" >&2; exit 1; }
  info=$(codesign -dv "$f" 2>&1)
  echo "$info" | grep -q "^TeamIdentifier=$team$" || { echo "$bin is not signed with the app's Developer ID" >&2; exit 1; }
  echo "$info" | grep -q "(runtime)" || { echo "$bin is signed without the hardened runtime" >&2; exit 1; }
  codesign --verify --strict "$f"
done

# Tauri notarizes the app but not the DMG around it, and macOS rejects a downloaded DMG that is not notarized.
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
dmg="$out/dmg/SproutCrew_${version}_aarch64.dmg"
[ -f "$dmg" ] || { echo "Expected versioned disk image is missing: $dmg" >&2; exit 1; }
xcrun notarytool submit "$dmg" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait
xcrun stapler staple "$dmg"
spctl --assess --type open --context context:primary-signature -vv "$dmg"
echo "$dmg"
