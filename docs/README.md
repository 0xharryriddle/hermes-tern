# Documentation

Hermes for Tern: a Rust application using the Tern Surface Protocol, packaged with a small Luau integration. This is the canonical documentation directory.

## Read in this order

1. [Current status](STATUS.md): what actually works, evidence and blockers.
2. [Product and requirements](product.md): users, workflows, scope and acceptance requirements.
3. [Architecture](architecture.md): proposed ownership boundaries and integration choices.
4. [Roadmap](ROADMAP.md): dependency order, release gates and the next implementation slice.
5. [Development](development.md): build and verify the current workspace.

## Integration and design

| Document | Authority |
| --- | --- |
| [Tern integration](tern-integration.md) | Rust SDK, Luau package, TSP lifecycle and native AgentCx boundaries |
| [Hermes integration](hermes-integration.md) | Gateway attach, isolated stdio, capability parity and profile ownership |
| [Actors and OMP](actors.md) | Actor identity, routing, scheduling, OMP compatibility and delegated children |
| [OMP handoff](omp-handoff.md) | Explicit local proposals, one-way dispatch fence and human-confirmed window bridge; live acceptance separate |
| [Protocol and persistence](protocol.md) | Proposed envelopes, delivery states, durable transactions and recovery |
| [Security](security.md) | Trust model, approvals, secrets, same-user limitations and release threats |
| [Native user experience](ux.md) | UI information hierarchy, interaction rules, accessibility and evidence labels |
| [History viewer](history-viewer.md) | Bounded immutable history projection inspection; local paging, no server cursor or live authority |
| [Configuration](configuration.md) | Existing options, proposed configuration, isolation and bounded resource defaults |
| [Operations](operations.md) | Preview installation, troubleshooting, recovery and release policy |
| [Verification](verification.md) | Local gates, native checks, acceptance scenarios and evidence taxonomy |
| [Research](research.md) | Source-verified facts, version skew, unknowns and review dispositions |
| [Architecture decisions](decisions.md) | Proposed and bootstrap decisions with rejected alternatives |
| [Implementation plan](implementation-plan.md) | First connected vertical slice, file ownership, tests and execution review |
| [Workspace layout](workspace-layout.md) | Binary/library package ownership, dependency direction and migration checks |
| [Coordination implementation](coordination-implementation.md) | Durable mailbox, actor capability and supervisor tranche |
| [Operator-key policy](operator-key-policy.md) | Key-first startup, missing-key refusal and bounded bootstrap/retry contract |
| [Hermes peer tools](peer-tools.md) | Implemented physical-session plugin binding, operator provisioning, tool contracts and evidence limits |
| [Gateway qualification](gateway-qualification.md) | Executed discover-only WebSocket handshake, ping and capability evidence |

## Machine-readable authorities

- [tasks.json](tasks.json): requirement inventory, task identifiers, dependencies, acceptance, evidence, blockers and status. Planned tasks are not permission to execute.
- [sources.json](sources.json): exact upstream revisions, inspected surfaces and qualification of each source.
- [verification.json](verification.json): current verification checkpoint with separately labelled historical runtime observations, not a runtime certificate.

## Status vocabulary and precedence

`Implemented` means code exists. `Verified` states the exact executed check. `Proposed` means an unimplemented design subject to owner review. `Blocked` names a missing prerequisite. `Not established` means no adequate evidence was obtained. `Deferred` is consciously outside the first release.

The user request and [contributor rules](../AGENTS.md) govern work. Requirements govern design; executable behavior and measurements govern current status. A design cannot overrule a failing runtime test. A green link checker cannot establish an external API claim.

Design sections describe targets unless marked implemented or runtime-tested. The owner authorized full native implementation. Current CLI and verified results are listed in [configuration](configuration.md) and [status](STATUS.md); proposed fleet sockets/schemas are not yet exported.
