# Local Word, PowerPoint and PDF exports

Owner decision, November 8, 2026: replace Pandoc with **docx-rs 0.4.22** and **PptxGenJS 4.0.1**, keep **Typst 0.15.1**. This supersedes the previous choice to bundle Pandoc. V1 and older candidate bytes are unchanged.

## Architecture

`clientfiles::make` scans the project title, subtitle and both complete source texts for secrets before creating its output folder. It strips existing team-only sections, then uses `office_export` to parse Markdown with pulldown-cmark into typed blocks and spans. It never evaluates model-authored code, raw HTML or converter directives.

- Word: docx-rs produces real DOCX paragraphs, heading styles, tables, inline formatting and allowed hyperlinks. Documents use brand fonts/colors, readable paragraph spacing and padded tables.
- PowerPoint: PptxGenJS consumes only JSON data in a fresh embedded QuickJS runtime. It has no filesystem, process, environment, browser DOM, module loader or network bindings. No user-installed Node, cloud conversion, runtime download or webview is involved. Rust controls output creation. The fixed layout code adds semantic title placeholders, content before page numbers, editable tables with explicit header semantics, speaker notes and numbered continuation slides. The table-header flag is added only to app-generated OOXML using the same local JSZip, not by patching the third-party library or accepting source XML.
- PDF: Typst remains the engine. Client document PDFs now consume the same parsed blocks, with real tables, inline formatting, hyperlinks and preserved list numbers. Content is serialized only into quoted strings inside fixed template expressions. Run reports retain their existing conservative Markdown-to-Typst renderer.
- Packages retain original Markdown, sources, decisions, reports and hash/size manifests. The automatic final-files cache has a new renderer revision, so it cannot reuse previous Pandoc files. Partial-export warnings persist; adding Typst creates a new complete set without overwriting the old partial set.

## Supported content and limits

Headings, paragraphs, bold/italic/strike, inline and fenced code, nested/ordered/task lists, quotes, tables, Unicode text and HTTP(S)/mailto links are supported. Raw markup stays literal. Explicit `::: notes` containers outside fenced code become speaker notes; malformed unclosed containers stay visible. Images remain labelled text references with a visible warning: no media URL or local path is fetched. Source diagrams remain separate artifacts; embedded Office diagrams/math and exact cross-format layout parity are not promised.

Maximum text input is 1,000,000 bytes, with 2,000 blocks, 16 nesting levels, 32 table columns and 2,000 rows per table. PowerPoint uses a 128 MiB JS heap, 2 MiB stack, 30-second interrupt deadline and 250-slide cap. Columns are grouped at six per slide; long content continues rather than being truncated. Oversized table headers fail explicitly. Output is limited to 32 MiB and written only to a new owner-only file; an existing file or symlink is refused. These bounds are not an OS sandbox claim.

## Rebuild JavaScript vendor files

```sh
cd crates/bff-core/export-js
npm ci --ignore-scripts
npm audit
cd ../../..
node scripts/vendor-office.mjs
```

The script concatenates unchanged upstream JSZip and PptxGenJS browser files and records each input and final SHA-256. It deliberately uses locked **JSZip 3.10.2**, instead of the older embedded ZIP library in PptxGenJS's prebuilt bundle. `image-size` is pinned to patched **2.1.0** in the dependency tree; Node-only media loading is not exposed to the runtime. The vendor manifest, package lock and full notices are committed. MIT is selected for dual-licensed JSZip. Notices include the separate QuickJS engine grant and exact upstream docx-rs/rquickjs grants omitted from their published crate roots. The existing MPL dependencies still have their source archive. `scripts/distribution-materials.py SIGNED_APP RELEASE_DIRECTORY` verifies the signed app version, absence of Pandoc, archive checksums against Cargo.lock and copied notice bytes; it prepares local material without publishing.

## Verification

The parser/Office fixtures first failed against unimplemented functions. Additional tests reproduced direct-export secret leakage and missing speaker-note handling before their fixes. Current fixtures cover structure, formatting, safe links, Unicode, hostile markup, no media reads, no overwrite, size limits, long-slide continuation, real editable tables and notes. Existing helper-only tests now expect built-in Office output and optional-PDF warnings, preserving latest-result and no-overwrite assertions.

`tests/export_pipeline.rs` runs the real app-core export paths with a synthetic replay engine and local sources: client files, final-file cache, decision log, run report and full package. Every package hash and size is checked, required outputs must exist, and missing/incomplete lists must be empty. It uses no account, credential or provider request. All Office fixtures always run; the complete PDF integration check requires detected Typst.

For retained visual evidence, use a new directory:

```sh
cargo run -p bff-core --example export_smoke -- target/office-export-qa-NEW
python3 scripts/check-office-exports.py target/office-export-qa-NEW
```

Inspect the generated DOCX/PPTX in Word/PowerPoint and render the PDF pages before clearing the candidate. XML/round-trip tests establish structural checks, not pixel-perfect Office output or model-writing quality.

November 8 verification: **1,052 Rust tests passed, 2 intentional live-web ignores**, fmt and all-target clippy passed; **66 UI tests**, production UI build and **3 website tests** passed. Final suite log: `/tmp/sprout-office-freeze-checks.log`. The retained full app-core fixture is `target/office-export-qa-2.0.5-final-review/`; all **39 package files** match their manifests, with no missing/incomplete parts. Six Office archives passed XML/relationship/CRC and semantic title/header validation. Word and PowerPoint opened the corrected files without repair prompts, and both reported **Accessibility: Good to go** for these fixtures. Speaker notes were observed in PowerPoint. The final one-page document PDF was rendered and visually checked after its table/link/numbering fix. Private UI proof stays under `target/private-launch-evidence/office-2.0.5/`, outside the public payload. The signed, notarized 2.0.5 installer subsequently passed native preflight and generated all client formats, report, decision log and a 24-file package from retained results with no provider requests; see [the release review](RELEASE_CANDIDATE_2.0.5.md). The final website source/notices gate adds a fourth passing site test.
