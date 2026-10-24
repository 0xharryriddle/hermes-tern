# Architecture

Status: owner-authorized implementation in progress. Current code uses binary crates under `bin/` and library crates under `crates/`, with a static SDK preview, discover-only WebSocket adapter, native/plain session UI, local actor store and Unix-IPC supervisor. Six standalone Hermes tools now call a bounded Rust bridge with operator-attested physical-session binding. The small Luau launcher and verification tooling remain separate. Provider-backed peer calls, canonical continuity, parallel live actors and OMP acceptance are still open; the broader topology below remains a target, not a completion claim. [Workspace layout](workspace-layout.md) defines the implemented package graph; [peer tools](peer-tools.md) defines the new adapter boundary.

## Options considered

| Approach | Strength | Cost or ceiling | Disposition |
| --- | --- | --- | --- |
| Rust TSP client + thin Luau + Hermes gateway attach | Native UI with the broad Hermes control surface; preserves runtime ownership | Gateway discovery/auth/version compatibility must be verified | Recommended |
| Rust client using ACP as its primary adapter | Standardized external-agent lifecycle; useful for an existing ACP host | Does not imply full Hermes command, profile or delegation parity | Optional reduced-capability adapter |
| Wrap the interactive Hermes terminal and parse its output | Fast visual coexistence | Brittle state inference, weak approval correlation and no dependable peer protocol | Not a foundation for coordination |

A pure-Luau implementation is also possible for small panels but does not satisfy the requested Rust application boundary; long-lived orchestration does not belong on Tern's window thread.

## Three control planes

```text
Developer
  |
  +-- Tern window / Luau command                 presentation and launch
  |       |
  |       +-- hermes-tern Rust pane client       TSP projection and user input
  |                    |
  |             workspace supervisor            proposed coordination authority
  |                    |
  |            +-------+--------------------+
  |            |                            |
  |      Hermes adapter                 OMP adapter
  |            |                            |
  |     existing gateway owner       native AgentCx / qualified RPC
  |            |
  |     Hermes sessions and children           agent execution authority
  |
  +-- explicit human approval                   never delegated to peer text
```

TSP is a rendering and input protocol, not an actor bus. Tern pane identity is a presentation binding, not an authentication credential. Hermes remains the authority for conversations, tool execution, providers, memory, plugins and model usage. The supervisor owns only cross-runtime coordination: actor bindings, delivery records, routing policy and task ownership.

## Ownership rules

| State | Single owner | Other components may do |
| --- | --- | --- |
| Hermes transcript and session IDs | Hermes runtime | Request history/resume through the adapter |
| Provider credentials and auth pools | Hermes runtime/profile | Display sanitized capability/health only |
| Actor identity and connection incarnation | Workspace supervisor | Bind a view to the assigned identity |
| Inter-actor mailbox and task lease | Workspace supervisor | Submit authenticated intent, read status |
| TSP node IDs and current surface | Rust pane client | Rebuild projection from supervisor/runtime snapshots |
| Layout, focus and compositor | Tern | Request explicit layout actions through Luau |
| Human approval decision | Human, enforced by runtime | Relay an exact pending request and response |

Never open Hermes' database directly to attach, copy live SQLite/WAL files to clone a profile, or create a second writer beside a gateway owner. Source inspection shows current Hermes product surfaces attach to a gateway over WebSocket; standalone stdio claims exclusive launch-home ownership. See [Hermes integration](hermes-integration.md).

## Process topology

For the first live-session slice, one Rust client attaches to one approved Hermes runtime and session. A separate supervisor is introduced only with persistent multi-actor coordination. It must not be embedded in a window plugin or tied to a renderer's lifetime.

The local supervisor now exposes an explicitly started foreground process, a private Unix-domain socket and one locked SQLite writer. Store/IPC/process fixtures cover admission, authorization and crash recovery. The full target still requires qualified agent bindings and replay consumers; these are not proven by synthetic registrations. Startup does not kill an unknown lock owner. An explicitly started supervisor may survive view closure; persistence beyond session/process exit is never assumed for Hermes child threads.

Independent actors may share one Hermes gateway only where that gateway's qualified API supports safe multi-session ownership. Separately launched gateways require separately provisioned homes. Peer actors must not share writable memory/profile files accidentally. Profile isolation is not a sandbox.

### Process ownership and distinct shutdown operations

| Object | Control authority | Closing or disconnecting a view |
| --- | --- | --- |
| Bootstrap preview process | Tern launches it after an explicit palette action | The static process normally exits after printing; retained output is not a live actor |
| Existing Hermes gateway | Its existing runtime owner, not hermes-tern | Detach the client only; never signal or restart the gateway to recover a view |
| Explicitly provisioned backend spawned by the future supervisor | Supervisor, within the user's launch/termination policy | Keep or stop according to that explicit policy, never inferred from pane closure |
| External OMP or terminal pane | User/Tern; opt-in prompt permission is not process ownership | No automatic process termination; destructive AgentCx stop requires separate explicit authority |
| Workspace supervisor | Explicit local lifecycle owner | May outlive presenters only when deliberately started in that mode |

`cancel-turn` requests cooperative cancellation of the selected runtime turn. `close-view` removes presentation. `detach` drops a client connection without stopping the runtime. `terminate-backend` ends a process and is a separate privileged operation. None implies another.

For a process the supervisor actually spawned, retain an OS process handle or a PID plus verified start identity, launch generation, and owned process-group identity. Registration alone does not authorize termination. Refuse signalling after an identity mismatch; a reused PID or stale actor incarnation must never target another process. Shutdown reports requested, observed-stopped, or unknown separately. A supervisor restart must re-establish ownership before signalling any surviving backend.

## Adapter boundary

The Rust application normalizes events into a small internal vocabulary while retaining upstream session/request identifiers. A capability report is produced at attachment, not guessed from a version string. Unknown events may be preserved as bounded diagnostic metadata; unknown authority-bearing requests are rejected.

An adapter must describe: attachment method, ownership mode, capabilities, active session identity, cancellation semantics, replay support, payload limits and health. Missing mandatory approval or cancellation behavior blocks a controlled session rather than falling back to shell typing.

The owner-directed split gives each existing subsystem its own library package: core validation, gateway transport/session state, UI presentation, durable store, supervisor IPC and CLI composition. The new `crates/omp` owns immutable local handoff proposals and dispatch fencing, not agent execution. The production binary is only an entry point; `bin/testing` exercises the native fixture. No compatibility monolith or empty future adapter package is retained.

The [OMP handoff](omp-handoff.md) is deliberately outside automatic mailbox delivery: an operator prepares a proposal, the window displays its exact untrusted text and asks for pane-level consent, then Rust persists an irreversible local arm before one `AgentsCx.ask`. Only one local grant is issued; a queued ask and idle observation do not prove consumption. Window loss cannot be repaired by regranting. The API lacks conditional composer/session binding, so this lane cannot satisfy a strict durable-actor dispatch contract.

## Data flow and threading

1. Human input becomes a typed local command with explicit actor/session target.
2. Policy checks the operation and records the intent before an irreversible outbound operation.
3. The adapter sends the runtime's actual request and correlates responses/events.
4. Hermes executes inference/tools; the supervisor never duplicates that loop.
5. The presenter derives the native view from accepted state changes.

The Rust SDK is synchronous. Keep one thread responsible for terminal ownership and `Session::next`/`pump`; use bounded worker channels and `Session::waker()` for network events. `Ok(None)` can mean a wake/timeout, not end-of-input. Coalesce display deltas, not approval decisions or accepted mailbox items. Never wait for a model response on the terminal event loop.

## Failure and recovery

Surface detach affects drawing credits, not agent execution. A renderer crash does not grant permission to restart a model turn. Gateway disconnect produces a disconnected/unknown state and suspends destructive controls until reconciliation. Durable queued messages can be reoffered only where delivery deduplication is supported; ambiguous runtime submission requires reconciliation or human disposition.

On supervisor restart, reconstruct its own journal and attach to runtime state. Mark process-local Hermes child handles unknown/interrupted when the runtime cannot reconnect; do not automatically replace them. A Tern screen restored in scrollback is read-only presentation history, not evidence of a resumed actor.

## Implementation boundary

[Protocol](protocol.md) defines the proposed durable contract. [Actors](actors.md) owns scheduling and interoperability. [Security](security.md) defines authority limits. The owner authorized native execution of the [implementation plan](implementation-plan.md); personal runtime changes and publication retain separate authorization boundaries.
