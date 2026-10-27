# Hermes for Tern

A Rust integration for using Hermes Agent inside Tern, with a planned coordination layer for independent Hermes and OMP actors in the same workspace.

**Current checkpoint: closed supported-v2 archive DDL diagnostic full local verification passed; product acceptance remains partial.** Offline preflight-recovery now refuses extra or altered tables,indexes,triggers,views,constraints and types by comparing the exact bounded schema inventory,not only valid rows/counts/hash. Full local campaign417Rust/377Python,two live ignored,zeroResourceWarning,20gates/nineindependentpackagepairs/259sources PASS;allsource/log/binary/lock hashes and16,243retainedmetadataidentities match,gatechildrenreaped/no liveownedgroups.22Rust/tenproductionCLIprocess methods include coherently rehashed negative fixtures,fresh/migrated-v2 and nonmutating positive controls. Parent took over interrupted tests/stub;no independent review approval claimed. Zero freshnative/live cases;postdocs/seal separate. No restore activation,current-journal comparison or worker-quiescence proof. Live/provider/window,peers/OMP,pixels,title binding,maxload andrelease remain open. See [current status](docs/STATUS.md).

**Previous checkpoint: intact retained-original operator-key publication full local verified.** The separately attested exact inode publication never generates a replacement,opens SQLite orstarts a runtime. Its sealed395Rust/367Python/257source campaign is historical after this diagnostic change;independent review remains unestablished.

**Previous checkpoint: owner plugin inventory full local/native synthetic verification passed.** Commands/F4 offers exact builtin Read /plugins with visible owner/profile/discovery disclosure,typed bounded metadata,raw-error suppression anddraft/stale/Unknown guards. Its sealed377Rust/357Python/76nativecase/200pair/253source campaign is historical after the recovery CLI/binary change,not freshnative/live acceptance. See [current status](docs/STATUS.md).

**Previous checkpoint: native MCP reload full local/native synthetic verification passed.** Runtime details offers local Review, then explicit Confirm for profile-wide MCP effects and consequential session reattach. One reload per consent, dedicated postresult readback, persistent Unknown and cross-control guards; busy/partial reports are not rollback, tool health or atomic configuration proof. Historical full campaign:362Rust/347Python,two live ignored,24gates/ninepackagepairs/72nativecases188pairs/249sources. Its postdocs/seal are retained separately;not current plugin-tranche or live/provider/window acceptance. See [current status](docs/STATUS.md).

**Previous checkpoint: optional-event authority repair passed 342 Rust/335 Python and 68 native cases/176 pairs.** Unsupported event kinds consume sequence without acquiring execution authority; approval/task-plan continuity is preserved. Its 242-file seal remains historical after MCP changes; retained failures and qualification limits remain in [current status](docs/STATUS.md).

**Previous checkpoint: delegated-agent roster full local/native synthetic verification passed.** Runtime details offers an explicit current-session `subagent.list` snapshot, not child control or an independent peer binding. Full campaign: 283 Rust/321 Python, two live opt-ins ignored, zero ResourceWarning, 24 gates/nine package pairs/68 native cases/172 image-layout pairs passed; all 234 source and log/binary/pair hashes match, with 68 confirmed zero exits and no owned survivors. Two parent-discovered paging defects observed RED then GREEN. Review completed/reconciled, not independent approval of later repairs. No automatic refresh, transcript, Steer/Stop, new-agent or provider action; Cargo.lock unchanged. Current pixel inspection still returns HTTP402. Live/provider/window, broader session UX, mixed-agent/load/restore/release gates remain open. See [current status](docs/STATUS.md).

**Previous checkpoint: native text attachment and full local/native synthetic verification passed.** F5 or Attach text file inside Commands offers separate Review/Stage/Add reference actions;an immutable local UTF-8 snapshot <=256 KiB is uploaded only after Stage. Adding its validated reference never sends the draft. Full campaign:253Rust/310Python,two live opt-ins ignored,zero ResourceWarning,24gates/nine package pairs/68native cases168image-layoutpairs PASS with hashes checked and68confirmed client exits. Eight staging attempts/connection,unknown/no retry,no remote cleanup or owner path reads. Failed generations retained;pixel inspection and independent final harness approval unavailable. Live/provider/window,mixed-agent,load/restore and release remain open. See [current status](docs/STATUS.md).

## Run what exists

Offline `recover-operator-key` finishes same-inode publication of an intact retained original only,with exact staging UUID,separately retained digest andexplicit original-key attestation. It refuses active ownership,conflicting orunsafe files andnever opens SQLite orreconstructs a secret. See [procedure and limitations](docs/operations.md#recover-an-intact-retained-original-operator-key). Do not use it to repair Hermes session turn leases.

Offline `preflight-recovery` inspects an exact settled archive,separately retained key expectation and exact supported-v2 DDL without opening live SQLite or restoring authority;see [recovery diagnostic](docs/operations.md#offline-archive-diagnostic-without-restoration). Five existing Rust/eight CLI plus22newRust/tennewCLI schema methods pass in the full local campaign. Successful DDL checking does not establish current-journal equivalence,stopped workers or permission to restore;load/recovery qualification remains open.

Versioned local package staging/update/rollback tooling is available without changing personal installations;see [package preflight](docs/operations.md#versioned-local-package-preflight). It is not a signed or licensed release and does not close live/runtime/load gates.

Owner-authorized local developer installation and non-inference checks are documented in [configuration](docs/configuration.md#local-developer-installation). The installed Tern package is a copy, not a development link; the optional Hermes peer package remains separate.

For the new native entry, run this checkout's binary inside a direct Tern shell:

```sh
cargo build --locked -p hermes-tern
./target/debug/hermes-tern open
# Optional explicit workspace for a new conversation:
./target/debug/hermes-tern open --cwd "$PWD"
```

The Hermes gateway must already be running. Listing/Close sends no prompt and creates/resumes no session; New/Resume opens only after human activation. The upstream `hermes` command is not redirected. No-argument `hermes-tern` still prints help. The personal binary/plugin copy from the earlier installation has not been updated by this tranche; its palette still follows its installed bytes. See [configuration](docs/configuration.md#native-session-entry-from-this-checkout).

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

The default runner requires Tern; it never silently skips missing gates. It retains a report/logs, runs no inference, installs no personal plugins and reports full-product/live gates as unestablished. The required 64 native cases cover synthetic conversation, Deny, Skip, Interrupt, disconnect, long-output folding, keyboard motion Off/On, history, launcher Close/New/Resume, Runtime details, Session model, existing-profile switching and explicit Commands at wide/narrow light/dark. Pixel inspection remains separate from automated geometry/readiness. A separate installed OMP no-inference startup/negotiation/state/EOF probe has passed; this is not an OMP connector or provider-backed collaboration.

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
