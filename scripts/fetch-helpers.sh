#!/bin/bash
# Fetches the Typst program that SproutCrew bundles to make PDFs from its official GitHub release and checks the
# download against the hash GitHub publishes. Apple Silicon only. The result is not committed
# (crates/bff-app/binaries/ is ignored).
set -euo pipefail

TYPST=0.15.1
TYPST_SHA=48f62ed034aa3a7978309579ac6ca00045e2ef0da73114e8af27cfd8e74dc05a
triple=aarch64-apple-darwin

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/crates/bff-app/binaries"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out/licenses"

name="typst-$triple.tar.xz"
curl -fsSL --proto '=https' -o "$work/$name" "https://github.com/typst/typst/releases/download/v$TYPST/$name"
echo "$TYPST_SHA  $work/$name" | shasum -a 256 -c - >/dev/null || { echo "Hash mismatch for $name" >&2; exit 1; }
mkdir "$work/typst" && tar -xJf "$work/$name" -C "$work/typst"

cp "$(find "$work/typst" -type f -name typst)" "$out/typst-$triple"
chmod 755 "$out/typst-$triple"
cp "$(find "$work/typst" -type f -name LICENSE | head -1)" "$out/licenses/typst-LICENSE.txt"
curl -fsSL --proto '=https' -o "$out/licenses/typst-NOTICE.txt" "https://raw.githubusercontent.com/typst/typst/v$TYPST/NOTICE"

"$out/typst-$triple" --version
