# Development

Status: owner-authorized implementation, split into binary and library crates. [Architecture](architecture.md) owns runtime boundaries; [workspace layout](workspace-layout.md) owns package responsibilities.

## Prerequisites

- Git, Rustup, a C toolchain/linker, and Python 3.11 or later (stdlib-only document tests).
- Rust 1.98.0 with rustfmt and clippy, selected by `rust-toolchain.toml`.
- Network access for the first pinned Git/crates dependency download; a lockfile does not make a fresh build offline.
- Linux x86-64 Luau 0.743 for the window callback harness. Tests fetch the official archive into a private test-tool cache and verify archive/binary SHA-256; or set `HERMES_TERN_TEST_LUAU` to the same private hash-verified binary. No PATH/profile/plugin installation. This first download also needs network access.
- Tern is optional for ordinary build/tests and required for native rendering. Current preview and wide/narrow session-fixture smoke passed on Tern 0.9.5 (2863c37), Linux.
- Hermes and OMP are not needed to run the preview or the mandatory verification gate.

The initially attempted version-named 1.98.1 installation on the development host was incomplete despite a working stable alias; the project uses the independently checked 1.98.0 toolchain instead. This is a host setup finding, not an SDK incompatibility claim.

## Current repository layout

```text
bin/hermes-tern/          Production entry point and executable/process tests
bin/testing/              Labelled native fixture executable
crates/cli/               Argument parsing and command dispatch
crates/core/              Safe errors, strict JSON and terminal-safe text
crates/gateway/           Discover-only helper, transport and session driver
crates/ui/                Native/plain loops, view, composer and preview
crates/store/             SQLite actor mailbox and task leases
crates/supervisor/        Authenticated local Unix IPC
crates/omp/               Bounded proposals and irreversible local dispatch fence
plugins/hermes-tern/      Luau palette package
plugins/hermes-tern-peer/ Optional Python Hermes tool adapter
scripts/                 docs integrity and isolated native smoke
tests/                   Python documentation/package tests
docs/                    canonical design, research, status and roadmap
.github/workflows/       CI invoking the same local gate
```

All paths are relative to the repository root. The library packages hold existing implementations, not empty scaffolds. Peer tools have isolated loader/registry and process tests; actual provider-backed collaboration remains open. Binary crates may depend on libraries, never the reverse. The gateway helper is embedded from `crates/gateway/bridge/hermes_discover.py`.

## Build and verify

From the repository root:

```sh
./verify.sh
cargo build --locked -p hermes-tern
cargo build --workspace --bins --locked
cargo run --locked -p hermes-tern -- --help
cargo run --locked -p hermes-tern -- preview
TERN_TSP=0 cargo run --locked -p hermes-tern -- preview
```

`./verify.sh` checks formatting, checks all targets, denies clippy warnings, runs Rust tests, builds both executable targets, runs Python negative controls/package/process checks, checks docs navigation/task integrity and whitespace. The explicit binary build prevents Python checks from exercising a stale executable. It exits nonzero on any failure. It does not claim a Hermes session or remote CI run.

For explicit automatic local/native execution use `python3 -B scripts/verify_runtime.py`. It runs the same wrapper, derives standalone package test/clippy commands from Cargo metadata, then static smoke and the real production-native matrix. `--local-only` deliberately selects the smaller scope; default mode requires Tern and fails if missing. `--out /absolute/new/path` refuses existing paths. Logs are bounded (8 MiB per command), deadlines are finite, and command exit/owned process identity/source witnesses are retained. Reports mark live Hermes/provider, window plugin, parallel actors, OMP and license/publication separately. A `running` or absent report after an external kill is incomplete, not PASS. Outer automation must allow the command's complete bounded campaign rather than a short per-call deadline; use a managed process for long runs and never run a second writer/test campaign while the first survives.

`python3 -B scripts/smoke_native_chat.py` runs 20 cases by default: conversation, Deny, Skip, Interrupt, disconnect × wide/narrow × light/dark. Optional `--case`, `--size`, `--theme` select a narrower, explicitly recorded matrix. Real `tern serve`/control replies drive the production client; an exact editor/control-tree readiness predicate precedes each screenshot, while main-text waits and backend RPC counts prove separate facts. Empty editors expose placeholder text plus the `empty` class, not an empty tree string. No approval is issued except Deny to a synthetic fixture. The native disconnect case verifies complete draft/status/Detach; attempted resubmission refusal remains in the real-PTY test. The original shot-only harness exposed incomplete draft pixels and timing-sensitive clicks and is retained as historical evidence, not reused as a current image verdict.

Startup allowances are test infrastructure,not product latency assertions. The shared helper uses one fifteen-second readiness budget;death or late success fails. It monitors the supervisor while a fixed status subprocess is pending and kills/reaps a timed-out probe before another read-only attempt. Cleanup has a separate bounded reap allowance;generic mutation helpers never retry. Native accept begins after private setup/client launch. Operation,expiry,fault-barrier and shutdown deadlines are unchanged. Renderer-specific limits remain bounded within sixty seconds. Private roots and environment isolation do not imply an OS sandbox. The complete default autorun passes;prior failed/interrupted logs remain retained.

The archive/migration regressions also require `cc`: they compile small test-only Linux interposers with warnings denied into private fixture directories. `LD_PRELOAD` is set only for the owned coordinator/offline migrator, never the test runner or personal Hermes/Tern processes. The fsync fault selects one exact directory; the archive-identity barrier selects the owned WAL write-lock descriptor or archive-root fsync and pauses for a bounded interval. The Python fixture mutates only its synthetic archive, checks source/schema rollback and includes unchanged/repair success controls. Retained-staging tests additionally inject database/manifest/bundle-directory/archive-root fsync errors, a final-name creation race and post-sync manifest mutations. The directory fault waits for the manifest: SQLite itself can fsync its output parent before the bundle is complete. Lock offsets are source-qualified to pinned SQLite 3.53.2. Missing compiler/interposition support or an unreached barrier fails the gate rather than silently skipping it. Fixtures retain generated libraries/logs with ownership inventories.

Private-publication reuse tests use the same exact-path libc fault/barrier fixtures for binding and operator-key files. Faults are confined to owned supervisors; no secret content is printed or copied into repository evidence. Startup may cause SQLite directory fsync before key validation, so tests that distinguish invalid-key validation from private-file sync target the exact key descriptor rather than treating all state-directory syncs as key operations. Post-sync probes alter only synthetic file contents/inodes/directories and include unchanged/repair controls.

State-projection tests run with `cargo test --locked -p hermes-tern-store --test projection` and `python3 -B -W error::ResourceWarning -m unittest discover -s tests -p test_state_projection.py -v`. Ten Rust tests and six production-binary methods cover complete enumeration, revision/epoch refusal, byte/precision bounds, migration witnesses and read-only behavior. Fixtures compose the existing recovery/migration helpers rather than inherit their tests. The concurrent-read control intentionally commits through a second SQLite connection inside its private fixture; this is adversarial test access, not a supported second production coordinator. Successful pages must match their requested revision, otherwise restart with no items. No provider or personal runtime is involved.

Focused checks:

`cargo test --locked -p hermes-tern-omp --all-targets` exercises four library methods. After building the binary, `python3 -B -W error::ResourceWarning -m unittest discover -s tests -p 'test_omp*.py' -v` runs the production exchange and actual Luau entry. Seventeen methods include cross-process arm races, lost replies, private/corrupt inventory, state-parent/file/directory fsync EIO and post-fsync SIGKILL, and two executions of 31 synthetic callback scenarios. The harness invalidates each WindowCx generation, repeats callbacks, refuses missing consent/unsafe targets and checks exact text/fixed argv. It does not execute a real Tern window or a provider, and its `tern.json`/host callbacks are synthetic. Review canvas content is inert but persists in Tern state; avoid secrets.

Native chat lifecycle runs with `python3 -B -W error::ResourceWarning -m unittest discover -s tests -p test_native_chat.py -v` after the production binary build. Eighteen methods need Linux PTYs/loopback sockets, not Tern, Hermes or credentials; they inspect actual CLI/SDK RPCs, ACK starvation, composed error/gone/action events and termios restoration. Private synthetic discovery/HOME, ownership records, joined gateway threads and reaped CLIs remain required; forced cleanup fails. `cargo test --locked -p hermes-tern-ui --test view --test native` runs ten tests, including deterministic single-read lifecycle checks. The surface-aware adapter uses the pinned SDK's inbox-before-waker-before-read order: one wake is a queue-end barrier, not a polling delay. Missing waker or an undrained inbox after 8192 iterations refuses. Tests leave a second scripted chunk unread to prove the adapter never reads past that barrier; SDK close still performs its own bounded raw drain/restoration. No runtime-only test flags or replacement SDK were added. Keep protocol tests, actual Tern pixels and provider-backed acceptance separate.

Retained-credential recovery runs with `cargo test --locked -p hermes-tern-store --test credential` and `python3 -B -W error::ResourceWarning -m unittest discover -s tests -p test_credential_recovery.py -v`. Four Rust tests verify exact current authority, callback failure/non-mutation, rotated refusal and a separately opened SQLite writer blocked during publication but successful afterward. Nine Python methods exercise the real binary, fixed staged/final names, peer/strict-envelope/unsafe-file refusal, missing/FIFO candidates, capacity, lost reply/restart and preservation of an existing peer binding. The exact-path credential interposer is armed after startup and distinguishes directory-entry, file, pre-rename directory and post-rename directory fsync failures. The existing barrier exercises unchanged and substituted bytes/inodes/permissions/credential-directory/state-directory cases, plus a racing final target. These are bounded Linux fixtures, not physical power loss or universal same-user exclusion; all generated secret files stay private and retained, never printed in evidence.

```sh
cargo test --locked -p hermes-tern --test cli
cargo test --locked -p hermes-tern-gateway --test transport
cargo test --locked -p hermes-tern-ui --test view
cargo test --locked -p hermes-tern-store --all-targets
python3 -B -m unittest discover -s tests -v
python3 -B scripts/check_docs.py
```

The root default package is `hermes-tern`. Use `--workspace` to check all packages, or `-p <package>` for each boundary; workspace feature unification alone does not prove standalone packages build. `cargo run --locked -p hermes-tern-testing` requires a native Tern pane and displays only fixture data, with no model calls or tool execution.

## Native renderer smoke

```sh
cargo build --workspace --bins --locked
python3 -B scripts/smoke_native.py
python3 -B scripts/smoke_native.py --fixture
python3 -B scripts/smoke_native.py --fixture --size 480x760
```

The script creates an isolated private scratch directory with short child names, redirects HOME/HERMES_HOME/config/state/cache/log/runtime/TMPDIR roots and daemon socket, and runs Tern's `shot` scenarios. Only PATH, locale, timezone and TERM are inherited; provider/session/preload/Python environment variables are not passed. The selected binary is either the production static preview or the explicitly labelled testing fixture. In Tern 0.9.5 the main-region marker succeeds where a dock-only marker wait times out, so fixture readiness uses `Hermes / FIXTURE` and independently checks dock/editor/control geometry. Light/dark pairs must have matching PNG dimensions and visible in-viewport required regions. `--size` accepts bounded WIDTHxHEIGHT with optional scale @1..4; it selects a probe, not a promise that every accepted size is visually qualified.

Each run prints its private output directory and retains `result.json`, `renderer.log`, image/layout pairs on success, and `ownership.json` with exact paths/inode identities. Renderer failures propagate, zero exit with no output refuses, and timeout partial diagnostics are retained. No files are deleted. Keep TMPDIR short enough for Unix-domain socket names; nested long roots can make Tern listeners unavailable. This is configuration isolation, not an OS sandbox. No personal plugin is installed and no Hermes/OMP runtime is invoked. `python3 -B -m unittest discover -s tests -p test_native_smoke.py -v` runs eight harness contract tests without requiring Tern; the full verification wrapper builds both binaries first. Controlled geometry and injected runner errors in those tests are not actual native renders.

Keep the scratch output while diagnosing a failure. Do not substitute a fabricated screenshot. The script's geometry check and Tern's text assertion are separate from pixel-level review. Plugin action execution requires a real isolated window and is not included here.

## Contribution sequence

For the optional real Hermes directory-loader/registry check, use `scripts/smoke_peer_plugin.py` with an explicitly supplied pinned checkout and its Python. It creates an isolated scratch home, loads only the copied peer plugin, and invokes the real registry against a fixture supervisor without launching an agent. [Peer tools](peer-tools.md#reproducible-checks) gives the command and distinguishes this from provider-backed acceptance.

1. Read [AGENTS.md](../AGENTS.md), the requirement and its task row.
2. Verify the relevant upstream contract at the pinned/deployed version. Record unknowns before coding.
3. Create a narrowly scoped behavior test and observe the expected failure.
4. Implement the smallest complete vertical slice, not placeholder adapters.
5. Run focused checks followed by `./verify.sh` and the lane-specific gate.
6. Update [STATUS](STATUS.md), [ROADMAP](ROADMAP.md) and [tasks.json](tasks.json) together, with measured evidence.
7. Review all untracked files explicitly. Commit/push only when the owner authorizes it.

## Dependency and release policy

The root manifest pins the SDK by full Git revision; the lockfile fixes transitive resolution. To update, inspect upstream changes, update only intended pins, regenerate the lockfile intentionally, and run local/native compatibility tests. Do not claim the minimum supported Rust version from a single build; the toolchain is a tested pin, not an MSRV promise.

The initial CI is configured for Linux and does not install proprietary/beta Tern or provider credentials. macOS/Windows are future qualification targets. Publication requires owner license selection, release instructions, security review and compatibility evidence.
