# Actors, parallel work and OMP

Status: local actor/mailbox and explicit physical-session tools implemented; live parallel scheduling and OMP remain proposed/unqualified. No actor is launched or registered by the static preview. See [architecture](architecture.md), [protocol](protocol.md), [peer tools](peer-tools.md) and [security](security.md).

## Identity is not a pane

An actor is an independently addressable agent execution context. The supervisor assigns a stable opaque actor ID within a workspace and a fresh incarnation ID for each attachment. It records runtime kind, session identity, capability report, effective provider/model, working directory, profile ownership and parent lineage where applicable.

A pane binding records the Tern host/window/session context and the pane ID exactly as received. It can be replaced without changing the actor. A pane title, process name or observed prompt does not authenticate an actor. Multiple panes displaying one runtime session are multiple views, not multiple actors.

| Kind | Ownership | Peer participation |
| --- | --- | --- |
| Managed Hermes actor | Runtime attached through qualified Hermes adapter | Explicit mailbox and task capabilities |
| Hermes delegated child | Parent Hermes runtime | Observed lineage; no independent peer authority by default |
| Opted-in native OMP pane | Tern AgentCx binding | Limited prompt handoff/readback, qualified separately |
| Direct OMP RPC actor | Future qualified OMP adapter | Only proven capabilities |
| Unmanaged terminal/agent pane | User/Tern | Observe presence only; no injection or assumed transcript access |

## Lifecycle

Proposed actor states: registered, connecting, ready, running, waiting_input, cancelling, disconnected, stopped and failed. A runtime state that cannot be reconciled is unknown, not ready. Keep lifecycle separate from mailbox delivery and task completion.

- `ready` requires a live qualified connection, target binding and permission to accept work.
- `running` is based on runtime events, not a spinner or process existence alone.
- `waiting_input` blocks automated prompt forwarding that could answer a human-only request.
- `cancelling` means a request was sent; only observed termination/cancel completion reaches a terminal state.
- Renderer closure does not implicitly stop an independently managed actor.
- An exited process cannot be revived by replaying a UI snapshot.

## Routing and communication

Messages are addressed to an actor ID, scoped to a workspace, tagged with purpose and provenance, and limited by recipient capabilities. Default routing is deny until the user enables a route or accepts a proposed handoff. Direct messages are the first feature; broadcast and open-ended topic subscriptions are deferred.

A routed message contains peer context, not a new human command. The recipient adapter must preserve that distinction. If a runtime exposes only ordinary user-prompt submission, mark the lane as a human-confirmed prompt handoff; do not advertise autonomous peer messaging. A transport wrapper that prepends an actor name to user text does not create a trust boundary.

For autonomous Hermes peer delivery, the M3 design needs a runtime-native peer-message/tool hook or an optional Hermes plugin that preserves origin and authority. It must be callable by the agent through a narrow messaging tool; a visual mailbox alone does not let the model communicate. The plugin must use supported extension APIs, authenticate to the local supervisor, and expose only explicitly permitted send/read/result actions. No capability grants, approvals or raw shell APIs travel through peer content.

### Example collaboration

1. The user creates two actors with non-overlapping worktrees and a shared task goal.
2. Actor A proposes a bounded task for B; the routing policy admits it and records the message.
3. B receives attributed context at a safe boundary and acknowledges the understood task through its adapter/tool, not through guessed UI text.
4. B returns a result with changed-file references, commands executed and limitations.
5. A reviews the result as untrusted evidence. The user authorizes merge or publication separately.

## Scheduling and write scopes

Start with a finite actor pool and a FIFO queue per actor, with fairness across active senders. Limit ready/running actors, accepted bytes, outstanding requests and provider spend separately. Budget exhaustion refuses new work and preserves accepted state; it is not a reason to drop pending approvals.

A task lease is a supervisor scheduling fence, not filesystem enforcement. Claim/renew/complete use compare-and-swap on the current generation. A stale completion cannot overwrite a newer result. Lease expiry does not prove the old process stopped: do not reassign a writing task until the prior actor is confirmed stopped or its write boundary has been revoked.

Editing actors use different worktrees by default; concurrent shared-tree work requires explicit user permission and path ownership. Worktrees reduce conflicts but cannot stop a process writing elsewhere. Hardened filesystem execution is a separate release capability, with positive and negative tests.

## Native OMP lane

The exact pinned Luau service type is `AgentsCx`, exposed as `cx.agents` (earlier documents use the shorthand AgentCx). It supports list/start/ask/wait/transcript/interrupt/stop. The inspected type definitions describe native OMP prompt submission through TSP send and transcript extraction from `omp.session`. The online guide differs on the `ask` mechanism; verify the installed runtime rather than copying either description blindly.

This lane is useful for explicit user-approved handoffs to an existing OMP pane. Its limitations must remain visible:

- It is window-scoped. A separate [local handoff exchange](omp-handoff.md) and Luau bridge now exist; they are not wired to automatic coordinator delivery and actual-window acceptance remains open.
- `ask` submission and `wait` idle are not application-level durable consumption acknowledgements.
- `transcript` exposes OMP chat projection, not a generic Hermes transcript or replayable execution journal.
- `interrupt` sends Ctrl-C; `stop` closes a program. Both need explicit target and policy checks.
- Native APIs do not establish pre-execution tool approval interception or filesystem sandboxing.

A timeout after submission is ambiguous. Do not retry the same prompt automatically. Reconcile visible/runtime state or ask the user. On window loss, this lane disconnects rather than pretending the headless supervisor can still call AgentCx.

The local connector uses fixed-argv `tern.process.run` with bounded JSON stdin/stdout, then human pane selection and confirmation. The Rust endpoint is `omp-handoff exchange`; its durable arm never grants again after a lost reply. Synthetic callback execution is verified separately from installed-window behavior. The direct callback is `(result, cx)`, whereas an awaitable's `next` callback is `(result, err, cx)`. `ProcessOptions.env` augments inherited variables, so it cannot establish a scrubbed environment. Results arrive after process completion, not as a duplex stream. Every continuation uses fresh `WindowCx` and consumes its callback before any ask. Pinned anchors: `plugins/tern.d.luau:41–73,599–648,1855–1867`; the [shared API guide](https://docs.stencil.so/tern/reference/api-shared.html) qualifies callback loss on reload.

The inline snapshot may be retained; requiring a dock composer and empty layer rejects main-only retained views but does not prove conditional liveness. `ask` has no expected surface/session incarnation parameter and can queue. The consent UI explicitly authorizes that weaker pane-level operation, not a guarantee of unchanged composer identity at delivery. No OMP transcript is exported by v1; idle observations remain outcome-unknown.

`ask` returns no delivery ID, transcript rows expose no stable message ID, and `interrupt` returns no cancellation receipt. Queued prompts may survive plugin VM reset while callbacks are lost. These limits forbid blind retry or a durable-consumption claim. No automatic start/stop or peer approval belongs in this lane.

## Direct OMP RPC lane

The installed OMP help advertises `--mode=rpc` and `--mode=rpc-ui`. The official v18.12.6 source is now pinned to `45c35e40c46166dc726ccc52f5d27f40f2449f99`; the [RPC reference](https://github.com/can1357/oh-my-pi/blob/45c35e40c46166dc726ccc52f5d27f40f2449f99/docs/rpc.md) defines a custom JSONL protocol, not JSON-RPC 2.0 or assumed Pi compatibility. Source verification is not installed runtime acceptance.

- Startup emits `ready` with supported protocol versions and frame bounds. Negotiate v2; validate ordered base64 `rpc_chunk` sequences, strict UTF-8, declared byte lengths and bounded reassembly. Inbound commands remain unchunked.
- `prompt` acknowledgement is admission, not completion. A response with `data.agentInvoked:false` is local completion; otherwise correlate `prompt_result` by request ID. `sessionSettled:false` means background work remains; wait for `session_settled` or `get_state.isSettled` before recycling an owned worker.
- `abort` is session-wide and does not promise reversal of tool effects or termination of every descendant. Continue draining stdout after closing stdin or shutdown may stall.
- The documented default approval mode is yolo. Any managed launch must explicitly select an approved restrictive policy such as `--approval-mode always-ask`; reads remain auto-approved under that mode. Do not pass yolo/auto-approve flags. Tool approvals arrive through extension UI selects; answer only an explicit human-selected emitted choice, or deny/cancel.
- Native AgentCx has no equivalent permission-response surface. It remains a bounded human-confirmed handoff lane, not a substitute for direct RPC supervision.
- The official Rust `omp-rpc` crate exists but is marked `publish=false`; its inspected transport uses unbounded outgoing/event channels and accumulates prompt events. Do not adopt it unchanged under this project's bounded-resource contract.
- A peer payload can contain a slash command recognized by OMP. Never forward peer text blindly as a privileged user prompt; preserve provenance through an approved tool-result channel or require human confirmation.

T-12 must still capture real startup, prompt, completion, cancellation, approval and shutdown behavior in an isolated authorized OMP environment. No provider-backed OMP run was performed by this research.

The installed official18.12.6 binary now passed a bounded no-inference ready → `negotiate_protocol` v2 → correlated `get_state` → stdin EOF/exit0 probe. It used a fresh private HOME with cwd beneath it, a replacement environment, explicit catalog model `openai/gpt-4o-mini`, `--approval-mode always-ask`, no tools/ambient extensions/skills/rules/session, disabled background features and an independently tested network namespace. State reported zero messages/tools, settled/no pending work; no prompt was submitted. Unsolicited `available_commands_update` arrives before the response (`rpc-mode.ts:1753–1759`) and must not be mistaken for that response. No raw state/system prompt/transcript was persisted. Earlier timeout and overly strict event-parser attempts remain failures, not first-pass acceptance.

This is only the installed transport prerequisite. It does not establish chunk reassembly, permission flow, inference, cancellation, native AgentCx or a production adapter. Official asset size/digest equality binds the binary to the release, not to a reproducible source build. Noninteractive startup requires a model (`main.ts:2617–2647`), and discovery/prewarm may contact providers without inference. Same-child network controls rejected both a parent-reachable loopback listener and an external address with ENETUNREACH; configuration isolation is not filesystem enforcement. The probe's permissive filesystem remains explicitly outside its safety claim.

Choose this lane over native AgentCx only where it provides verified requirements the native lane cannot. Do not replace a running user OMP session or migrate its state automatically. An MCP integration can expose tools, but does not by itself solve actor lifecycle, durable delivery or approvals.

## Scaling envelope

The first coordinated acceptance scenario uses two Hermes peers and one opted-in OMP pane. A later load harness varies actor count and event rate with recorded CPU/RSS/disk/latency, injects slow consumers, and keeps accepted-message loss at zero. Do not advertise a maximum fleet size from the defaults or a synthetic event test. See [configuration](configuration.md) and [verification](verification.md).
