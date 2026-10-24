# Connected Hermes vertical slice implementation plan

> For agentic workers: use `superpowers:executing-plans` for native execution, or `superpowers:subagent-driven-development` if the owner selects that method. Complete the checkboxes with observed RED/GREEN evidence, not intent.

Goal: replace a preview-only product with a usable Rust client that attaches to the existing Hermes gateway, manages a real session, renders native Tern conversations and preserves human approval/cancellation semantics.

Architecture: a short, version-qualified Python discovery helper reuses Hermes' runtime bootstrap in discover-only mode and returns a private connection grant over an anonymous pipe. Rust owns WebSocket transport, bounded event state, CLI and TSP presentation. Hermes owns all inference/tools/persistence; no second writer or agent loop is created.

Tech stack: Rust 2024 / Rust 1.98.0, pinned tern-sdk, serde/serde_json, tungstenite, clap, uuid, sha2; Python 3 stdlib helper running with the installed Hermes interpreter. Use a bounded synchronous network thread to match the synchronous SDK and keep blocking I/O off the TSP owner thread.

Spec: [architecture](architecture.md), [Hermes integration](hermes-integration.md), [product requirements](product.md), [security](security.md), [verification](verification.md).

Status: implementation authorized by the owner after presentation of this plan. File and package paths below are updated for the owner-directed workspace split. Checkboxes retain the original acceptance sequence, not current task state; [STATUS](STATUS.md) and [tasks.json](tasks.json) report measured implementation and open gates. The owner subsequently authorized the initial source/docs commit and push to `https://github.com/0xharryriddle/hermes-tern`, then continued native implementation. Personal-runtime reconfiguration and provider-budget gates remain separate.

## Global constraints

- All docs remain in this `docs/` directory.
- Keep the SDK revision `9e9c07463f4958b0c633424c9860aba499487d28` and Rust `1.98.0`.
- Keep the current preview/help/version behavior working; invalid arguments exit 2.
- Never ensure/recover/start/restart an existing Hermes owner from default attach.
- Never modify Hermes core, personal plugins, profile config, credentials or live database files.
- Never log gateway tickets or expose them in CLI arguments; output to an anonymous child pipe only.
- No automatic approval, hidden sensitive input, runtime protocol downgrade or shell scraping.
- Use binary crates under `bin/` and library crates under `crates/`, following the owner-directed [workspace layout](workspace-layout.md). No no-op runtime adapters.
- Native rendering, window plugin action and provider-backed acceptance are separate gates.
- Provider-backed tests use a bounded owner-approved budget; absence of approval/credentials is a named external gate, not synthetic success.
- Initial source/docs commit and push are explicitly owner-authorized; later promotion must stay within that requested scope and preserve truthful acceptance limits.

## Historical pre-implementation baseline

At plan creation the workspace contained a compiled preview, 7 CLI tests and 14 Python checks. The SDK renders native light/dark flow output. Personal plugin install and provider inference have not been performed. A real-window probe failed to produce preview text while its isolated Tern access gate was unsigned-in; this remains open T-03, not a skip.

Hermes source baseline for this slice: `bf0203a794a07aac3ff764972ab9087baa4e4f4e`. The current checkout was clean at discovery. Re-check it before contract capture; refuse incompatible capabilities even if the version string resembles this baseline.

A discover-only live probe succeeded with `hermes-gateway-v1`, `ping -> {pong:true}`, and `runtime.describe`. It created no sessions and performed no inference. Reported capabilities were durable-admission-v1, event-replay-v1, local-cli-create-v1, acp-editor-policy-v1 and acp-session-mcp-v1. The create sources include tui; accepted field names are obtained from runtime.describe rather than assumed.

### Source contract anchors

These paths are in the inspected Hermes source, not files created in this repository:

- `ui-tui/scripts/gateway_bootstrap.py`: `bootstrap(False, False)`, discovery, same-user private ticket and WS target.
- `hermes_cli/gateway_client.py`: canonical subprotocol, ticket header and response/transport-error distinction.
- `gateway/session_contract.py`: immutable session handle, admission receipt, snapshot and generation semantics.
- `gateway/session_controls.py`: `describe`, `create`, `resume`, `submission`, `receipt`, `interrupt`, `respond`, `respond_clarify`, `detach`.
- `gateway/session_events.py`: sequenced `method=event`, `params.type/session_id/payload/replay_epoch/seq`.
- `gateway/session_pending_controls.py`: approval/clarify prompts, generation binding and settled events.
- `gateway/session_server_requests.py` and `tui_gateway/contracts/server_requests.py`: string `srq-` server requests and responses.

## Review focus

1. Ticket-bearing diagnostics or redirect to a non-loopback peer: block before connection and redact every error path (Tasks 1-3).
2. A receipt arriving after streamed events, or duplicate/reordered events after reconnect: preserve ordering and reconcile snapshots without duplicate submit (Tasks 3-4).
3. A stale approval dialog, foreign session event or reused generation: refuse the answer rather than approve the wrong operation (Tasks 4-5).
4. Unicode draft editing and wake-without-input: no cursor corruption, missing draft or accidental UI exit (Task 5).
5. Closed stdin, slow consumers, oversized frames and an exited helper: bounded termination and a nonzero useful error, never a hanging or fake-success CLI (Tasks 1-3, 6-7).

## File boundaries

| Path | Responsibility |
| --- | --- |
| `crates/core/src/lib.rs`, `crates/gateway/src/lib.rs`, `crates/ui/src/lib.rs` | Public library boundaries and exports |
| `crates/gateway/src/config.rs` | Strict CLI/runtime options, limits and trusted helper selection |
| `crates/core/src/error.rs` | Typed bounded errors and safe user diagnostics |
| `crates/gateway/src/bootstrap.rs` | Bounded discovery child, secret grant handling, validation |
| `crates/gateway/bridge/hermes_discover.py` | Discover-only adapter to the installed Hermes bootstrap |
| `crates/gateway/src/transport.rs` | Qualified WS connection, bounded queues, response correlation |
| `crates/gateway/src/protocol.rs` | Wire requests, snapshots, receipts, events and server requests |
| `crates/gateway/src/session.rs` | Session/turn/prompt reducer and typed actions |
| `crates/ui/src/composer.rs` | Unicode-safe draft edits and cursor conversion |
| `crates/ui/src/view.rs` | Native SDK projection from state; no runtime side effects |
| `crates/ui/src/app.rs` | Terminal loop, CLI/plain run orchestration and shutdown |
| `crates/cli/src/lib.rs` | CLI parse, dispatch and exit codes |
| `bin/hermes-tern/src/main.rs` | Thin production entry point calling the CLI library |
| `bin/testing/src/main.rs` | Labelled native fixture executable, no inference |
| `crates/gateway/tests/`, `crates/ui/tests/`, `bin/hermes-tern/tests/` | Boundary/state/transport/CLI behavior tests |
| `tests/test_discover.py` | Helper isolation, non-mutating discovery and secret/error handling |

No actor store is added to this first slice. The next plans depend on these interfaces rather than creating a competing runtime while attachment is unsettled.

## Implemented follow-up: reproducible native fixture smoke

The interactive fixture is already implemented; its historical isolated TTY failure is not evidence against current bytes. Re-run the real Tern harness in private HOME/config/state/cache/runtime/socket roots before changing Rust. A syscall trace now establishes raw-mode setup, TSP hello/frame/ACK and clean exit for the unchanged fixture. Waiting for dock-only `FIXTURE ONLY` text times out while a main-region `Hermes / FIXTURE` assertion passes on the same binary; the actual light/dark images include both main and dock. This identifies an assertion-scope problem in the attempted harness, not a proven universal explanation of the earlier TTY error.

Implementation scope: `scripts/smoke_native.py` and `tests/test_native_smoke.py`; checkpoint/runbook scope: this plan, `docs/STATUS.md`, `docs/ROADMAP.md`, `docs/tasks.json`, `docs/verification.json`, `docs/verification.md`, `docs/development.md`, `docs/workspace-layout.md`, root README/CHANGELOG and the ignored implementation ledger. Default static preview is preserved; explicit `--fixture`/`--size` select the labelled testing binary and bounded viewport. The main-region text assertion is paired with independent visible in-viewport main/dock/editor/control geometry. Short private roots and an environment allowlist exclude personal Tern/Hermes context. Results, partial timeout diagnostics and exact scratch inventory are retained without deletion.

Eight harness tests pass after initial missing-API RED and a subsequent lost-timeout-log RED. These test Python harness behavior, not native acceptance. Separate real Tern 0.9.5 runs passed preview light/dark and fixture light/dark at 1100×760 and 480×760 (scale 2); all four fixture images were inspected without visible clipping or overlap. Deliberately absent text exited 1 and produced no screenshot. `./verify.sh` passed 129 Rust/101 Python with two live opt-ins ignored and no ResourceWarning; all eight standalone package test/clippy gates passed. No pinned SDK, Rust source, provider routing or frozen credential-review member/binary changed. T-03 real-window/plugin, provider-backed behavior and interactive action semantics remain distinct open gates; the earlier TTY error remains historical, not a fully diagnosed universal root cause.

## Task 1 — Strict options, bounded errors and testable package boundary

Files:
- Create: `crates/core/src/lib.rs`, `crates/gateway/src/lib.rs`
- Create: `crates/gateway/src/config.rs`
- Create: `crates/core/src/error.rs`
- Create: `crates/gateway/tests/config.rs`
- Modify: `Cargo.toml`
- Modify: library and binary `Cargo.toml` files listed in [workspace layout](workspace-layout.md)
- Modify: `Cargo.lock`

Interfaces:
- Produce `Limits { max_frame_bytes: usize, max_events: usize, max_pending_requests: usize, connect_timeout: Duration, request_timeout: Duration }` with validated positive bounds.
- Produce `AttachOptions { hermes_root: PathBuf, python: PathBuf, home: Option<PathBuf> }` and `SessionOptions { cwd: PathBuf, resume: Option<String>, model: Option<String>, provider: Option<String> }`.
- Produce `Error` variants for invalid configuration, bootstrap, transport, protocol, refusal, timeout, disconnected/unknown outcome and unsupported capability. Display excludes raw tickets and remote free-text dumps.

- [ ] Write tests before implementation:

```rust
#[test]
fn limits_reject_zero_before_opening_transport() {
    let mut limits = Limits::default();
    limits.max_frame_bytes = 0;
    assert!(limits.validate().is_err());
}
#[test]
fn relative_runtime_paths_are_not_silently_resolved_from_peer_text() {
    let options = AttachOptions::new("relative/root", "/usr/bin/python3", None);
    assert!(options.is_err());
}
```

- [ ] Run `cargo test --locked -p hermes-tern-gateway --test config`; observe the missing API/validation failure.
- [ ] Implement concrete types, defaults and reject-before-I/O validation; add dependency pins discovered from real crate metadata, regenerate lock intentionally.
- [ ] Run the focused test, `cargo check --workspace --all-targets --locked` and strict clippy.
- [ ] Rollback: keep connected commands unavailable; preserve preview. Do not remove validation to make an unavailable runtime pass.
- [ ] Evidence: retain command/exit/result in the tranche checkpoint; update status only after the gate.

## Task 2 — Discover the existing Hermes owner without starting one

Files:
- Create: `crates/gateway/bridge/hermes_discover.py`
- Create: `crates/gateway/src/bootstrap.rs`
- Create: `tests/test_discover.py`
- Create: `crates/gateway/tests/bootstrap.rs`

Interfaces:
- Consume `AttachOptions` and `Limits`.
- Produce `ConnectionGrant { url, protocols, profile_id, instance_id }`; do not implement Debug that prints protocols/tickets.
- `discover(&AttachOptions, &Limits) -> Result<ConnectionGrant, Error>` starts only the bounded helper; no Hermes runtime process.

Helper contract: require explicit root/interpreter supplied by CLI/config or safely discovered installer metadata. Import Hermes bootstrap under its runtime Python, load the inspected gateway helper and call only `bootstrap(False, False)`. Emit exactly one JSON grant to stdout and bounded sanitized error code to stderr; never expose result in ordinary diagnostics. The optional home selection must be explicit and may not copy auth/database files. Upstream discovery may tighten an existing same-owner lock mode; disclose that limited effect rather than claiming absolute filesystem immutability.

- [ ] Write helper tests with a task-owned source fixture whose bootstrap function records its booleans. Require `(False, False)` exactly, unchanged fixture config, no credential fallback and a rejected malformed grant.
- [ ] Write Rust tests:

```rust
#[test]
fn grant_rejects_non_loopback_or_credential_in_url() {
    assert!(ConnectionGrant::parse(br#"{"url":"ws://example.com/api/ws","protocols":["hermes-gateway-v1"],"profile_id":"p","instance_id":"i"}"#).is_err());
}
```

- [ ] Observe RED via `python3 -B -m unittest discover -s tests -p test_discover.py -v` and `cargo test --locked -p hermes-tern-gateway --test bootstrap`.
- [ ] Implement anonymous-pipe capture with byte cap, deadline, reap-on-exit and explicit process ownership. Reject wrong URL scheme/path, userinfo, fragment, missing/duplicate protocols and missing identities. An unavailable owner returns a failure, never ensure/recover.
- [ ] Verify local helper tests and a discover-only live probe; print only sanitized capability metadata.
- [ ] Rollback: disable attach; do not fall back to a second writer or another protocol.

## Task 3 — Qualified WebSocket worker and typed canonical wire model

Files:
- Create: `crates/gateway/src/transport.rs`
- Create: `crates/gateway/src/protocol.rs`
- Create: `crates/gateway/tests/transport.rs`
- Create: `crates/gateway/tests/protocol.rs`

Interfaces:
- Produce `RpcClient::connect(ConnectionGrant, Limits)`, `request(method, params) -> RequestId`, `respond_server(request_id, result)`, `try_recv() -> Option<Inbound>`, and `close()`.
- `Inbound` distinguishes RPC result/refusal, sequenced event, server request and disconnected/unknown outcome. Request IDs and server `srq-` IDs cannot collide.
- The network worker owns its socket; the caller owns the view. Bounded channels and pending-request maps fail closed on overflow/stall instead of dropping terminal decisions.

- [ ] Add local real-WebSocket fixture tests for subprotocol mismatch, response-before/after events, duplicate JSON keys, malformed frames, binary/oversized messages, EOF, timeout, pending-response failure and secret-redacted diagnostics.
- [ ] Canonical event test:

```rust
#[test]
fn ready_is_an_event_type_not_a_request_method() {
    let event = parse_frame(r#"{"jsonrpc":"2.0","method":"event","params":{"type":"gateway.ready","payload":{}}}"#).unwrap();
    assert!(matches!(event, Inbound::Event(ref e) if e.kind == "gateway.ready"));
}
```

- [ ] Observe RED with `cargo test --locked -p hermes-tern-gateway --test transport --test protocol`.
- [ ] Implement handshake with exact `hermes-gateway-v1` echo, loopback-only initial transport, no Origin header, configured timeouts and byte caps. Do not route local WS through ambient HTTP proxies. Tickets are single-use and expire after 30 seconds: rediscover/mint for each reconnect, never reuse a grant. Reject unsupported versions; no fallback.
- [ ] Implement explicit canonical request constructors, never a permissive raw-RPC UI. Fetch `runtime.describe` before session creation, check instance/profile identity against the grant, and verify required capabilities/field support. Advertise `client.capabilities {server_requests:true}` after request handlers are installed. Use `session.info` for structured status; canonical `session.status` is unavailable. Send one JSON object per WS message, not concatenated NDJSON.
- [ ] Run local fixtures and read-only live ping/describe through the Rust transport.
- [ ] Rollback: close only our WS/helper; never signal an attached gateway. Pending submissions become outcome-unknown.

## Task 4 — Session reducer, durable admission receipts and human requests

Files:
- Create: `crates/gateway/src/session.rs`
- Create: `crates/gateway/tests/session.rs`
- Modify: `crates/gateway/src/protocol.rs`

Interfaces:
- Produce `SessionState` with session/profile/instance identities, subscription, replay epoch/sequence, generation, pending admission IDs, transcript/tool projection, draft-independent prompt registry and connection state. `SessionState::from_snapshot(profile: &str, instance: &str, snapshot: serde_json::Value) -> Result<Self, Error>` is the real hydration boundary, shared by runtime and tests.
- `apply(Inbound) -> Result<Vec<Action>, Error>` validates session/generation and derives follow-up reconciliation requests.
- `submit(text: &str, input_id: &str) -> Result<Action, Error>`, `interrupt() -> Result<Action, Error>`, `answer_approval(prompt_id: &str, generation: u64, choice: &str) -> Result<Action, Error>`, `answer_clarify(prompt_id: &str, generation: u64, answer: &str) -> Result<Action, Error>` and `detach() -> Result<Action, Error>` return typed actions, not raw model-controllable RPC.

Verified requests (one object per separate WebSocket message):

```jsonl
{"method":"session.create","params":{"source":"tui","request_id":"stable-create-id","cwd":"/absolute/workspace","yolo":false}}
{"method":"session.resume","params":{"session_id":"opaque-session","display_projection":true}}
{"method":"prompt.submit","params":{"session_id":"opaque-session","input_id":"stable-input-id","text":"hello"}}
{"method":"prompt.receipt","params":{"session_id":"opaque-session","admission_id":"server-issued-id","include_result":true}}
{"method":"session.interrupt","params":{"session_id":"opaque-session","execution_generation":1}}
{"method":"approval.respond","params":{"session_id":"opaque-session","execution_generation":1,"prompt_id":"upstream-prompt","choice":"deny"}}
{"method":"session.detach","params":{"session_id":"opaque-session","subscription_id":"upstream-subscription"}}
```

Examples are parameter contracts, not captured successful mutation responses. Use runtime-supported source/fields and do not invent a custom source label.

- [ ] Test stale/foreign approval, settled request, wrong generation, duplicate event, replay gap, out-of-order receipt, a disconnect after submit and a server request requiring unsupported sensitive input.
- [ ] Test the crucial boundary:

```rust
#[test]
fn disconnect_never_resubmits_an_ambiguous_prompt() {
    let mut state = SessionState::from_snapshot("profile", "instance", serde_json::json!({
        "session_id": "session", "subscription_id": "subscription",
        "execution_generation": 1, "authority_epoch": 1, "revision": 1,
        "replay_epoch": "epoch", "last_sequence": 0, "running": false,
        "messages": [], "pending": [], "prompts": [], "info": {}
    })).unwrap();
    state.submit("review this change", "input-1").unwrap();
    let actions = state.apply(Inbound::Disconnected).unwrap();
    assert!(actions.iter().all(|a| !matches!(a, Action::Submit { .. })));
    assert!(state.outcome_unknown());
}
```

- [ ] Observe RED with `cargo test --locked -p hermes-tern-gateway --test session`.
- [ ] Implement snapshot hydration from display_messages when supplied, otherwise typed stored content. Preserve parallel tool-call IDs. Use upstream receipts to reconcile completion; no spinner-based result inference.
- [ ] Handle both generation-bound approval/clarify events and string-ID server requests. Unsupported sensitive requests are explicitly declined/skipped with their documented response or error, never approved or collected in plaintext.
- [ ] On replay gap, suspend stale controls and request one snapshot; on epoch change reset the watermark only with the new snapshot. Detach never terminates the backend.
- [ ] Rollback: connected commands fail closed with session ID retained for recovery.

## Task 5 — Native composer, UI projection and terminal event loop

### Implemented follow-up: production native chat loop; review F1 parent-remediated

The offscreen fixture now renders, but its loop intentionally discards actions. The two existing native tests cover wake/EOF and one interrupt key without exercising `chat` or gateway dispatch. Add `tests/test_native_chat.py` using the actual production CLI over an owned PTY and a bounded loopback WebSocket fixture. The discovery module lives only in a private synthetic runtime root and asserts discover-only arguments; no Hermes runtime, personal profile, real credential or provider is accessed. Decode actual SDK output and inject source-qualified TSP messages, not a replacement application renderer.

Pin behavior through the existing entry point: keyboard, Send button and native composer submission; exact approval IDs/generation and inert stale actions; Ctrl+C without draft submission; empty/no-ACK and delayed-ACK behavior; surface/node `gone`, terminal `error`, and transport loss; terminal restoration and detach rather than backend interruption on viewer exit. Observe behavioral RED before changing product code. Initial source inspection suggests all unhandled terminal events are currently treated only as repaint requests, so surface removal may become a render error and terminal rejection may be ignored; confirm with production tests before fixing. Scope source fixes to `crates/ui/src/app.rs` and only directly necessary `view.rs`/`composer.rs` changes, retain pins and Cargo.lock, and do not introduce runtime test switches or public no-op adapters.

Test/implementation files: `tests/test_native_chat.py`, `crates/ui/src/app.rs` and directly required UI siblings only. Checkpoint scope: STATUS/ROADMAP/tasks/verification, this plan, development/operations runbooks, CHANGELOG and the ignored ledger. Keep bounded fixture/protocol evidence distinct from actual Tern pixels, real-window/plugin execution and live-agent acceptance. Re-run `./verify.sh`, independent affected packages and actual rendering when UI bytes change; a later binary is not the exact binary of the earlier credential review even when its credential source cohort remains unchanged.

Measured scope amendment before approval fix: the production PTY probe observed an old generation-2 approval node issue a generation-3 `approval.respond` while frame credit was blocked. The pinned SDK replaces handler routes at render time, before the pending frame is ACKed. A node key based only on prompt ID (or only adding generation) also permits same-generation content/choice replacement. Extend the allowed source/test scope to `crates/gateway/src/session.rs` and `crates/ui/tests/view.rs`: assign each hydrated prompt an opaque local control identity, retain it across ordinary repaints, change it on replacement, include it in native node IDs and typed actions, and validate it against the current prompt before dispatch. The native dispatch guard also fences actions already decoded before the gateway replaces a prompt. This is local UI correlation, not a new upstream wire parameter, capability grant or provider behavior. Add same-generation choice-swap and queued-action regressions before implementation; keep plain operator syntax unchanged and disclose its separate contract.

Sibling control check also reproduced an old Interrupt button cancelling the later turn. Bind pointer interrupt node/action to its rendered execution generation and reject a decoded stale tuple; keep keyboard Ctrl+C explicitly targeted at the current turn. This stays inside the declared UI/view-test scope. The earlier full green run is historical until the expanded regression and final full gate pass.

Historical parent outcome before review: all five production regressions passed after repair; fifteen Python methods and three new Rust view tests passed, full gate 132 Rust/116 Python with two live opt-ins ignored and no ResourceWarning. All eight standalone packages passed locked tests/clippy; lock/pins unchanged. Four actual Tern production-client cases exercised keyboard input, one synthetic prompt/receipt/result, pointer Detach and exit-0 sentinel at wide/narrow light/dark; real images inspected without clipping/overlap. Earlier Ctrl+d scenario lacked a measured detach and is not credited as exit acceptance. No provider, real Hermes owner or window-plugin acceptance was claimed.

Review `deleg_b37cb913` found F1 (P2): the owning-surface early break discards a current/global Error queued before Gone in the same SDK read, including behind a harmless Theme. Parent verified all five source/two binary identities and 243 indexed review artifacts, then repeated the exact six-method reproducer in a private output root: two controls passed, four Error+Gone assertions failed (exit 0 instead of 1). The earlier suite was green but did not cover this composition.

F1 remediation scope: `crates/ui/src/app.rs`, `crates/ui/tests/native.rs`, `tests/test_native_chat.py`, and checkpoint/runbook documents. Add production PTY regressions first, then deterministic real-SDK single-chunk cases. Keep all actions fenced once the owning surface is closed; inspect every already-decoded input for current/global rejection, regardless of position relative to Gone. Use the pinned SDK inbox-before-waker ordering as a no-new-read barrier: signal its waker once, drain queued input only, stop at the wake result, and cap item count. Missing waker or excess queue must fail closed, never wait/read indefinitely or report a clean exit with unknown status. Production Tty supplies a waker; custom unwakeable transports get an explicit refusal. Move lifecycle classification to the shared production input adapter, without changing SDK/pins/provider/gateway authority. Test unrelated errors and plain Gone remain clean, queued actions stay inert, no extra reads occur, and rejection/redaction/detach/termios survive all compositions. Rebuild both binaries, rerun focused/full/package gates and actual Tern input/render evidence before closing F1; retain the original review defect verdict as historical, not NO_DEFECT_ESTABLISHED.

F1 outcome: parent reproducer 6/6 and current native suite 18/18 pass; three added Rust methods constrain both error/Gone orderings, a no-extra-read sentinel, missing-waker refusal and oversized-inbox failure. Full gate 135 Rust/119 Python, two live ignored, no ResourceWarning; eight package tests/clippy pass. Three reviewer SDK probes pass against current libraries; pristine mutation control passes and four scratch-only guard mutants fail as expected. Four repaired-binary Tern wide/narrow light/dark keyboard/send/result/pointer-detach cases and static renderer checks pass; four current production-chat images inspected. Scope remains synthetic gateway. Review is reconciled with parent remediation, not a fresh independent verdict; proceed to remaining dependency-ready work.

Files:
- Create: `crates/ui/src/composer.rs`
- Create: `crates/ui/src/view.rs`
- Create: `crates/ui/src/app.rs`
- Create: `crates/ui/tests/view.rs`
- Create: `crates/ui/tests/composer.rs`
- Modify: `crates/ui/src/lib.rs`

Interfaces:
- `Composer { text, cursor_chars }` exposes insert/backspace/delete/move and a UTF-16 cursor for TSP.
- `render(&SessionState, &Composer) -> View<UiAction>` creates stable transcript/tool/prompt IDs.
- `run_native(options) -> Result<(), Error>` owns one SDK Session, receives network events through bounded queues and keeps ACK pumping independent of inference.

- [ ] Write Unicode edit tests with ASCII, combining characters, emoji and pasted newlines; verify no invalid byte slicing and no draft loss after a backend event.
- [ ] Test pure view props and prompt actions with controlled state, then scripted Terminal input. A Waker returning `Ok(None)` must continue; `InputClosed` exits cleanly.
- [ ] Observe RED with `cargo test --locked -p hermes-tern-ui --test composer --test view`.
- [ ] Implement `main` transcript/tool regions, `dock` composer/status, contextual human approval controls. Keep disconnected/unknown state explicit; no fabricated costs or replies.
- [ ] Advertise native SEND only with a real common submission path and set sendable only when ready. Ctrl-C requests turn cancellation; explicit exit detaches. Do not interpret close-view as terminate-backend.
- [ ] Exercise no-ACK/slow view, resize/theme, retention and error cleanup with actual SDK test transport. Then produce real native light/dark renders from a labelled fixture state; keep fixture UI evidence separate from live runtime acceptance.
- [ ] Rollback: preserve draft and recorded session ID; return a useful failure without leaving raw mode.

## Task 6 — Connected CLI, plain mode, inspection and launcher

Files:
- Modify: `crates/cli/src/lib.rs`
- Modify: `bin/hermes-tern/tests/cli.rs`
- Modify: `plugins/hermes-tern/window.luau`
- Modify: `plugins/hermes-tern/plugin.toml`
- Modify: `README.md`
- Modify: `docs/configuration.md`
- Modify: `docs/operations.md`

Commands to implement: `doctor` (read-only discovery/capability report), `sessions` (explicit saved/live list), `chat` (new or resume; native or plain), and existing `preview`. `chat --query TEXT --plain` is a bounded one-turn consumer with explicit session ID and outcome. No generic arbitrary RPC command is exported.

- [ ] Extend CLI tests with unknown flags, trailing arguments, closed stdin, unavailable owner, helper timeout, non-TTY mode, and executable child-launch traps. Use the freshly built binary and bounded subprocess waits.
- [ ] Observe RED; implement clap parsing before any discovery/terminal I/O. Default no-argument behavior remains help; launch into chat is an explicit palette command.
- [ ] Plain mode displays real streamed events and requests human input only on an interactive terminal. In noninteractive query mode, approval requests are denied and clarify/sensitive requests are explicitly unanswered; report this policy before execution.
- [ ] Add a fixed `hermes-tern chat` Luau command while retaining preview. Do not interpolate prompts/paths into the launch command or auto-start on plugin load.
- [ ] Run full `./verify.sh`, scratch binary install, plugin package check and isolated native renderer.
- [ ] Rollback: revert only new launcher selection to preview; do not weaken connected policy or touch user plugin config.

## Task 7 — Real acceptance, docs and precise closure

### Authorized follow-up: repeatable local autorun and native control matrix

The owner explicitly requested automatic execution of tests while completing implementation. Ship `scripts/smoke_native_chat.py` instead of relying on retained scratch probes: run the actual production executable in real isolated Tern against the existing bounded loopback fixture, covering conversation, explicit Deny, clarification Skip, pointer Interrupt and disconnect/no blind retry. Reuse `tests/test_native_chat.py` as the source-qualified fixture, subclassing only synthetic replies; never load personal runtime, submit affirmative real approvals or present fixture RPCs as provider acceptance. Require exact request counts/arguments, main text readiness, real image/layout output, an exit-0 shell sentinel and clean owned gateway termination. Use fixed scenario templates and quote executable paths; no peer text enters shell commands. Keep retained outputs under private short roots and never remove them.

Add `scripts/verify_runtime.py` as an explicit autorunner: execute `./verify.sh`, derive and test/clippy every package from Cargo metadata, then default to static and production-chat native matrices when Tern is available. Missing requested tools, failed/timeout commands or missing result evidence must make the requested run fail, not skip-to-green. Offer explicit `--local-only` for hosts without Tern, with an honest local-only scope. No inference, profile mutation, personal plugin installation, gateway restart or publication occurs. Store per-command exit/deadline/log/hash and source/binary witnesses plus external-gate states in a task-owned report. Use bounded output capture and process-group ownership for commands; never kill processes merely by name or inherited TMPDIR. Exact `--out` must be new so prior evidence cannot be overwritten. New tests in `tests/test_runtime_autorun.py` first observe RED, then exercise real child return codes, timeout and output caps, result validation, credential-free renderer environment and conservative action expectations. No Cargo/SDK/production changes unless these runtime probes establish a concrete defect.

This local gate is not T-06 live provider, T-03 window-plugin, T-10 parallel live actors, T-11/12 OMP, T-13 max-resource or T-14 license/publication acceptance. Read-only source discovery for OMP proceeds independently; no task is marked complete from a planned dependency or missing external account.

Measured harness amendment: real Tern startup exceeded the PTY fixture's three-second accept deadline, so the synthetic server exited before the production client connected. Extend the shared `tests/test_native_chat.py` fixture with explicit bounded accept/read deadlines and interruptible no-client shutdown, retaining its short defaults for ordinary tests. Renderer scenarios select longer bounded deadlines; these are harness parameters, not production timing relaxations. The failed full-autorun logs remain retained. A post-disconnect second-Enter/click variant was also timing-sensitive across untraced/traced runs; no universal selector or product-root-cause claim is made. Keep no-resubmit behavior in the existing production PTY test and measure renderer draft/status/exit separately.

Prior autorun checkpoint was partial:standalone native20/20 passed but full coordinator/native fixture startup failed. That generation and its failed logs remain retained. The readiness continuation below supersedes the full-gate failure only after test-first correction and a complete rerun;no production policy was relaxed. OMP remains qualified only for minimum isolated no-inference transport.

### Test-only readiness continuation

The retained full-gate failures occur before behavior assertions: coordinator startup spends time in page/journal I/O, and the native fixture starts its accept clock before completing private filesystem setup. Replace arbitrary three-to-five-second startup assumptions with a separately named, bounded fifteen-second test startup allowance and fail-fast child-exit checks. This is not a product latency guarantee: production IPC read/write, helper, approval-expiry, operation, mutation-barrier and shutdown deadlines remain unchanged. Do not retry mutations or turn a timed-out operation into a successful assertion.

Parent edit scope: `bin/hermes-tern/tests/coordinator_process.rs`; new `tests/fixture_process.py`/`tests/test_fixture_readiness.py`; native fixture startup/ready only; shared coordinator and peer-bridge startup plus the six fault-suite callers that duplicate it. Test delayed startup, exited-child refusal even after a successful status response, never-ready refusal and bounded input validation before using the helper. Native delayed-launch regression must exercise the real CLI/PTY/WS path. Record RED/GREEN and run the full autorun without changing production code or hiding prior failures. A bounded read-only reviewer checks only startup call sites and untouched safety-semantic deadlines.

Initial readiness outcome:5Python methods and missingRust helper RED,including real delayed3.2s launcher failure;then GREEN and first full136Rust/136Python autorun pass. This generation remains historical after the review-driven helper expansion below.

Review reconciliation: `deleg_89e43e4e` read earlier loops,not the final helper. Parent verified eight call sites and read-only status→Hello mapping. Pre/post liveness and remaining-budget checks already closed part of its guidance. Three real-process regressions exposed blocked status timeout and delayed supervisor-exit detection;bounded polling,kill/reap before read-only retry and one overall deadline now pass. Eight readiness methods GREEN. Generic operation helpers,production2s write/3s response/500ms server-frame,expiry/barrier/shutdown semantics unchanged. Final default autorun passes136Rust/139Python,2liveignored,8package pairs and20native cases/32pairs. T-16 complete;not independent post-fix review or full product acceptance.

Files:
- Create: `scripts/smoke_gateway.py`
- Create: `docs/runtime-compatibility.md`
- Modify: `verify.sh`
- Modify: `docs/README.md`
- Modify: `docs/STATUS.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/tasks.json`
- Modify: `docs/verification.json`
- Modify: `CHANGELOG.md`

- [ ] Run all deterministic tests with no provider credentials and an isolated environment. Include fault injection and an expected-refusal positive control.
- [ ] Re-run Rust doctor against the current owner without creating sessions. Verify no gateway PID/instance change and no credential/ticket leakage.
- [ ] With approved provider budget, create a separate named test session, submit a harmless prompt, observe durable terminal receipt and read back its actual assistant result. Exercise denial, expiry, cancel and resume using a controlled test workspace; never operate on the current conversation.
- [ ] Run a real native Tern client session if the access gate permits an authorized test window. Do not copy personal auth or bypass closed-beta checks. If blocked, retain failure details and keep T-03/M2 runtime acceptance open.
- [ ] Update feature matrix with implemented/tested/unsupported values. Keep live provider and fixture evidence separate. Recompute source witnesses and test counts after the last edit, then run `./verify.sh` again.
- [ ] Parent applies task transitions only for fully satisfied acceptance. No universal completion claim while a named live/external gate remains unmet.

## Full-project continuation, not a scope reduction

This plan is the first connected vertical slice of the user's full-build request. The existing roadmap remains the complete goal. Once the interfaces above are stable, prepare and execute the next dependent plans rather than adding speculative placeholder code now:

| Workstream | Existing tasks | Required concrete deliverable |
| --- | --- | --- |
| Agent-visible peer provenance | T-08 | Optional standalone Hermes plugin with narrow send/read/result tools; metadata sourced from trusted session context, tool-result delivery instead of user/system injection |
| Durable local coordination | T-09 | Rust Unix-socket supervisor, strict request framing, SQLite transactions, actor capabilities, admission dedup, dispatch ambiguity, task lease fencing and bounded storage |
| Parallel Hermes | T-10 | Independently bound sessions/workspaces, effective provider/model evidence, actor views and cooperative lifecycle |
| Native OMP | T-11 | Qualified Rust-to-Luau connector and explicit consent-based handoff through installed AgentCx; window-loss and readiness semantics tested |
| Direct OMP evaluation | T-12 | Actual RPC schema capture; implement only demonstrated required capabilities, otherwise record justified optional-lane deferral |
| Load/recovery/security | T-13 | Reproducible one/two/four-actor workload, crash-point recovery, overflow/disk controls; no fake sandbox claim |
| Packaging/release | T-14 | Runnable binary/plugin packages, installation and rollback, license decision, exact release verification; publishing remains separately authorized |

These streams are not complete because the first client compiles. Each must have concrete Create/Modify ownership and tested interfaces before implementation. OMP source research has now identified the pinned custom JSONL/v2 framing, prompt-result, settled-state and approval contracts; see [actors](actors.md#direct-omp-rpc-lane). Installed runtime acceptance is still open; no unknown RPC shape is hardcoded into this plan.

### Implemented local OMP handoff subset

The [handoff contract](omp-handoff.md) now owns T-11's local subset: `crates/omp`, `crates/cli/src/cli_omp.rs`, explicit window integration and production process/Luau tests. The local subsystem can be verified before T-10 without closing the dependent live-actor gate. Final autorun passes 140 Rust/156 Python, nine standalone packages and all 24 gates; two live tests remain ignored. Design review is parent-reconciled with source-label/outcome/producer-region RED/GREEN and six scratch guard mutations. Actual window execution and provider-backed OMP behavior remain open. No automatic mailbox ACK, resend, approval or actor-identity claim is introduced.

## Execution review

Recommended execution method: Native (parent implements; independent agents perform read-only contract/review work). The transport/state/UI interfaces are tightly coupled and the workspace has no committed base; this avoids concurrent code ownership conflicts.

Owner instruction received: begin and complete implementation. Native execution is selected; proceed through the dependent roadmap without repeated plan-approval prompts. Personal runtime changes, publishing and destructive actions retain separate approval boundaries.
