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

# With the updater key in the settings file, the build also makes a signed update and the app checks
# download.sproutcrew.app/latest.json. Without it, the app is built as one that does not update itself.
extra=()
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  [ -n "${SPROUTCREW_UPDATER_PUBKEY:-}" ] || { echo "SPROUTCREW_UPDATER_PUBKEY is empty in $env_file" >&2; exit 1; }
  updater_conf="$(mktemp)"
  trap 'rm -f "$updater_conf"' EXIT
  python3 -I -c 'import json, sys; json.dump({"bundle": {"createUpdaterArtifacts": True}, "plugins": {"updater": {"pubkey": sys.argv[1], "endpoints": ["https://download.sproutcrew.app/latest.json"]}}}, open(sys.argv[2], "w"))' "$SPROUTCREW_UPDATER_PUBKEY" "$updater_conf"
  extra=(--config "$updater_conf")
fi
# macOS bash 3.2 treats an empty array as unset under set -u, hence the guard.
cargo tauri build --bundles app,dmg --target aarch64-apple-darwin ${extra[@]+"${extra[@]}"}

out="$CARGO_TARGET_DIR/aarch64-apple-darwin/release/bundle"
app="$out/macos/SproutCrew.app"
codesign --verify --deep --strict "$app"
spctl --assess --type execute -vv "$app"
xcrun stapler validate "$app"
for f in "$app/Contents/MacOS/"*; do
  [ "$(lipo -archs "$f")" = "arm64" ] || { echo "$f is not Apple Silicon only" >&2; exit 1; }
done
# The bundled program must carry the same Developer ID and the hardened runtime notarization requires.
# agent-desktop is not bundled while no step can use it (decided November 16, 2026); when computer use is wired,
# add it and its helper back here and to tauri.conf.json, with a check that the signed copy still starts.
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

# The update file, for Apple Silicon Macs only.
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
  python3 -I - "$out/macos/SproutCrew.app.tar.gz" "$version" "$out/latest.json" <<'PY'
import datetime, json, pathlib, sys
archive, version, dest = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
entry = {
    "signature": pathlib.Path(str(archive) + ".sig").read_text().strip(),
    "url": f"https://download.sproutcrew.app/releases/{version}/SproutCrew.app.tar.gz",
}
json.dump({
    "version": version,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {"darwin-aarch64": entry},
}, open(dest, "w"), indent=2)
print(dest)
PY
fi
