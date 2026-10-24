# Hermes for Tern

A Rust integration for using Hermes Agent inside Tern, with a planned coordination layer for independent Hermes and OMP actors in the same workspace.

**Current checkpoint: bounded history inspection locally verified; acceptance partial.** Complete autorun passes178Rust/191Python, all nine package pairs and36real-Tern synthetic-gateway cases. `history --session ID` reads an existing live session's display projection once, then browses bounded immutable pages without resume/submit/approval authority. Keyboard retained-text inspection also remains tested. Updated pixel review was unavailable; the delayed bounded source review is now reconciled with its one finding parent-remediated; no independent final-source, live/provider/window or full-history acceptance is claimed. See [current status](docs/STATUS.md) and [history contract](docs/history-viewer.md).

## Run what exists

Owner-authorized local developer installation and non-inference checks are documented in [configuration](docs/configuration.md#local-developer-installation). The installed Tern package is a copy, not a development link; the optional Hermes peer package remains separate.

From this checkout, with Rustup and Python 3.11 or later:

```sh
./verify.sh
cargo run --locked -p hermes-tern -- --help
cargo run --locked -p hermes-tern -- preview
cargo run --locked -p hermes-tern -- doctor
TERN_TSP=0 cargo run --locked -p hermes-tern -- preview
```

Inside Tern, `preview` uses the real Rust `tern-sdk`. Outside Tern it prints readable text. The dependency is pinned to a reviewed Git revision, and application dependencies are locked in `Cargo.lock`.

Optional real Tern rendering check, with no agent inference:

```sh
cargo build --workspace --bins --locked
python3 -B scripts/smoke_native.py
python3 -B scripts/smoke_native.py --fixture
python3 -B scripts/smoke_native.py --fixture --size 480x760
```

These check light/dark preview and wide/narrow labelled session-fixture rendering in private scratch. Result, diagnostic, image/layout and exact ownership paths are retained and printed. They do not load the Luau launcher, exercise live approval actions or prove a Hermes session. See [verification](docs/verification.md) and [current status](docs/STATUS.md).

The full gate also runs eighteen native-chat methods against the production CLI, an owned PTY and a synthetic loopback WebSocket gateway; no Hermes owner or provider is started. After building, the focused selector is `python3 -B -W error::ResourceWarning -m unittest discover -s tests -p test_native_chat.py -v`, with a private `TMPDIR` as described in [development](docs/development.md). Native input/render evidence and live-agent acceptance remain separate.

## Automatic verification

To automatically run the canonical suite, every package's locked tests/clippy and the real native matrix:

```sh
python3 -B scripts/verify_runtime.py
# Explicitly select a smaller scope on hosts without Tern:
python3 -B scripts/verify_runtime.py --local-only
# Or run the production native control matrix alone after building:
python3 -B scripts/smoke_native_chat.py
```

The default runner requires Tern; it never silently skips missing gates. It retains a report/logs, runs no inference, installs no personal plugins and reports full-product/live gates as unestablished. Native cases exercise synthetic conversation, Deny, Skip, Interrupt, disconnect, long-output folding, keyboard motion Off/On and history at wide/narrow light/dark. Pixel inspection remains separate from automated geometry/readiness checks. A separate installed OMP no-inference startup/negotiation/state/EOF probe has passed; this is not an OMP connector or provider-backed collaboration.

## Workspace layout

Executable crates live in `bin/hermes-tern` and `bin/testing`; library crates live in `crates/cli`, `crates/core`, `crates/gateway`, `crates/ui`, `crates/store`, `crates/supervisor` and `crates/omp`. The production binary delegates to the CLI library. `hermes-tern-testing` is a labelled native rendering fixture, not another agent. See [workspace ownership](docs/workspace-layout.md) for the dependency graph and per-package checks.

## Why two languages

Tern's Rust SDK is a Surface Protocol library for programs running in panes. Installable Tern plugins use Luau. Therefore the Rust executable owns the application, while the small Luau package supplies Tern palette integration. This is not a Rust plugin dynamic library.

## Intended direction

- Native Hermes conversations, tool progress, approvals and session controls without a second LLM loop.
- Preserve Hermes providers, profiles, memory, skills, plugins, MCP and delegation through supported runtime interfaces.
- Independent actors with explicit identity, attribution, bounded communication and separate write scopes.
- OMP coexistence through Tern's native agent APIs, with deeper interoperability conditional on verified protocol support.
- Recovery and observability that distinguish a queued message from a consumed message, and an interrupted run from a successful one.

The full capability set remains a target. [Current status](docs/STATUS.md) distinguishes tested behavior from unrun gates; [requirements](docs/product.md), [architecture](docs/architecture.md), and [roadmap](docs/ROADMAP.md) define completion.

## Documentation and contribution

Start at [docs/README.md](docs/README.md). Read [AGENTS.md](AGENTS.md) before changing code and [development](docs/development.md) for the verified local workflow. All project documentation lives under `docs/`; root files are entry points and contributor rules.

[Peer tools](docs/peer-tools.md) documents explicit operator binding, private credential handling, six tool contracts and isolated verification. The Python Hermes package is optional and separate from the Luau Tern launcher; neither is personally installed by the verifier.

Source repository: [0xharryriddle/hermes-tern](https://github.com/0xharryriddle/hermes-tern). The owner authorized the initial source push; this is experimental source, not a qualified product release. A project license has not yet been selected by the owner; the upstream SDK is MIT-licensed. Do not infer redistribution rights for this project's source from the dependency license.
