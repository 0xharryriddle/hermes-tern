# Native user experience

Status: native/plain connected client is implemented and locally tested; provider/window acceptance remains open. The layout below is the full UX target unless a section explicitly says implemented. Static preview remains labelled as disconnected.

## Current slice: bounded long transcript

Implemented and locally verified; review parent-reconciled and complete post-remediation autorun passes: enforce the existing 256-row/64-KiB-per-row/2-MiB-total display budget after stream and tool replacements as well as insertion. Preserve oldest-to-newest eviction and the retention notice. Per-row truncation prevents joining later chunks across a missing prefix; an evicted active stream stays absent until a complete replacement. Retained strings copy only their prefix rather than retaining the discarded allocation. These are display budgets, not total RSS, maximum-load or upstream archival guarantees.

Native rows longer than 1024 UTF-8 bytes or twelve lines default to a bounded preview with explicit Show retained text/Collapse controls. At most one row opens, keyed by message ID and a local display incarnation; unknown/evicted/replaced rows refuse stale actions and navigation never dispatches an RPC or changes composer/prompt authority. Node keys and already-decoded actions carry that incarnation. Changed retained text or snapshot hydration refolds the row; ordinary repaints and unchanged text keep controls stable. Expanded text remains bounded to what the client retained and can be inspected while disconnected. Row truncation and local folding have different notices. Short rows, approval controls and dock layout stay unchanged; no new animation or global shortcuts. Tern owns scrolling; keyboard-only folding and motion-mode qualification are established separately below, not by pointer tests.

Eight new gateway retention/receipt/generation methods, four UI methods and three production-PTY methods pass; gateway/UI packages pass 34/16 tests with two live opt-ins ignored. Review reproduced missing-completion receipt fallback and stale same-ID Show under credit backpressure. Parent also reproduced receipt/info generation advancement retaining an earlier stream and late partial frames corrupting a recovered receipt result. These observed RED cases are repaired; the review is parent-reconciled, not independent post-fix approval. Two new smoke-contract methods and twelve runner methods pass. Actual Tern's long-output case passes wide/narrow light/dark with three captures each: folded, expanded, refolded, then confirmed Detach; no prompt/approval/cancel RPC. Two narrow captures inspected without clipping/overlap. The first run timed out because the harness assumed raw tree text: Tern 0.9.5 collapses whitespace and caps observed text at 200 characters. The fixture now exceeds twelve lines while its normalized text stays below that cap; exact-tail negative controls remain, and raw larger-content checks use PTY. Failed/probe generations remain retained. The pre-remediation aggregate passed 150 Rust/170 Python, 24 command gates and 24 native cases/44 pairs; it is historical after the review fixes. The complete post-remediation aggregate passes 155 Rust/171 Python, two live ignored, all24 command gates, nine standalone package pairs and24 native cases/44 pairs. Four current long-output images inspected across both widths/themes without observed clipping/overlap; not all44 images reviewed. No live/provider/window/full-history acceptance follows.

Receipt fallback applies only to this viewer's current-generation admission when no completion event was observed; it replaces partial/clipped/evicted retained text under the same budget. An observed live completion (including response reuse) is not overwritten by the stored representation. Late start/delta/interim frames cannot reopen or append to a durably settled result; a late completion may still replace it. The pinned canonical Hermes claim increases execution generation for each admission; client event, receipt and info paths all reset stream/segment state on advancement. This is a bounded display projection, not full canonical-history reconciliation.

## Current slice: keyboard retained-text inspection

Locally verified: Tab/Shift+Tab cycles only foldable retained rows and the composer, in transcript order; Enter/Space toggles the selected current row. Escape returns from inspection to the composer, while Ctrl+D remains explicit detach and Ctrl+C remains current-turn interruption. Other keys in inspection do not edit or submit the draft. A removed/replaced selected row enters an inert stale-selection state until explicit Tab/Escape navigation; Enter must never fall through to prompt submission. Selection binds the existing local row incarnation, is visibly marked and requests Tern reveal, not a fake button focus op. Only the editor owns TSP caret/IME focus.

Scope: `crates/ui/src/transcript.rs`, `view.rs`, `app.rs`, `native_visibility.rs`, `native_input.rs` and focused Rust/production-PTY tests; `scripts/smoke_native_chat.py`, `scripts/verify_runtime.py` and their contract tests for required keyboard/motion cases. Preserve short-row behavior, pointer identity fences, draft, pins and production deadlines. The initial pre-implementation real-Tern diagnostic confirmed Tab alone did not leave the composer. The implementation now observes bounded frame/ACK/key metadata through the SDK's public Terminal seam on direct TTY; the pinned SDK still owns handshake, input parsing and rendering. Mux transports retain their existing path and refuse the new keyboard inspection. Source review was parent-remediated with RED/GREEN, not independently reapproved. Activation requires the current positioning frame ACK, distinct from a rendered-view ACK. A dashed selected outline is pending; only the exact layout-neutral ready-ring class turns it solid after that ACK, and native scenarios wait for it without key retries or sleeps; gateway layout updates and Resize/Theme/Motion invalidate the revision and trigger bounded re-reveal. The input observer retains at most sixteen SDK-parser/decoder candidates at idle boundaries and intersects visibility after matching SDK-returned keys, avoiding a second flush clock. It refuses excess ambiguity, more than8192queued keys or128KiB undecided input per candidate; none of these limits authorize an upstream action. No new approval/Send/Interrupt button keyboard activation or whole-control accessibility is claimed. Native tests must prove keyboard-only fold/unfold, exact retained text, draft, no mutating RPC and exit, plus isolated Reduce Motion On/Off readback and selected image inspection. The control-tree `.sf-still` class is observed mode evidence, not a broad animation/accessibility certification.

Historical keyboard autorun:167Rust/181Python,2liveignored,0ResourceWarning,9package pairs,24command gates and32actual Tern synthetic cases/68image-layout pairs PASS. The8keyboard/motion cases contribute24pairs;four current keyboard images were inspected across widths/themes/modes, not all68. No prompt/approval/clarification/interrupt RPC occurs in the local inspection cases; explicit pointer Detach plus receipt/client-exit sentinel verifies exit. Twelve new Rust methods and ten Python methods cover this slice. Review3findings were reproduced and parent-remediated, not independently approved after fixes. Failed mode-projection and early-key generations remain retained. Full-history paging,maximum load,OS transport fault coverage,mux keyboard,whole-control accessibility and live/provider/window acceptance remain open.

## Current slice: immutable history projection

The separate [history viewer](history-viewer.md) is implemented and locally/native-synthetic verified. A qualified one-shot `session.history` read closes before browsing; the surface has no editor or live session controls. A fixed snapshot keeps all projected JSON fields within explicit bounds and labels source/projected counts independently. UTF-8-safe fragments make long rows inspectable over multiple local pages without granting durable row authority. A page-keyed45vh scroll region prevents chat-tail follow on page changes. Left/Right/Home/End are native-qualified;Up/Down has a supplemental native scroll witness and bounded direct-TTY ACK gate;PageUp/PageDown are PTY-only,not proven native forwarding.

Current complete autorun178Rust/191Python,2liveignored,0ResourceWarning,24gates,9package pairs,36native cases/80pairs PASS. This is partial visual acceptance: the revised captures were not pixel-inspected after HTTP402. The delayed complete bounded source review is now reconciled:all9original hashes match,and its only finding is the bidi defect already reproduced RED then fixed GREEN. The parent-discovered page-key mismatch also has RED/GREEN. The missing reviewer file/truncated initial transcript remain historical provenance,not a missing delivered verdict. No raw/full history,server paging,complete independent approval,whole-control accessibility or live/provider/window claim follows.

## Information hierarchy

The primary screen is a working conversation, not a dashboard of decorative cards. Use a compact transcript, one composer, a thin session/status line and contextual tool/approval details. A peer roster is secondary and opens only when useful. Avoid gradients, heavy shadows, icon-card grids and ornamental animation.

Proposed regions:

```text
main   conversation, tools, diffs, bounded peer messages
 dock  composer + current actor/profile/model + pending-work status
layer  approval, clarification, session picker and capability details
```

Tern owns layout, font metrics, scroll, native motion and theme. Use semantic SDK nodes; never measure strings or implement spinner timers just to mimic terminal cells. Theme and reduce-motion changes are runtime events, not startup-only settings.

## Session identity

Always make the active actor, runtime, profile and effective model inspectable. A model/provider switch displays the confirmed effective value, not the requested one until the runtime accepts it. A disconnected session keeps its transcript but disables stale action controls.

Use state words with precise meaning: connecting, ready, running, waiting for input, cancelling, disconnected, failed. Do not equate a green process dot with completed work. A child row is labelled as delegated by its parent, distinct from an independent peer.

## Composer and commands

One submission path handles keyboard submit and native TSP `send`. Advertise send support only after it is implemented, and set sendable only when that actor/session can accept a new prompt. A focused editor is not necessarily ready. Preserve unsent draft text across view updates and do not drop it on a temporary backend disconnect.

Commands are resolved through a verified Hermes command surface. Offer supported commands and explain unavailable ones; do not hardcode a cloned slash-command catalogue that drifts. Destructive rewind/reset/profile operations require explicit confirmation and current runtime identifiers.

## Streaming and tool work

Render message deltas incrementally with stable keys. Tool rows are correlated by tool-call ID, not tool name. Long outputs are folded/paged with a clear truncation marker and an inspectable retained reference. Hidden reasoning is not a cross-agent payload. If reasoning display is supported by the provider/runtime, respect the user's existing policy.

Use `tool`, `diff`, `code` and `checklist` nodes for the corresponding data. Show usage/cost only when the runtime provides it, with unavailable/estimated labels otherwise. Do not derive cost from animation duration.

## Human-only interactions

Approval UI must show actor/session, action, scope and exactly what the choice authorizes. Pending requests are keyed by request ID and generation; a stale dialog cannot answer a new request that reused a node. Escape/cancel/timeout never approves. Clear sensitive input after use and keep it out of normal transcript events.

Peer-message UI shows sender, recipient, purpose and delivery state. A queued badge is not a read receipt. Ambiguous delivery is a first-class warning with reconciliation actions; it must not offer automatic resend as the primary action.

## Keyboard and accessibility

Keep all operations reachable without a pointer, visible focus, clear labels and logical tab order. Do not reserve global Tern shortcuts by default. Use Tern's native accessibility representation and test keyboard/native-send equivalence. Small panes should prioritize the composer and active work, not squeeze all peer metadata into one line.

## Required visual evidence

For connected UI changes, inspect actual Tern renders at narrow and wide widths, light/dark themes and reduced motion. Validate pending approval, long tool output, empty/error/disconnected states and concurrent same-name tools. Record source revision plus screenshot identity and inspect pixels; a layout JSON or render exit code alone is not visual approval.

The original bootstrap pixel review was unavailable; later selected native fixture/chat captures have been inspected separately. No review of every image, full accessibility or live-provider quality follows. [Current status](STATUS.md) records each evidence generation.
