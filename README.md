# SproutCrew

SproutCrew is a Mac app that runs a small crew of AI agents on a piece of work, under supervision. One model writes, a model from a different maker checks, and you approve the plan, the guardrails and the cost limit on one screen before anything runs.

- Website and download: <https://www.sproutcrew.app>
- What changed in each version: <https://www.sproutcrew.app/changelog>
- What is planned: <https://www.sproutcrew.app/roadmap>
- How it is built and released: <https://www.sproutcrew.app/how-its-built>

The app is free while it is in beta. You bring your own API keys (OpenRouter, Anthropic, OpenAI, Google Gemini, or your own service). It runs on Macs with Apple Silicon on macOS 15 or later. It has no account, no cloud sync and no analytics.

## Source-available, not open source

The code is published so that anyone can read it, check what the app does with their keys and files, and build it for themselves. It is under the [PolyForm Shield License 1.0.0](LICENSE.md). In plain words, and with the licence text as the final word:

- You may read, run, fork and change the code.
- You may share copies, including changed ones, as long as the licence and the copyright notice go with them.
- You may not use it to provide a product that competes with SproutCrew, whether paid or free. A rebuilt or renamed SproutCrew offered to other people as a substitute is the clearest case.
- The SproutCrew name and logo are not licensed.

Bundled and vendored third-party code keeps its own licences. They are listed in `docs/licenses/` and `crates/bff-core/export-js/vendor/`.

## What is in this repository

This is a snapshot of the source for each public release, published without the private working history.

| Path | What it holds |
| --- | --- |
| `crates/bff-core` | Everything the app decides: plans and approval, the action policy, cost and token limits, redaction, providers, sources, exports. No interface code. |
| `crates/bff-app` | The Mac app: a Tauri shell and the React interface in `ui/`. |
| `crates/m0` | The first proof harness, kept for its tests. |
| `fixtures/redacted` | Recorded provider events with private details removed, used by the tests. |
| `website` | The pages at sproutcrew.app, including the changelog and roadmap. |
| `scripts` | Release, notices and vendoring scripts. |
| `docs` | The action policy, the latest security review, export and notification notes, and third-party licences. |

The internal names `bff-core` and `bff-app` come from the project's working name.

## Three rules the code keeps

1. Text written by a model, or read from a file or a web page, is data. It never becomes a command, a permission or an address to fetch.
2. An approval covers the plan, the guardrails, the sources and the limits together. Change any of them and the approval no longer holds.
3. An unknown cost is shown as unknown, never as zero, and a request whose price is unknown is refused on a connection with a dollar limit.

## Build it yourself

You need a Mac with Apple Silicon, Xcode command-line tools, Rust (1.97 was used for 2.1.0), Node 22 or later, and the Tauri command-line tool (`cargo install tauri-cli`).

```bash
./scripts/fetch-helpers.sh
```

```bash
cd crates/bff-app/ui && npm ci && npm run build && npm test
```

```bash
cargo test
```

```bash
cd crates/bff-app && cargo tauri build --debug --bundles app
```

The first command downloads Typst, which makes the PDFs, and checks its hash. A build you make yourself is not signed or notarized, and Google Docs is unavailable in it because that needs a Google client of your own.

## Contributing

Bug reports are welcome as issues. Pull requests are not being accepted yet, because the licence question for outside contributions has not been settled. For a security problem, see [SECURITY.md](SECURITY.md).

Made by [Nitish Prasad](https://www.nitishprasad.com).
