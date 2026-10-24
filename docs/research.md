# Research and evidence

Research date: 2026-10-23. Status: source-grounded kickoff, not runtime compatibility certification. Machine-readable provenance is in [sources.json](sources.json).

## Method

Inspect the empty target repository, clone the named public SDK at an exact revision, read its Rust API/examples and Luau declarations, consult official Tern/Hermes documentation, inspect the installed Hermes source baseline, and run the actual bootstrap against the installed Tern renderer. No private credentials or user transcripts were collected.

Source precedence is scoped: installed runtime behavior wins for compatibility; pinned source defines what was inspected; official docs explain intended interfaces but may lag. Never silently replace a pinned observation with a moving checkout's contents.

## Findings

| ID | Status | Finding | Impact |
| --- | --- | --- | --- |
| F-01 | Source verified | Rust `tern-sdk` is a synchronous Surface Protocol library, package 0.1.0, Rust 2024 | Native renderer belongs in Rust; no need for a second cell-based TUI |
| F-02 | Source verified | Installable Tern plugins use Luau host/window entries | Ship a thin Luau integration, not a fictitious Rust plugin ABI |
| F-03 | Source verified | The SDK has custom Terminal transport and a cross-thread waker | Test event loops without a live GUI; do not treat wake/timeout as EOF |
| F-04 | Source verified | Drawing ACK credits depend on windows; retained surfaces are not process persistence | Bound buffers; separate UI recovery from actor recovery |
| F-05 | Source verified | Tern AgentCx exists in window Luau, including native OMP operations | Reuse a qualified native OMP lane instead of claiming no Tern agent APIs |
| F-06 | Version discrepancy | Online AgentCx ask prose says paste/Enter; pinned types say atomic native TSP send | Test the installed implementation before choosing semantics |
| F-07 | Source verified | AgentCx transcript reads OMP TSP chat; non-OMP returns no structured transcript | Hermes needs its own runtime adapter; native transcript is not universal |
| F-08 | Source verified | Hermes offers ACP, TUI gateway and HTTP control interfaces | Compare by capability, not convenience of parsing terminal output |
| F-09 | Source verified | Current product surfaces attach over WS; standalone stdio owns a home exclusively | Primary product attach design is WS; stdio is isolated testing/dedicated ownership |
| F-10 | Source verified | Private gateway bootstrap handles discovery/ensure and connection grants | External attach contract must be qualified; never copy private tickets or auto-restart owners |
| F-11 | Source verified | Hermes public subagent lifecycle is parent-turn-bound and process-local | Delegated children cannot be sold as persistent independent peers |
| F-12 | CLI verified only | Installed OMP advertises rpc and rpc-ui modes | Existence of flags is not evidence of schema or approval parity |
| F-13 | Runtime verified | Rust preview renders real native Tern light/dark surfaces and plain fallback | Bootstrap is executable; it still launches no model or peer |

## Source pins and local observations

The SDK is pinned at `9e9c07463f4958b0c633424c9860aba499487d28` and built through Cargo, not copied into this repository. Hermes source was initially inspected at `85d57d956454914a97bcf24b4c8c6e463267ef1e`. During the read-only review the installed checkout advanced to `bf0203a794a07aac3ff764972ab9087baa4e4f4e`; the ownership/WS conclusions were rechecked using `git show` at the initial revision. No Hermes update or core edit was performed for this project.

Initial executable observations: Tern 0.9.5 (2863c37), OMP 18.12.6, Hermes version command reporting v0.22.4+71.g85d57d9. These are checkpoint observations, not minimum supported versions or present-time claims after the checkout changed.

Rust 1.98.0 is the project's verified pin. The earlier version-named 1.98.1 setup failed despite a working stable alias; the fallback was a separately installed complete toolchain, not omission of clippy.

## Review disposition

Two independent same-model role passes reviewed the bootstrap proposal: architecture/scope and adversarial verification. They returned conditional GO for docs/preview scope, not approval of the completed documents or product readiness. Shared-model bias remains high. This was a bounded first-pass review, not a heterogeneous debate or a consensus seal.

The parent independently confirmed the consequential observation: standalone Hermes stdio is not an attach path to an owned home. This changed the recommendation to qualified WS attach. Other adopted controls: separate plugin parse vs window execution evidence; isolated sockets/processes as well as config roots; explicit lifecycle/delivery ambiguity; no inherited default model drift; no fake live status; and local CI results distinct from remote execution. Later AgentCx research corrected the initial overly broad absence inference. The docs retain the narrower, correct claim: the Rust Surface SDK does not itself provide the proposed durable actor bus.

### Late-delivered review reconciliation

The complete review batch arrived after the bootstrap handoff. It concerns the original proposal, not a fresh review of the delivered code/documents. The AgentCx steering messages did not reach either reviewer before completion, so their output cannot independently validate that later discovery.

| Review item | Current disposition |
| --- | --- |
| Architecture 1: stdio/profile ownership | Corrected before handoff: qualified WS attach is primary; exclusive-home stdio is a separate lane |
| Architecture 2: lifecycle/termination authority | Made explicit after full review delivery: owned-process table, distinct cancel/close/detach/terminate operations, PID/start identity and generation |
| Architecture 3: protocol authority/envelope | Already specified: event envelope, peer-origin boundary and fail-closed capability handling; runtime evidence remains open |
| Architecture 4: early failure gates | Strengthened T-06/M1 acceptance to include malformed/oversized/partial frames, EOF, timeout, bounded shutdown and stale process identity |
| Architecture 5: bootstrap evidence | Actual SDK build, CLI and native render executed; window-launch and pixel evidence remain not established |
| Verification 1: isolation | Scratch HOME/XDG/socket isolation and no agent launch were exercised; native smoke still inherits non-overridden environment values, so no claim of a scrubbed credential environment |
| Verification 2: lifecycle evidence | Interactive cases assigned to T-06/T-07; the failed window text assertion is retained as an executed failure with acceptance not established, not a skip |
| Verification 3: side-effect tests | Existing CLI behavior tests cover the documented subset; child-launch traps, bounded subprocess tests and native transport-error coverage remain explicit M1 prerequisites, not claimed passing |
| Verification 4: structural controls | Negative controls and source-pin/lockfile comparison executed; evidence-reference semantics remain a manual review obligation, not an automated guarantee |
| Verification 5: durable mail | Proposed invariants and crash tests owned by T-09; no exactly-once external-effect claim |
| Verification 6: public wording | Static-preview/proposed labels and no private workstation paths in public docs; newer AgentCx discovery qualified separately |
| Verification 7: commands/provenance | Locked build/test/clippy, format check and local wrapper executed; remote CI still unrun |

The full review does not establish runtime readiness or owner approval. Its remaining runtime/test obligations stay on the roadmap; they are not silently promoted to bootstrap PASS.

## Open research gates

### Late peer-tool and OMP contract extraction

The delayed read-only contract reports were reconciled after the workspace split. The parent rechecked the pinned Hermes revision and clean tracked source, the actual registration/dispatch/compression code, pinned Tern declarations, official plugin/shared API documentation, and pinned OMP startup/discovery code. This is source research, not another implementation or runtime acceptance pass.

- Hermes tool `session_id` is genuine host metadata but can rotate with transcript compression. `profile_name` can fall back to `default`; neither value alone proves a durable canonical actor binding. T-08 stays open and must deny missing/unproven/rotated identity rather than auto-rebind.
- The Tern service is `AgentsCx`; direct process callbacks have two arguments, awaitable callbacks three. Process environment is inherited-plus-overrides, not cleared. Native ask/interrupt and transcript rows lack durable operation/message receipts. The proposed connector remains a human-confirmed window lane.
- OMP noninteractive startup requires a resolved model; a no-inference state probe needs explicit catalog selection in an isolated environment. Startup may still discover local providers. No credential-free, network-free or installed-runtime success is inferred from source inspection.

Detailed boundaries are recorded in [Hermes integration](hermes-integration.md#source-qualified-peer-tool-identity-boundary) and [actors](actors.md#native-omp-lane). T-08, T-11 and T-12 remain open; these research reports are not the separate workspace-regression review.

| ID | Unknown | Resolution owner |
| --- | --- | --- |
| U-01 | Stable external Hermes discovery/auth/WS attachment contract for the target release | T-05; inspect supported APIs and capture isolated live handshake |
| U-02 | Full interactive request/event schemas and reconnect semantics | T-06; source/real-fixture contract inventory |
| U-03 | Trusted physical-to-canonical session/profile binding; host tool keyword is source-qualified but can rotate | T-08; qualified Hermes plugin/public hook, no role laundering or guessed rebinding |
| U-04 | Installed AgentsCx ask/readiness and proposed fixed-argv Rust-to-window exchange | T-11; native interoperability spike, human-confirmed handoff only |
| U-05 | OMP installed-runtime behavior and source/build equivalence; pinned source wire/approval contract is now documented | T-12; explicit isolated protocol capture |
| U-06 | Resource envelope and provider-backed scaling behavior | T-13; bounded measured workload |
| U-07 | Project license and distribution targets | T-14; owner decision |

## Non-findings

A missing `tern agent` CLI subcommand does not mean Tern has no agent API. A `ready` package catalog does not prove its window Luau ran. A restored surface does not prove a process continued. A Tern shot containing a native surface does not prove Hermes integration. These distinctions are carried into the [roadmap](ROADMAP.md).
