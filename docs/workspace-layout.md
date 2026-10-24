# Binary and library workspace layout

Status: layout implemented and locally verified (T-15). The owner replaced the earlier single-package layout with library crates under `crates/` and binary crates under `bin/`. The executable remains `hermes-tern`; no `crates/hermes-tern` compatibility package remains. Live/native acceptance remains a separate gate, detailed below.

## Ownership and dependency direction

| Directory | Cargo package | Responsibility | Local dependencies |
| --- | --- | --- | --- |
| `bin/hermes-tern` | `hermes-tern` | Production process entry; CLI and process acceptance tests | cli |
| `bin/testing` | `hermes-tern-testing` | Labelled native rendering fixture executable; no inference | core, gateway, ui |
| `crates/cli` | `hermes-tern-cli` | Argument parsing, command dispatch, operator/peer CLI | core, gateway, ui, store, supervisor, omp |
| `crates/core` | `hermes-tern-core` | Safe errors, strict JSON parsing and display-text sanitization | none |
| `crates/gateway` | `hermes-tern-gateway` | Hermes options, discovery helper, WebSocket transport, wire/session state and conversation driver | core |
| `crates/ui` | `hermes-tern-ui` | Tern/native and plain presentation loops, composer, view and preview | core, gateway |
| `crates/store` | `hermes-tern-store` | Private SQLite journal, actor credentials, routes, admission and task leases | core |
| `crates/supervisor` | `hermes-tern-supervisor` | Bounded Unix IPC and operator/peer authorization | core, store |
| `crates/omp` | `hermes-tern-omp` | Bounded operator proposals and one-way dispatch fence; never agent execution or an OMP receipt | core |

Library crates do not depend on executable crates. Gateway and storage do not depend on each other or on UI. Core has no Tern, SQLite, WebSocket, clap or OS-runtime dependency. The CLI library composes subsystems; it does not re-export a monolithic API. New OMP/application modules belong to their own real responsibility boundary when implemented, not empty placeholder packages.

Verification orchestration stays in `scripts/verify_runtime.py`, outside runtime crates. `scripts/smoke_native_chat.py` drives real Tern and reuses the bounded synthetic gateway in `tests/test_native_chat.py`; it does not become a Hermes inference adapter. Behavioral runner contracts live in `tests/test_runtime_autorun.py`. Native and provider/window gates remain distinct from package/layout validation.

## Migration and verification plan

1. Record the existing source inventory, lockfile and collected test identities. Baseline: 51 Rust tests passed, 2 explicit live-gateway tests ignored. The existing full gate fails rustfmt only on the just-added process test; format that owned file as part of its move.
2. Add executable Cargo-metadata tests for binary/library placement and the dependency DAG; observe RED on the old monolith.
3. Move each module and its behavior tests to its owning package. Move the Python bootstrap into the gateway package so `include_str!` remains package-local. Move CLI/process tests to the production binary package and the native fixture to `bin/testing`.
4. Split only the shared strict-JSON/text helpers out of the Hermes wire module. Update explicit cross-crate imports and dependency declarations. Preserve private gateway grant fields, request semantics, CLI names and test assertions.
5. Refresh Cargo.lock offline without dependency upgrades; compare every external package's version, source, checksum and dependency list to the pre-move lock.
6. Run the original test-identity comparison, every package independently with `--locked`, full workspace build/tests/clippy and `./verify.sh`. Build and execute both binary crates; real native rendering stays a labelled fixture and is not live-agent evidence.
7. Update current status, roadmap, tasks, source witnesses, contributor rules and path references together. No commit, personal install, gateway restart or publishing is part of this change.

This refactor changes code organization, not the outstanding release gates: live provider-backed controls, native window-plugin execution, peer tool integration and OMP interoperation remain separately qualified.

## Historical split checkpoint result

These measurements describe T-15 at handoff, not subsequent peer-tool changes. [Current status](STATUS.md) owns current test totals and gates.

- All 53 pre-split Rust test identities remain collected: 51 passed, 0 failed, 2 live-gateway opt-ins ignored. The full local gate passed with 19 Python tests, including binary/library placement and exact subsystem dependency checks.
- Each of the 8 packages passed `cargo test --locked -p <package> --all-targets` and `cargo clippy --locked -p <package> --all-targets -- -D warnings` independently. The 131 external lockfile records are unchanged, including dependency edges and checksums.
- `cargo build --workspace --bins --locked` passed. Production version/plain preview and locked offline scratch installation from `bin/hermes-tern` passed; the native preview rendered in light/dark via an isolated Tern scenario.
- `hermes-tern-testing` refuses non-native execution with exit 1. At the split checkpoint isolated native launch failed with TTY I/O error for both current and retained pre-split binaries; exec/PTY probes also timed out. The later native-smoke checkpoint rendered that unchanged fixture at wide/narrow sizes in light/dark, with actual pixel inspection. Native-chat controls/lifecycle were subsequently repaired and tested through production PTY/SDK and actual Tern synthetic-gateway input/render/detach paths. The old failure remains historical; complete T-07 and live/window acceptance stay open. See [current status](STATUS.md).
- No commit, personal install, Hermes session mutation, inference, gateway restart or remote CI run occurred in this split tranche.
- A read-only split-regression review found no concrete regression within scope. The parent matched all 60 reviewed source hashes to current files and independently confirmed `cargo package --frozen -p hermes-tern-gateway --allow-dirty --list` includes the helper. The review's test subset is not added to full-suite totals, and its verdict does not qualify live/native runtime behavior.
