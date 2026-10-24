# Gateway attachment qualification

Status: live discover-only connection verified; session mutation and inference not exercised by this qualification. Discover-only means no runtime owner is started, not an absolute filesystem-read-only guarantee.

## Observed contract

Source baseline: `bf0203a794a07aac3ff764972ab9087baa4e4f4e`. Installed launcher metadata was read programmatically using `hermes --print-runtime-command`; do not copy display-redacted interpreter paths into commands.

A task-owned helper called the existing bootstrap with start=false and recover=false. It obtained a local connection grant without printing the ticket, connected over WS with the exact canonical subprotocol, and ran only ping and runtime.describe. No sessions were created; no model calls or gateway restarts were requested.

Observed results:

```json
{"method":"ping","result":{"pong":true}}
```

The accepted subprotocol was `hermes-gateway-v1`. The ready event arrived as the `event` envelope with `params.type=gateway.ready`. Runtime capability names: durable-admission-v1, event-replay-v1, local-cli-create-v1, acp-editor-policy-v1, acp-session-mcp-v1. The described create sources included tui; params included source, request_id, cwd, model, provider, toolsets, skills, checkpoints and additional fields.

## Source-qualified wire details

At the inspected revision, bootstrap tickets are single-use with a 30-second TTL. Mint a fresh grant on every connection; never persist or reuse it. Native ticketed WS requires a loopback peer and no Origin header. Keep the exact subprotocol echo and no-proxy check.

Advertise `client.capabilities` with `server_requests:true` only after the client can answer or explicitly decline server requests. `session.info` provides structured state; `session.status` is legacy-only and is not an available canonical session method. `session.history` returns projected messages, and its source-row count need not equal the displayed row count.

The discovery fallback can tighten permissions of an existing same-owner gateway lock to 0600. Resume can refresh profile policy/hooks through the owner. Neither behavior authorizes another writer or a runtime restart, but both invalidate an unconditional claim that attaching never changes any file. The checkout revision also does not prove the running gateway was booted from that revision.

## Qualification boundary

This proves the installed private-bootstrap/canonical-WS route works at the observed checkpoint. It does not turn an internal helper into a stable external API, establish future version compatibility, prove all mutation schemas or authorize starting/restarting owners. The integration must contain this dependency in one helper and fail closed on unsupported capability/auth/ownership.

The captured capability response contained runtime-owned instance/profile identities; public notes omit those values and all grants. Source contracts and implementation are in [the first connected-slice plan](implementation-plan.md).
