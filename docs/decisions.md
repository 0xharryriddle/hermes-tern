# Architecture decisions

Status labels below distinguish bootstrap choices from proposed runtime decisions. The latter require owner review; no ADR silently authorizes implementation.

## ADR-001 — Rust application, thin Luau package

Status: accepted for bootstrap by the explicit initialization request.

Use the Rust `tern-sdk` in the executable and Luau only for Tern registration/layout integration. The SDK is a Surface Protocol client, while plugins are Luau packages. Rejected: fictional Rust-loaded plugin ABI, a full custom terminal renderer, or moving the entire application into window Luau.

Consequence: distribution has a binary and a small plugin directory; native-render and plugin-window acceptance are separate. See [Tern integration](tern-integration.md).

## ADR-002 — Hermes remains the agent owner

Status: proposed for the connected runtime.

Use a capability-qualified Hermes TUI gateway attachment for broad feature coverage. The product path should attach to the existing owner over WS using a supported discovery/auth flow; standalone stdio is isolated or dedicated-home only. ACP remains optional reduced capability; HTTP is a later transport option. Never fall back from authentication failure to another interface automatically.

Rejected: direct `state.db` mutation, a second writer for an owned profile, screen scraping as lifecycle, a Rust rewrite of Hermes inference/tools. Main risk: private bootstrap coupling; the first implementation gate must settle the external attachment contract. See [Hermes integration](hermes-integration.md).

## ADR-003 — Independent actors differ from delegated children

Status: proposed.

A peer actor is an explicitly registered execution context with capabilities and policy. Hermes children stay under their parent lifecycle unless deliberately promoted through a qualified adapter. Do not equate one pane with one independent actor or promise child survival after process exit.

Consequence: identity, task lineage and display bindings are distinct. See [actors](actors.md).

## ADR-004 — Durable coordination, no exactly-once inference claim

Status: proposed.

Introduce a separate local supervisor and SQLite journal only when multi-actor coordination is built. Use transactional admission, idempotent message keys, explicit dispatch attempts and ambiguity reconciliation. The database does not atomically control external runtime side effects.

Rejected: writing mailbox state into Tern KV (concurrent last-writer behavior is unsuitable), sharing Hermes persistence, unbounded in-memory message queues, and blind resend after disconnect. See [protocol](protocol.md).

## ADR-005 — OMP native lane before speculative RPC parity

Status: proposed.

Qualify Tern AgentCx as a bounded native prompt-handoff lane. Investigate direct OMP RPC only for requirements not met by that lane. Expose reduced capabilities rather than invent a universal adapter.

Consequence: initial native handoff requires explicit consent and remains window-dependent; autonomous durable peer messaging needs stronger runtime provenance/ack semantics. See [actors](actors.md) and [research](research.md).

## ADR-006 — Local first, honest isolation and bounded scale

Status: proposed.

Target Linux local desktop first. Worktrees/profile homes prevent accidental overlap but are not security sandboxes. Reserve hardened execution and remote-host control for measured backend-specific gates. Use bounded queues/storage and record provider budgets.

Rejected: unlimited actors, hidden automatic approval, automatic merge/publish, and claiming terminal restart restores arbitrary agent processes. See [security](security.md) and [configuration](configuration.md).

## ADR-007 — Small executable bootstrap, comprehensive but non-duplicated docs

Status: accepted for bootstrap; single-package layout superseded by ADR-008.

Ship one real package with static preview, tests and verification rather than empty future crates. Keep all detailed docs under `docs/`, with one task ledger and one status checkpoint. Pin the SDK by exact Git revision and the tested toolchain; keep CI and local gates identical.

Future detailed implementation plans are written after architecture review, one complete vertical slice at a time. A roadmap is not a claim of implementation readiness for every future file.

## ADR-008 — Executable crates in bin, subsystem libraries in crates

Status: explicitly requested by the owner; implemented and locally verified.

Put the production entry point in `bin/hermes-tern` and the native fixture in `bin/testing`. Move argument parsing to `crates/cli`; keep existing core, gateway, UI, store and supervisor logic in their respective library crates. Do not retain a monolithic compatibility crate. Library dependencies point toward shared validation and subsystem contracts, never toward binary packages.

Consequence: tests move with their owning libraries, executable/process tests remain with the production binary, and the Python discovery helper is embedded from within the gateway package. Verify each package independently and compare external lock records and collected test identities during migration. [Workspace layout](workspace-layout.md) records the concrete graph and measured result; live-runtime and native-window gates do not become complete merely because the split compiles.
