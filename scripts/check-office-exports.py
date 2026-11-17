#!/usr/bin/env python3
"""Validate generated Office QA files without extraction, macros, network access or provider calls."""
import pathlib
import posixpath
import sys
import xml.etree.ElementTree as ET
import zipfile

root = pathlib.Path(sys.argv[1]).resolve()
checked = 0
for file in sorted(root.rglob("*")):
    if file.suffix not in (".docx", ".pptx") or not file.is_file():
        continue
    with zipfile.ZipFile(file) as archive:
        names = set(archive.namelist())
        assert len(names) == len(archive.infolist()) < 10000, file
        assert "[Content_Types].xml" in names, file
        for info in archive.infolist():
            name = info.filename
            assert not name.startswith("/") and ".." not in pathlib.PurePosixPath(name).parts, name
            assert info.file_size < 32 * 1024 * 1024 and not info.flag_bits & 1, name
            assert not name.lower().endswith(("vbaproject.bin", ".exe", ".js")), name
            if not name.endswith((".xml", ".rels")):
                continue
            data = archive.read(name)
            assert b"<!DOCTYPE" not in data and b"<!ENTITY" not in data, name
            xml = ET.fromstring(data)
            if name.endswith(".rels"):
                owner = posixpath.dirname(name).replace("/_rels", "")
                if owner == "_rels":
                    owner = ""
                for rel in xml:
                    target = rel.attrib["Target"]
                    if rel.attrib.get("TargetMode") == "External":
                        assert target.startswith(("http://", "https://", "mailto:")), target
                    else:
                        path = posixpath.normpath(posixpath.join(owner, target))
                        assert path in names, (name, target)
        assert archive.testzip() is None, file
        if file.suffix == ".pptx":
            slides = [n for n in names if n.startswith("ppt/slides/slide") and n.endswith(".xml")]
            assert slides, file
            for name in slides:
                xml = archive.read(name)
                assert b'type="title"' in xml, (file, name, "missing semantic title")
                if b"<a:tbl>" in xml:
                    assert b'firstRow="1"' in xml, (file, name, "missing table header")
    checked += 1
assert checked > 0, "No Office files found"
print(f"PASS: {checked} Office archives; XML, relationships, safe links, title/header semantics and CRCs")
