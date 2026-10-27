# Hermes integration

Status: experimental Rust connected implementation. The active source contract is Hermes `bf0203a794a07aac3ff764972ab9087baa4e4f4e`. Real discover/doctor/create/detach/resume/history passed without inference; provider-backed approval/stream/cancel acceptance remains open. A source revision does not prove the running gateway's build identity.

The current task-plan build repeats both opt-in real-owner tests successfully: metadata handshake and a fresh logical create/detach/resume/history cycle, with exact-ID/same-owner/yolo=false/idle/empty-history assertions. Doctor before/after confirms the same instance and authority epoch10. Only the discover-only attach helper was used; no second writer, runtime start/restart or inference. The fresh logical session persists and is not a rollback/cleanup claim. These are two separate provider-free checks, not additions to the synthetic full-suite count and not native-window/provider acceptance. Runtime/test/harness/binary match the previous full seal; the documentation supplement is separately verified.

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

Owner-reported task plan source contract: `gateway/session_controls.py:630-648` returns top-level todo_state from paired history; `gateway/run_turn_progress.py:859-873` emits the same full `{todos,revision}` through a sequenced `todo.updated` after local tool completion, using `gateway/local_viewer_events.py:224-240`. `tui_gateway/tool_progress.py:116-129` distinguishes unreported empty revision-zero/null from an explicit clear. Rust consumes only these typed reports, not raw tool-result text or unversioned tool-start guesses; optional malformed metadata does not stop ordinary session controls. Tasks remain owner/model reports, not user instructions, successful work evidence or persistent scheduling authority. Local fixture/native checks do not establish live-owner emission or compression continuity.

Current delegated-agent roster source contract:gateway/session_ancillary.py24-35/193-217 handles subagent.list {session_id} after session:read authorization withoutcoldadoption;returns {subagents:[publicfields],delegations:[]}. Owner_objects395-408 can refuse managed-worker projection. Rosterownership476-502 may include descendants orsame-database compression-lineage records;this read's ownership isnot steer/stop/tail authority,whichremainsresident-parentconstrained. Rustprojectsstrictboundedknownfields only,onexplicitRead;no provider,registryimport,localpluginload,processinspection orruntimeagentconstruction. Syntheticfixturesproveclientcontracts,notreal-owner live roster orcanonicalpeercontinuity.

### MCP qualification boundary

Source-qualified at Hermes pin `bf0203a794a07aac3ff764972ab9087baa4e4f4e`; native client implementation is local/native synthetic verified, not live-qualified. `tui_gateway/contracts/tools_mcp_plugins.py:116-154` declares `reload.mcp`, but `gateway/session_ancillary.py:263-301` accepts only `session_id`, `confirm`, `rev`, `profile`: the declaration's optional `always` is refused even when false. Do not serialize declaration defaults without checking the actual handler. The minimal confirmed request names the exact attached session and `confirm:true`; profile-wide effects are separately disclosed, never inferred from a local Runtime panel opening. Optional `coalesced` and `turn_isolation` result flags allow null/bool; parser compatibility is pinned by RED/GREEN tests, while private acknowledgements/diagnostics are not retained.

`session_ancillary.py:276-283` reconciles live profile servers and refreshes cached agents before `_commit_session_mcp` calls session authorization at :346. Permission refusal, a `session:"busy"` result, transport loss or cancelled caller therefore do not establish no profile effects. `session_policy_refresh.py:205-240` shields an admitted refresh through commit/publication; `hermes_state_mutation_prepared.py:133-147` uses owner epoch/idle and old-policy/generation CAS internally, not a caller-supplied reload fence. The request ID is not durable idempotency; do not retry automatically. `reloaded`, `session=applied|unchanged|busy`, unavailable names and selected server names are owner reports, not tool health or a correlated generation receipt.

`session_policy_refresh.py:284-296` first publishes policy then `_publish_pending`, then local `session.info`. `session_authority.py:409-424` puts the current revision/generation in the info payload; `session_events.py:38-54` independently stamps retained `execution` into the envelope. These generations can differ. Do not discard the current payload merely because the envelope retains an older execution stamp, or use an unrelated info event as a reload receipt. `config.get {session_id,key:"mtime"}` (`session_config.py:65-98`) reads a frozen-policy 12-hex `mcp_rev`, not a live-config desired revision or an owner/session/generation-wrapped snapshot. `session_authority.py:278-283` plus `slash_commands.py:184-211` only peek resident/cached agents, but that alone does not prove dispatch is passive: `session_controls.py:287-293` can adopt a cold ref before its handler, and authorization can restore it. `session.info` uses the non-atomic inventory builder with I/O/probes/update prefetch (`session_local_inventory.py:192-203,133-189`); resume can refresh/adopt and attach hooks.

Seven private diagnostics executed unchanged pinned source AST with in-memory ports: confirm-absent no effects; `always:false` refused; profile effects before session permission refusal; busy acknowledged after profile effects; retained envelope generation versus newer payload generation; frozen revision despite changed live config; internal stale-policy/generation refusal before commit with a valid positive control. All seven passed with zero ResourceWarning; no upstream module imports, live owner/config/database/provider calls or product changes. They constrain source hypotheses only, not actual SQLite, native runtime, network isolation or seven additions to the canonical suite.

Native MCP reload implements an explicitly consented alternative, not strictly passive MCP-revision readback. Runtime details → Review MCP reload is local; Confirm profile reload and reattach names the qualified owner/profile/session and discloses every-chat profile effects, server process/network changes and same-connection reattach with policy/adoption/hooks. Send one minimal reload per fresh opaque consent, then one dedicated postresult `session.resume {session_id,display_projection:true}`. An old replay/model snapshot cannot settle it. Require exact owner/profile/session/authority, yolo=false, monotonic revision/generation/sequence and applied review-generation-plus-one before/after reducer reconciliation. New replay epochs may restart sequence according to the reducer contract.

The controller caps eight admissions per connection. Only definite local pre-enqueue refusal restores capacity. Error, malformed result/readback, timeout or disconnect stays Unknown across panel close/reopen; Send and sibling mutations stay withheld, with no automatic reload/retry. Busy, unchanged, applied and unavailable names remain owner reports; completion means current reattach readback, not tool health, atomic desired-config/MCP-revision equality or exclusion of concurrent profile changes. Bounded report names are at most128 per array/256UTF-8bytes each with complete escaped projection at most64KiB; wrong types, duplicates and unsafe identities refuse without displaying raw diagnostics.

Measured full `mr-a4_woqfn/a`:362Rust/347Python,two live ignored,zeroResourceWarning,24gates/nine independent packagepairs/three staticmodes/72nativecases188pairs/249sourcewitnesses PASS, all hashes/exactsets/exit and ownership evidence parent-checked. Bounded review completed without a concrete finding, using existing test binaries rather than an independent rebuild. This closes the local/native synthetic MCP slice only. Actual-owner MCP process/network/policy and provider/window behavior remain unqualified; existing inventory remains report-only. See [MCP operations](operations.md#native-mcp-reload-from-this-checkout).

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

Text attachment source contract: pinned gateway/session_ancillary.py136-165 authorizes a subscribed session:submit file.attach and returns attached/name/ref_path/ref_text/uploaded after owner staging. This client sends only session_id/data_url/name from an explicitly reviewed immutable local UTF-8 snapshot, never path, so owner path expansion/reopening is not involved. Stage writes retained owner storage and is not a read or prompt admission; uncertain replies remain unknown without retry or remote deletion. Strict receipts must use exact source formatting and roundtrip as one full-file reference in pinned agent/context_references.py; bare trailing punctuation, line-range syntax and tilde expansion are rejected rather than repaired. A separate Add modifies only the visible local draft; the user chooses Send later, and Hermes still owns reference expansion/policy/provider execution. Synthetic staging proves no real-owner persistence or inference behavior.

A response must bind the authenticated human connection, actor, runtime session, upstream request id and request generation. UI cancellation, timeout or disconnect must never become `allow`. A peer message cannot answer an approval or change policy. Display the runtime's action details and scope before a decision.

For request families the host cannot render safely, refuse the controlled operation with a named unsupported capability. Never submit a synthetic affirmative to keep the model running. Reconnect must reconcile pending requests and expiry before enabling response buttons.

## Profile and model isolation

Native Commands/F4 also offers exact builtin /plugins through `plugins.list {}`, not `command.dispatch`, `plugins.manage` or a tool call. At pin `bf0203a794a07aac3ff764972ab9087baa4e4f4e`, canonical `gateway/session_discovery.py:38-64` requires session:read and profile equality, rejects every parameter and enters the authenticated owner profile. `hermes_cli/plugins.py:1237-1289` may initially discover/load/register plugins and refresh secret sources, so the visible owner/profile disclosure precedes the explicit pointer read. Catalog/open/typing/Enter never requests it. Rust imports no plugin or personal configuration. Bind its distinct reply variant at admission; require all three canonical arrays, bounded unique exact keys/types/counts and a complete projection. Raw errors/paths/settings are never displayed; malformed/refused/stale reports are not empty success. Current local fences, pending/Unknown guards and no automatic retry remain consequential. Full `pi-msyxyzt9/a` passes377Rust357Python/twoignored/24gates/ninepackagepairs/76nativecases200pairs/253sources;bounded review/transcript/hashes reconciled without a concrete finding. This is synthetic-owner qualification,not actual-owner plugin discovery,health or permission.

Native Commands/F4 explicitly requests `commands.catalog {session_id}`. Ten fixed built-in reads use `command.dispatch {session_id,name,arg:""}` and may run owner pre-command hooks. Exact built-in /tools instead uses `tools.show {session_id}`,never slash dispatch or tool invocation;the expected result variant is bound at admission. Pinned `gateway/session_ancillary.py:64-104` authorizes this session read and resolves definitions from the resident agent's toolsets or current policy before construction,pre tool_search assembly. The Rust client imports no local plugins and does not reload MCP or mutate toolsets. It displays only bounded exact group/tool names and control-safe descriptions,requires an exact integer total equal to every unique projected row,and refuses partial/oversized/malformed reports. Zero is a valid owner report,not a fallback for unavailable. Discovery proves neither health nor permission. Handler `exec` text stays bounded data;`send`/`skill` directives never become prompts. Profile/model/session/catalog/panel-opening fences still apply. This panel does not implement write approval,skill invocation,MCP reload or arbitrary slash commands. Queue refusal before admission differs from an unknown transmitted outcome. Other contracts remain in pinned `gateway/session_commands.py` and `tui_gateway/command_discovery.py`;synthetic/native tests do not qualify real-owner behavior.

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
