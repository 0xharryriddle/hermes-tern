# Hermes integration

Status: experimental Rust connected implementation. The active source contract is Hermes `bf0203a794a07aac3ff764972ab9087baa4e4f4e`. Real discover/doctor/create/detach/resume/history passed without inference; provider-backed approval/stream/cancel acceptance remains open. A source revision does not prove the running gateway's build identity.

## Preserve Hermes as the runtime

The Rust application is a host and view, not another agent loop. Hermes continues to own model/provider resolution, tool execution, skills, memory, sessions, plugins, MCP and internal delegation. Do not copy provider credentials into Rust configuration or reproduce Hermes' tool approval logic with string filters.

Official [programmatic integration documentation](https://hermes-agent.nousresearch.com/docs/developer-guide/programmatic-integration) lists ACP, the TUI gateway and the HTTP API. Source inspection refines how the TUI gateway is attached in the installed generation.

## Attachment modes

| Mode | Intended use | Boundary |
| --- | --- | --- |
| Existing gateway, WebSocket control | Primary product path for a user's profile | Discover/authenticate through Hermes' supported bootstrap; do not create another owner |
| Isolated standalone stdio TUI gateway | Contract tests and explicitly dedicated homes | Exclusive launch-home owner; not an attach mechanism |
| ACP | Optional portable, reduced-capability client | Capability-qualified; not an automatic fallback after an auth failure |
| HTTP API | Future remote or service consumers | Separate auth, transport and deployment review |

At the baseline, `tui_gateway/entry.py` reserves the launch home and refuses if a gateway already owns it. Its own comment says product Ink/Desktop surfaces attach over WebSocket, while the standalone entry is used for development harnesses. `ui-tui/scripts/gateway_bootstrap.py` discovers/ensures a gateway and obtains a private connection grant before producing the WS target.

The private bootstrap is not an asserted stable third-party API. M1 must establish the supported external attach path with the deployed Hermes revision. If a small bridge is needed, keep it an optional, version-qualified Hermes plugin/helper using public contracts where possible; do not fork Hermes core or expose tickets in logs. Do not invoke private bootstrap scripts blindly: an ensure path can start or replace a runtime, while a discovery-only path must not.

## Wire shape that is source-verified

Standalone stdio reads one JSON object per line. Its ready notification is shaped as:

```json
{"jsonrpc":"2.0","method":"event","params":{"type":"gateway.ready","payload":{"change_events":true,"replay_epoch":"opaque-runtime-value"}}}
```

This is an illustrative subset, not a captured runtime message; the exact payload includes additional fields. Preserve the outer `event` envelope and inspect `params.type`. Do not mistake each event name for the JSON-RPC method. Keep stdout protocol-only and stderr separately bounded.

Do not fabricate request parameter schemas from method names. M1 captures the real registration/handler contracts and sanitized request/response fixtures for the baseline and target runtime. The Rust client implements the canonical envelope and bounded transport; static preview remains separate.

Canonical WS sends one JSON object per WebSocket message, not a concatenation of NDJSON lines. Its ticket is single-use and expires after 30 seconds; mint a fresh grant for reconnect and send no Origin header. Once server-request handlers can answer or decline requests, advertise `client.capabilities {server_requests:true}`. Do not claim a strictly read-only filesystem attachment: discovery can tighten an existing same-owner lock mode, and resume can refresh policy/hooks through the owner.

## Capability matrix

All rows below describe intended adapter coverage, not shipped behavior.

| Feature family | Upstream evidence | Required integration treatment |
| --- | --- | --- |
| Prompt and streaming | `prompt.submit`, message/tool events in official docs | Correlate session, turn and tool-call identifiers; preserve parallel same-name calls |
| Session lifecycle | canonical create/list/resume/history/info/interrupt; qualified mutation APIs | Distinguish saved transcript from active session; canonical `session.status` is not available |
| Approvals | request/response families | Actor/session/request-bound response; stale/expired response denied |
| Clarify, sudo, secrets | documented interactive request families | Separate ordinary text, sensitive input and expiry; never persist secret values |
| Provider/model change | command dispatch and model inventory | Resolve effective provider/model, report actual result, no implicit worker fallback |
| Skills, memory and plugins | Hermes runtime facilities | Reuse selected profile; provide honest availability, not a second store |
| MCP | runtime tool discovery/reload | Explicit reload; report errors and tool-availability changes |
| Delegation | delegation status, subagent controls, lifecycle API | Child lineage under its parent; no duplicate peer registration |
| Compression and rewind | session controls, explicit truncation confirmations | User-directed destructive intent, fresh durable IDs; never keep stale rewind flags |
| Background jobs and scheduled work | runtime-specific APIs | Inventory capability; do not promise survival or delivery unless supported |
| Images, files and browser/computer tools | runtime tool and attachment facilities | Preserve typed attachment references and tool permissions; native display is not execution authority |
| Voice and audio | runtime-dependent transport/features | Qualify separately; show unavailable until supported rather than silently dropping input |
| Terminal execution backends | runtime configuration | Preserve the selected backend; never substitute local execution for an unavailable isolated backend |

The public subagent lifecycle API requires an active parent turn, retains metadata/results in-process, and does not reconnect children after a process restart. It is not a general persistent actor scheduler. Its cooperative cancellation result is not proof the child has stopped. [Lifecycle source](https://hermes-agent.nousresearch.com/docs/developer-guide/subagent-lifecycle-api).

## Approvals and prompt identity

A response must bind the authenticated human connection, actor, runtime session, upstream request id and request generation. UI cancellation, timeout or disconnect must never become `allow`. A peer message cannot answer an approval or change policy. Display the runtime's action details and scope before a decision.

For request families the host cannot render safely, refuse the controlled operation with a named unsupported capability. Never submit a synthetic affirmative to keep the model running. Reconnect must reconcile pending requests and expiry before enabling response buttons.

## Profile and model isolation

Attaching to an existing profile requires explicit user selection and must not rewrite its configuration, copy its database or create another owner. Ordinary session activity may still persist state through the existing Hermes runtime. A dedicated actor runtime gets a separately provisioned home with narrowly authorized settings/auth references, never a copy of a live database. Avoid sharing writable skills/memory directories between independent actors unless that is an explicit owner choice.

Parallel actor launch specifications record the effective provider/model from the selected session. They must not inherit an unrelated machine default silently. A missing provider or unavailable configured model is an actionable failure, not permission to choose another paid route.

## Source-qualified peer-tool identity boundary

At the pinned Hermes revision, `PluginContext.register_tool` accepts a schema, handler and optional availability check with `override=False` by default (`hermes_cli/plugins.py:461–499`). Ordinary dispatch passes `session_id`, `task_id` and `user_task` separately from model arguments (`model_tools.py:823–839`); the executor supplies `agent.session_id` (`agent/tool_executor.py:1716–1736`). A handler must take the host keyword, not an identically named model argument, and return bounded JSON text (`tools/registry.py:928–963`). Availability checks are not authorization checks.

The host ID is a physical transcript/session identity, not a proven stable logical actor identity: compression replaces `agent.session_id` (`agent/conversation_compression.py:3287–3322`). `ctx.profile_name` is a label and falls back to `default` on error (`hermes_cli/plugins.py:399–408`); it is not a unique fail-closed profile namespace. The inspected plugin API supplies no canonical-session accessor. Therefore T-08 must require an independently verified, explicitly provisioned binding; unknown or rotated physical IDs are denied, not inferred from model text, ambient environment or a direct read of Hermes' database. Receive peer content only as bounded attributed tool results, never as system/user instructions. This is source qualification, not live plugin registration or peer-message acceptance. See the official [plugin guide](https://hermes-agent.nousresearch.com/docs/developer-guide/plugins).

## M1 compatibility gate

The standalone [peer-tool adapter](peer-tools.md) now implements the explicitly provisioned physical-ID boundary above. It has been loaded by the real pinned directory loader and invoked via the real registry in isolated home; it does not expose canonical continuity, a provider-backed turn or automatic binding as established features.

Before enabling connected mode, establish all of the following on an isolated test home and then the intended attach mode: runtime identity; supported bootstrap/auth path; non-mutating attach; ready event; create/resume; stream; real denial; expiry; interrupt; disconnect; request correlation; transcript readback; and no orphan processes. Provider-enabled checks require owner-approved inference budget and credentials supplied by the runtime.

Before exposing connected mode, T-06 must also reject malformed/oversized frames and test partial frames, EOF, timeout and bounded shutdown. These transport/failure controls belong in M1, not only in the later load campaign. Apply the process-ownership and distinct shutdown operations in [architecture](architecture.md#process-ownership-and-distinct-shutdown-operations).

The [roadmap](ROADMAP.md) keeps this gate open. Passing the preview build or replaying hand-written fixtures cannot close it.
