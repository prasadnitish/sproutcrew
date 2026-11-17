# Release notices and corresponding sources

`scripts/notices.py` now includes actual license/copyright text for runtime Rust packages, production UI packages, and the bundled font. It fails if a newly encountered dependency lacks local notice text. Packages whose published crate omits the repository's license file use the version-pinned upstream files in `upstream/manifest.json`. MIT/MPL standard texts are retained alongside upstream grants; no missing copyright dates have been invented.

## Current release: 2.0.5, without Pandoc

The owner authorized replacing Pandoc on November 8, 2026. The signed 2.0.5 bundle contains docx-rs, PptxGenJS, JSZip and embedded QuickJS for Office files, and retains Typst for PDF. The selected Office/JavaScript grants are MIT; Typst is Apache-2.0. The exact app contains no Pandoc program or Pandoc notice files. This removes the unresolved Pandoc source/build-material dependency from **new 2.0.5 distribution**; it does not retroactively clear older installers.

Prepared and verified beside the local installer:

- `SproutCrew_2.0.5_mpl-sources.tar.gz`: **158,302 bytes**, SHA-256 `699600de45e3d11bc5f8fbd2c823c4cc0e313d7249773c527f890ac2a828d684`. Five exact unmodified crate archives match Cargo.lock and the current source inventory. The archive also contains that inventory, MPL-2.0 and a scope README. No proprietary application source or user data is included.
- `SproutCrew_2.0.5_notices.zip`: **163,312 bytes**, SHA-256 `9fc518388125ba58abc5acd732279d626b704e639f242feed97b16b4ece86d0a`. Its three entries match the signed app's license/notice files byte-for-byte. Runtime Rust/UI/font grants and the complete embedded Office notices are included.

`scripts/distribution-materials.py` verifies the signed bundle identity and signature, package archive checksums and member paths, and then builds deterministic archives. It reopens both and compares every member to its input bytes. `target/releases/2.0.5/distribution-materials.json` records the resulting hashes. The runtime Cargo graph was independently checked against all five MPL inventory entries; source covered by those entries is unmodified. Version-pinned upstream grants and the objc2 license explanation remain in the notices.

Published November 9: both exact immutable archives are available alongside the versioned installer. The product download links [software notices](https://www.sproutcrew.app/software-notices), which exposes the covered source and notice URLs/checksums. Public downloads were fetched and compared byte-for-byte/hash with the local review artifacts before enabling `website/release.json`. Its availability is now true. See [the launch record](../PUBLIC_LAUNCH_2.0.5.md) for publication identity and verification. No proprietary application source was published.

Reproduce with:

```sh
python3 scripts/distribution-materials.py /path/to/SproutCrew.app target/releases/2.0.5
```

## Historical Pandoc candidates

See [Pandoc source follow-up](pandoc-source-followup.md) for the upstream build, masked freeze-record limitation and the owner-authorized [Discussion #12068](https://github.com/jgm/pandoc/discussions/12068). The last November 8 inspection was unanswered with zero comments; no new communication was sent as part of the replacement. The earlier binary-source requirement remains unresolved for **2.0.4 and older**, which are not offered for public download.

## 2.0.4 local distribution material

Prepared `target/releases/2.0.4/SproutCrew_2.0.4_mpl-sources.tar.gz`: **158,485 bytes**, SHA-256 `caf36e7f2b3f72cf0535185365e1cfcde8380570028530dfa7f90df8122ccdc8`. It contains the five unmodified crate archives in `source-inventory.json`, that inventory, the MPL text and a scope README. Every crate matches both the inventory and the current Cargo.lock checksum. Archive member paths were checked for traversal; the completed archive was reopened and every member compared with its input bytes. No proprietary app source or user data is included.

The six license/notice files in `target/releases/2.0.4/notices/` were copied byte-for-byte from the signed 2.0.4 app. Their hashes and the MPL archive are recorded in `target/releases/2.0.4/distribution-materials.json`. These are local review artifacts; no upload occurred. The manifest retains `public_available: false` and `complete_corresponding_source: false` because the **separate Pandoc source/build-material requirement remains unresolved**. This is an MPL-only source archive, not a complete source package for the installer.
