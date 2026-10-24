# Bounded canonical-history projection viewer

Status: implemented and local/native synthetic verified, with partial acceptance under T-07. Complete autorun178Rust/191Python,2liveignored,0ResourceWarning,24gates,9package pairs,36native cases/80pairs PASS. Updated pixel inspection and independent final-source approval are unavailable; the delayed bounded review is fully received and parent-reconciled; full-history/server-paging and live/provider acceptance remain open. [STATUS](STATUS.md) owns measured evidence.

## Pinned upstream contract

Hermes `bf0203a794a07aac3ff764972ab9087baa4e4f4e`, `gateway/session_tui_ops.py:121–133` accepts only `session_id` and optional `profile` for `session.history`. The caller needs `session:read` on an already live session. It reads the current physical transcript target with ancestors and durable row IDs, then returns `{count, messages}`. It accepts no limit/cursor/inline-images flag. `count` counts source conversation rows before display projection; it need not equal `messages.length`.

`gateway/session_controls.py:97–103` delegates to the upstream display projector. `tui_gateway/session_history.py:285–362` filters hidden/scaffolding rows, labels tool calls, omits most tool outputs, and may carry assistant sidecars. This is not a raw transcript export. Do not promise omitted bodies or treat projected ordinals/tool IDs as durable authority.

## Client boundary

Explicit `hermes-tern history --session ID [--plain] [--page N]` with the existing `--hermes-root`, `--python`, `--home` discovery options. It connects to the selected existing owner, qualifies identity/capabilities, requests exactly `session.history {session_id: ID}` once and closes that connection before presentation. No create/resume/subscription/submit/interrupt/approval, runtime restart, direct DB access, automatic refresh or disk export. Stored-only/unavailable sessions fail explicitly; history inspection does not auto-resume them.

A fetched snapshot remains immutable while paging. Its source row count and projected row count are labelled separately; no equality assertion or inferred missing-page count. The client preserves every projected row, including unknown fields and sidecars, as JSON-escaped inspection text. Terminal controls and bidi controls are visibly escaped, never executed. Embedded URLs/commands remain text, not command/link widgets.

The existing 8 MiB transport bound remains unchanged. Refuse malformed replies, more than 4096 projected rows or a projection whose escaped retained text exceeds 8 MiB; do not publish a partial snapshot as complete. These are finite client retention limits, not an upstream load/RSS guarantee. Large replies can fail at transport before projection. No fallback rereads database files or silently discards rows.

Pages contain at most 32 row fragments and 16 KiB of escaped row text. Oversized single rows continue at UTF-8 boundaries on following pages with exact zero-based byte ranges. All fragments concatenate to the retained escaped projection. An empty result still has one empty page. `--page` is one-based and must exist; invalid identity/page is rejected before owner discovery where possible.

## Presentation

Plain/non-TTY output prints only the explicitly selected page with snapshot counts, fragment ranges and a warning that another invocation fetches a new snapshot. It is not a stable cross-invocation pagination API.

Native Tern uses the pinned SDK and a separate read-only surface with Previous/Next/Close. PageUp/PageDown or Left/Right navigate; Home/End select first/last; Escape/Ctrl+D/Ctrl+C/Close leave the local viewer only. A 45vh scrolling region receives a new child identity per page, so returning to the first page does not retain the chat-tail offset. Up/Down line scrolling is qualified only for direct TTY, after the drawn-page ACK and with at most one outstanding raw scroll; blocked scroll keys are inert. The existing bounded input observer also rejects oversized unfinished paste before SDK accumulation. Mux uses unchanged SDK connection behavior, without this additional direct-TTY qualification.

No editor, submit, prompt-response or interrupt authority exists. Pointer page controls bind to the rendered page, so decoded old clicks cannot advance a different page. Raw keys only navigate local immutable data. Navigation remains usable without any subsequent gateway connection. The selected page is not a durable cross-process cursor.

## Verification plan

Executed: five gateway/four UI/one core/one CLI new Rust methods and seven history-PTY/three smoke Python methods. Tests cover exact retention boundaries, lossless Unicode/nested fields, all bidi controls, stale page actions, no live authority, real SDK page keys, no-ACK/unfinished-paste/queued terminal rejection and termios restoration. The four current native history cases contribute12pairs and assert exact initial/final/returned-page content plus scroll-container geometry. A supplemental narrow-dark Down-key probe measured a16px content movement without a page change; it is not an extra canonical test/case. Initial tail-follow capture failed,first174/190 aggregate is historical after fixes,and second178/191 canonical pass had a failed standalone helper fixture;all retained. The third full run passes after separating test-only positive startup from the500ms fault arm. Production deadlines and pins are unchanged.

The sequence used:

1. RED: CLI rejects missing/invalid identity/page and supports help without discovery; parser/page tests require lossless traversal of more than 256 rows, large Unicode rows, sidecars, raw/projected count mismatch, malformed/limit refusal and empty results.
2. GREEN: gateway snapshot/paging model, one-shot qualified RPC reader, native/plain viewer and CLI wiring without dependencies or lockfile changes.
3. Exercise the production binary with a loopback WS fixture and real PTY/SDK. Assert one history read and no session lifecycle/mutation requests; close restores terminal state. Page traversal, stale controls, EOF/no-ACK/error and plain output are separate assertions.
4. Run focused tests and `./verify.sh`, each package test/clippy, and native Tern rendering under private HOME/config/state/runtime/socket roots. Keep synthetic/native evidence separate from live Hermes/provider gates.
5. Update STATUS/ROADMAP/tasks/verification with measured results and remaining limits, preserve historical failed evidence and seal current bytes.

## Rulings

- PageUp/PageDown decode names are `page_up`/`page_down` in the pinned SDK. Real PTY regressions now cover both. Tern 0.9.5 control `key PageDown` did not advance the native page; the attempted `Page_Down` spelling was refused. Native qualification uses Left/Right and Home/End; PageUp/PageDown remain PTY-tested only, not proven native aliases. No activation retry is counted as acceptance.
- Source review `deleg_9615c8d4` arrived in full asynchronously after the initial handoff. It reports one bidi-control defect;all nine reported source hashes match the original candidate. The parent had already reproduced and fixed it in history escaping and shared display text,preserving normal RTL letters. Current core2/history5/UI-history4/PTY7 tests pass again. The requested reviewer file remains absent and the initial transcript was truncated,but the complete delivery resolves the earlier incomplete-report status. This is a complete bounded source review plus parent RED/GREEN,not independent post-fix approval.
- Local snapshot paging replaces the proposed server-paging implementation for this slice because the pinned server rejects pagination parameters. Cost: history beyond the fixed reply/retention limits remains unavailable here; full-history acceptance stays open.
- JSON-escaped projection is the inspectable source, not reconstructed chat prose. Cost: a denser technical viewer, but no silently dropped tool arguments, sidecars or unknown projected fields.
- No automatic resume, even for a stored session ID. Cost: the user must explicitly open the session elsewhere first; reading history cannot accidentally acquire live session policy/subscription effects.
