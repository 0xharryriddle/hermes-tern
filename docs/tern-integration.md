# Tern integration

Status: Rust preview implemented and native render smoke executed; interactive sessions and the plugin window action remain separate acceptance gates.

## Verified upstream surfaces

The pinned SDK is `tern-sdk` 0.1.0 at Git revision `9e9c07463f4958b0c633424c9860aba499487d28`. Its Rust crate provides wire/input parsing, typed UI builders, reconciliation, synchronous sessions, print/ask helpers and plain rendering. Installable Tern plugin entries are Luau, running in host/window VMs. [Sources](sources.json) records the inspected revisions.

| Surface | Role | This project |
| --- | --- | --- |
| Rust `tern_sdk::print` | One native flow output with fallback | Used by `preview` |
| Rust `Session`, `Input`, `SurfaceOptions` | Interactive surface lifecycle | Connected UI implemented; PTY and real-Tern synthetic controls pass |
| `Session::with_terminal` | Scripted/custom transport tests | Used in deterministic SDK lifecycle controls |
| Luau `tern.command` + `cx.layout:new_tab` | Explicit palette launch | Package present; actual window action needs testing |
| Luau `cx.agents` | Native agent-pane operations, especially OMP | Local handoff bridge implemented/callback-tested; actual-window OMP acceptance open |

The Rust crate is linked into our executable; Tern does not load it as a plugin dylib.

## Current bootstrap

`hermes-tern preview` calls `tern_sdk::print` with a static view. The visible text explicitly says `Preview only` and `No agent is connected`. It prints no fabricated conversation, progress or cost. Native print leaves a retained flow surface after the process exits; this is not a running actor.

The Luau package registers chat, preview and `plugin.hermes-tern.omp_handoff`. Loading only registers commands. Preview/chat request a new tab with a fixed command; their executable must be on the target shell's PATH. The opt-in [handoff](omp-handoff.md) reads explicit absolute binary/state paths only when invoked, then inspects local panes and asks for human confirmation. There is no startup hook, profile editing, process polling, default key binding or credential access.

Tern interprets `launch.command` through a login shell. Keep this command constant. Do not concatenate prompts, file paths, pane titles or model output into it. Future argv-aware execution belongs in the Rust process boundary or a qualified Tern API, not string interpolation.

## Interactive TSP contract

The connected client uses the SDK handshake rather than treating `TERM_PROGRAM` as proof. Respect the terminal's kinds/features/credits and fall back or refuse cleanly when necessary. Honor `TERN_TSP=0` as an explicit opt-out.

The event loop must pump acknowledgements even while backend work is busy. Bound model-output buffering while windows are detached: no drawing window means no ACKs. Persist bounded authoritative events independently and rebuild the view later; do not convert a stalled renderer into unbounded memory growth.

Keep stable node keys for messages, tools and approval requests. Handle `gone` retention events, theme/resize, input close and errors. The SDK's `Waker` can yield `Ok(None)`; that is not EOF. Restore terminal state on normal exit, signal and error. Do not reimplement SDK escape framing or raw-terminal cleanup without evidence the upstream path is inadequate.

Program text and CSS are scoped by TSP's allowed vocabulary. Native surfaces are not a webview. Use `md`, `code`, `diff`, `tool`, `agent`, `checklist`, `editor` and other negotiated kinds instead of drawing terminal cells by hand.

## Native OMP agent API

Pinned `plugins/tern.d.luau` defines AgentCx `list/get/start/ask/wait/transcript/interrupt/stop`. `start` retains a prompt until the composer is sendable; the pinned `ask` declaration describes atomic TSP send for native OMP. Structured transcript extraction reads `omp.session` and returns no structured history for non-OMP agents.

The online window guide describes `ask` as paste plus Enter, while pinned declarations describe native TSP send. This is version-sensitive evidence, not permission to assume either transport in an installed Tern. Export the installed declarations and exercise readiness, cancellation and unsupported-native cases before enabling the lane. Do not interpret `wait` resolving idle as transactional message-consumption acknowledgement.

AgentsCx is window-scoped. The local Rust exchange and Luau handoff now exist with callback/fault tests, not actual-window acceptance. A durable arm prevents another local grant, but no `ask` receipt or conditional expected-composer parameter exists. Snapshot checks reject observed bad state and main-only retained views; they cannot enforce atomic liveness or durable OMP identity. The consent UI warns that a pane-level ask may queue. No routing, tool-approval interception, sandbox or headless agent API is implied.

## Pane identity and remote hosts

Pass Tern pane IDs back unchanged in their original API context. Remote IDs can have different representations across windows, host APIs and CLI scripting. Persist our own actor IDs and qualified bindings; never identify a target by title alone.

The first supported deployment is local Linux desktop. A window half runs locally even when panes are remote. Do not use local filesystem checks to validate remote pane paths or assume a local binary exists on a remote host. Web clients do not run window Luau; mobile/remote launch parity is deferred.

## Evidence and acceptance

The [native smoke script](../scripts/smoke_native.py) runs the compiled preview in Tern's offscreen renderer, waits for its native surface text, and requires light/dark image-layout pairs with visible surface geometry. This is not a screenshot generated from a mock UI.

Separate release evidence is still required for: loading the actual Luau entry in an isolated desktop window, invoking its palette command, verifying the launched executable and error path, terminal restoration, interactive event routing, and a live Hermes session. The bootstrap's isolated Xvfb window probe reached the control endpoint but timed out waiting for preview text; its initial state showed an unsigned-in Tern access gate. This is a blocked/not-established launcher result, not proof of a plugin defect or a successful launch. A plugin catalog marked `ready` without a daemon may mean only that the manifest parsed. See [verification](verification.md).
