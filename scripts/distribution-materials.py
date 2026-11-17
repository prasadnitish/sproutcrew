#!/usr/bin/env python3
"""Prepare local notices and exact unmodified MPL sources beside a signed candidate. Never publishes."""
import gzip
import hashlib
import io
import json
import pathlib
import plistlib
import shutil
import subprocess
import sys
import tarfile
import tomllib
import zipfile

root = pathlib.Path(__file__).resolve().parent.parent
app, out = (pathlib.Path(arg).resolve() for arg in sys.argv[1:3])
version = json.loads((root / "crates/bff-app/tauri.conf.json").read_text())["version"]
assert plistlib.loads((app / "Contents/Info.plist").read_bytes())["CFBundleShortVersionString"] == version
subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
assert not any("pandoc" in p.name.lower() for p in (app / "Contents").rglob("*")), "Old Pandoc material in app"
inventory = json.loads((root / "docs/licenses/source-inventory.json").read_text())
lock = tomllib.loads((root / "Cargo.lock").read_text())
checksums = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
cache = pathlib.Path.home() / ".cargo/registry/cache"
members = {}
for package in inventory["packages"]:
    expected = package["sha256"]
    assert checksums[(package["name"], package["version"])] == expected
    candidates = list(cache.glob("*/" + package["archive"]))
    assert candidates, package["archive"]
    data = candidates[0].read_bytes()
    assert hashlib.sha256(data).hexdigest() == expected
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as crate:
        for entry in crate:
            assert not pathlib.PurePosixPath(entry.name).is_absolute() and ".." not in pathlib.PurePosixPath(entry.name).parts
    members["crates/" + package["archive"]] = data
members["source-inventory.json"] = json.dumps(inventory, indent=2).encode() + b"\n"
members["MPL-2.0.txt"] = (root / "docs/licenses/upstream/MPL-2.0.txt").read_bytes()
members["README.txt"] = (f"SproutCrew {version}: sources for five unmodified MPL-covered dependencies.\n"
    "The original .crate archives match Cargo.lock checksums. No proprietary app source or user data is included.\n"
    "Word uses docx-rs (MIT), PowerPoint uses PptxGenJS (MIT), QuickJS is MIT and Typst is Apache-2.0.\n"
    "New installers contain no Pandoc. Full third-party notices accompany the installer.\n"
    "This local candidate has not been publicly released.\n").encode()
out.mkdir(parents=True, exist_ok=True)
archive = out / f"SproutCrew_{version}_mpl-sources.tar.gz"
with archive.open("wb") as stream, gzip.GzipFile(fileobj=stream, mode="wb", filename="", mtime=0) as zipped, tarfile.open(fileobj=zipped, mode="w") as tar:
    for name, data in sorted(members.items()):
        entry = tarfile.TarInfo(name)
        entry.size = len(data); entry.mtime = 0; entry.mode = 0o644
        tar.addfile(entry, io.BytesIO(data))
with tarfile.open(archive) as tar:
    assert set(tar.getnames()) == set(members)
    for name, data in members.items():
        assert tar.extractfile(name).read() == data
notices = out / "notices"
notices.mkdir(exist_ok=True)
for name in ["THIRD-PARTY-NOTICES.txt", "typst-LICENSE.txt", "typst-NOTICE.txt"]:
    shutil.copyfile(app / "Contents/Resources/licenses" / name, notices / name)
notices_archive = out / f"SproutCrew_{version}_notices.zip"
notice_names = ["THIRD-PARTY-NOTICES.txt", "typst-LICENSE.txt", "typst-NOTICE.txt"]
with zipfile.ZipFile(notices_archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zipped:
    for name in sorted(notice_names):
        entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
        entry.compress_type = zipfile.ZIP_DEFLATED
        entry.external_attr = 0o100644 << 16
        zipped.writestr(entry, (notices / name).read_bytes(), compresslevel=9)
with zipfile.ZipFile(notices_archive) as zipped:
    assert set(zipped.namelist()) == set(notice_names)
    assert zipped.testzip() is None
    for name in notice_names:
        assert zipped.read(name) == (app / "Contents/Resources/licenses" / name).read_bytes()
def record(file):
    data = file.read_bytes()
    return {"path":str(file.relative_to(out)), "bytes":len(data), "sha256":hashlib.sha256(data).hexdigest()}
manifest = {"version":version, "public_available":False, "pandoc_included":False,
    "mpl_source":record(archive), "packages_verified":len(inventory["packages"]),
    "notices":[record(notices / name) for name in sorted(notice_names)],
    "notices_archive":record(notices_archive)}
(out / "distribution-materials.json").write_text(json.dumps(manifest, indent=2)+"\n")
print(json.dumps(manifest, indent=2))
