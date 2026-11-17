#!/usr/bin/env python3
"""Writes the third-party notices that ship in the app's licenses folder: every Rust crate compiled into
SproutCrew for macOS, and every package in the interface's production bundle, each with its license and
source. The bundled Typst program has its own license files next to it."""

import json
import pathlib
import subprocess
import sys


def license_texts(folder, explicit=None):
    """Keep the actual notices, including copyright lines, rather than links alone."""
    folder = pathlib.Path(folder)
    candidates = list(folder.iterdir())
    if explicit:
        candidates.append(folder / explicit)
    for name in ("LICENSES", "licenses"):
        if (folder / name).is_dir():
            candidates.extend((folder / name).rglob("*"))
    texts = []
    for file in sorted(set(candidates)):
        if file.is_symlink() or not file.is_file():
            continue
        if not (file.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"))
                or file.parent.name.lower() == "licenses" or (explicit and file == folder / explicit)):
            continue
        if not file.resolve().is_relative_to(folder.resolve()) or file.stat().st_size > 1024 * 1024:
            raise ValueError(f"Review license file: {file.name}")
        texts.append((str(file.relative_to(folder)), file.read_text(errors="replace")))
    return texts

root = pathlib.Path(__file__).resolve().parent.parent
out = root / "crates/bff-app/binaries/licenses/THIRD-PARTY-NOTICES.txt"
for name in ("typst-LICENSE.txt", "typst-NOTICE.txt"):
    if not (out.parent / name).is_file():
        sys.exit(f"Missing helper notice {name}; run scripts/fetch-helpers.sh before release")

meta = json.loads(
    subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout
)
packages = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

# Walk normal (not build or dev) dependencies from the app crate.
app = next(p["id"] for p in meta["packages"] if p["name"] == "bff-app")
seen, todo = set(), [app]
while todo:
    pid = todo.pop()
    if pid in seen:
        continue
    seen.add(pid)
    for dep in nodes[pid]["deps"]:
        if any(k["kind"] is None for k in dep["dep_kinds"]):
            todo.append(dep["pkg"])

ours = {p["id"] for p in meta["packages"] if p["source"] is None}
rust = sorted(
    (packages[i]["name"], packages[i]["version"], packages[i].get("license") or "see source",
     packages[i].get("repository") or "")
    for i in seen - ours
)
full_notices = []
missing_text = []
notice_cache = root / "docs/licenses/upstream"
fallbacks = json.loads((notice_cache / "manifest.json").read_text())
for pid in sorted(seen - ours, key=lambda i: (packages[i]["name"], packages[i]["version"])):
    package = packages[pid]
    texts = license_texts(pathlib.Path(package["manifest_path"]).parent, package.get("license_file"))
    if not texts:
        fallback = fallbacks.get(f'{package["name"]} {package["version"]}')
        if fallback:
            filename = fallback["file"]
            notice = (notice_cache / filename).read_text()
            texts = [(filename, f'Upstream: {fallback["source"]}\nAuthors from package metadata: {", ".join(fallback["authors"])}\n\n{notice}')]
            if "MIT" in fallback["license"]:
                texts.append(("MIT license text", (notice_cache / "MIT.txt").read_text()))
            if fallback["license"] == "MPL-2.0":
                texts.append(("MPL 2.0 license text", (notice_cache / "MPL-2.0.txt").read_text()))
        else:
            missing_text.append(f'{package["name"]} {package["version"]}')
    full_notices.extend((f'{package["name"]} {package["version"]} / {name}', text) for name, text in texts)
    if package["name"] == "rquickjs-sys":
        engine_license = pathlib.Path(package["manifest_path"]).parent / "quickjs/LICENSE"
        full_notices.append(("QuickJS engine / MIT license", engine_license.read_text()))

office_vendor = root / "crates/bff-core/export-js/vendor"
office_manifest = json.loads((office_vendor / "manifest.json").read_text())
import hashlib
if hashlib.sha256((office_vendor / office_manifest["bundle"]).read_bytes()).hexdigest() != office_manifest["sha256"]:
    sys.exit("Office JavaScript bundle does not match its reviewed manifest")
full_notices.append(("Office JavaScript / upstream license texts", (office_vendor / "THIRD-PARTY-NOTICES.txt").read_text()))

ui = root / "crates/bff-app/ui"
lock = json.loads((ui / "package-lock.json").read_text())
web = []
for path, info in lock.get("packages", {}).items():
    if not path.startswith("node_modules/") or info.get("dev") or info.get("devOptional"):
        continue
    name = path.split("node_modules/")[-1]
    manifest = json.loads((ui / path / "package.json").read_text()) if (ui / path / "package.json").exists() else {}
    repo = manifest.get("repository", "")
    repo = repo.get("url", "") if isinstance(repo, dict) else repo
    web.append((name, info.get("version", ""), info.get("license") or manifest.get("license") or "see source", repo))
    texts = license_texts(ui / path) if (ui / path).is_dir() else []
    if not texts:
        missing_text.append(f'{name} {info.get("version", "")}')
    full_notices.extend((f'{name} {info.get("version", "")} / {filename}', text) for filename, text in texts)
web.sort()

lines = [
    "SproutCrew third-party notices",
    "",
    "SproutCrew includes the open-source software below. Each is used under its own license, named here; the",
    "license and copyright texts follow the index below. Source addresses identify their upstream projects.",
    "",
    "Bundled programs, run as separate programs and shipped unchanged:",
    "  Typst 0.15.1, Apache-2.0, https://github.com/typst/typst (license: typst-LICENSE.txt)",
    "  Office JavaScript: PptxGenJS 4.0.1 (MIT), JSZip 3.10.2 (MIT selected), dependencies/notices below",
    "",
    f"Rust crates ({len(rust)}):",
    *[f"  {n} {v}, {l}, {r}".rstrip(", ") for n, v, l, r in rust],
    "",
    f"Interface packages ({len(web)}):",
    *[f"  {n} {v}, {l}, {r}".rstrip(", ") for n, v, l, r in web],
    "",
]
font_license = ui / "public/brand/fonts/OFL.txt"
full_notices.append(("DM Sans font / SIL Open Font License", font_license.read_text()))
for title, text in full_notices:
    lines.extend(["=" * 72, title, "=" * 72, text, ""])
if missing_text:
    sys.exit("Missing license text; release stopped: " + ", ".join(missing_text))
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text("\n".join(lines))
print(f"{out}: {len(rust)} crates, {len(web)} packages")
missing = [n for n, _, l, _ in rust + web if l == "see source"]
if missing:
    print("No license field:", ", ".join(missing), file=sys.stderr)
