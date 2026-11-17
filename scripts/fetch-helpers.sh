#!/bin/bash
# Fetches the Typst program that SproutCrew bundles to make PDFs, and agent-desktop, which a computer-use step
# uses to operate one approved app through its accessibility tree, from their official GitHub releases, checks each download against the hash GitHub publishes, and joins the Intel and Apple
# Silicon builds into one universal program. The results are not committed (crates/bff-app/binaries/ is ignored).
set -euo pipefail

TYPST=0.15.1
AGENT_DESKTOP=1.2.0
# macOS ships bash 3.2, which has no associative arrays.
sha() {
  case "$1" in
    typst-aarch64-apple-darwin.tar.xz) echo 48f62ed034aa3a7978309579ac6ca00045e2ef0da73114e8af27cfd8e74dc05a ;;
    typst-x86_64-apple-darwin.tar.xz) echo 7f9fdd9584866245de9a79e0add8f9236fae6f40a8a45e2c4771ccc14db4e0fa ;;
    agent-desktop-v1.2.0-aarch64-apple-darwin.tar.gz) echo 219813428bb1ad8decae172faed05c5964a2415170df402e2786ccfa6452ba82 ;;
    agent-desktop-v1.2.0-x86_64-apple-darwin.tar.gz) echo d9dc50226f28d7fc86a2f5b0945f0c206dff6c31e418907749f493b6f9e773e6 ;;
  esac
}

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/crates/bff-app/binaries"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out" "$out/licenses"

fetch() { # url name
  curl -fsSL --proto '=https' -o "$work/$2" "$1"
  echo "$(sha "$2")  $work/$2" | shasum -a 256 -c - >/dev/null || { echo "Hash mismatch for $2" >&2; exit 1; }
}

for triple in aarch64-apple-darwin x86_64-apple-darwin; do
  name="typst-$triple.tar.xz"
  fetch "https://github.com/typst/typst/releases/download/v$TYPST/$name" "$name"
  mkdir "$work/typst-$triple" && tar -xJf "$work/$name" -C "$work/typst-$triple"
done

for triple in aarch64-apple-darwin x86_64-apple-darwin; do
  name="agent-desktop-v$AGENT_DESKTOP-$triple.tar.gz"
  fetch "https://github.com/vercel-labs/agent-desktop/releases/download/v$AGENT_DESKTOP/$name" "$name"
  mkdir "$work/ad-$triple" && tar -xzf "$work/$name" -C "$work/ad-$triple"
done
agent_desktop_arm="$work/ad-aarch64-apple-darwin/agent-desktop"
agent_desktop_x86="$work/ad-x86_64-apple-darwin/agent-desktop"
agent_desktop_macos_helper_arm="$work/ad-aarch64-apple-darwin/agent-desktop-macos-helper"
agent_desktop_macos_helper_x86="$work/ad-x86_64-apple-darwin/agent-desktop-macos-helper"

typst_arm=$(find "$work/typst-aarch64-apple-darwin" -type f -name typst)
typst_x86=$(find "$work/typst-x86_64-apple-darwin" -type f -name typst)

# Tauri looks for one file per target; a universal build uses the -universal-apple-darwin one.
for tool in typst agent-desktop agent-desktop-macos-helper; do
  var="${tool//-/_}"; arm_var="${var}_arm"; x86_var="${var}_x86"
  cp "${!arm_var}" "$out/$tool-aarch64-apple-darwin"
  cp "${!x86_var}" "$out/$tool-x86_64-apple-darwin"
  lipo -create "${!arm_var}" "${!x86_var}" -output "$out/$tool-universal-apple-darwin"
done
chmod 755 "$out"/typst-* "$out"/agent-desktop-*
curl -fsSL --proto '=https' -o "$out/licenses/agent-desktop-LICENSE.txt" "https://raw.githubusercontent.com/vercel-labs/agent-desktop/v$AGENT_DESKTOP/LICENSE"

cp "$(find "$work/typst-aarch64-apple-darwin" -type f -name LICENSE | head -1)" "$out/licenses/typst-LICENSE.txt"
curl -fsSL --proto '=https' -o "$out/licenses/typst-NOTICE.txt" "https://raw.githubusercontent.com/typst/typst/v$TYPST/NOTICE"

"$out/typst-universal-apple-darwin" --version
"$out/agent-desktop-universal-apple-darwin" version
for tool in typst agent-desktop agent-desktop-macos-helper; do lipo -archs "$out/$tool-universal-apple-darwin"; done
