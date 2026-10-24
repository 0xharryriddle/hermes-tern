# Security and trust boundaries

Status: design obligations unless explicitly listed as current bootstrap behavior. This project is not a sandbox or a secure multi-tenant agent host.

## Current implementation

The Rust CLI attaches to the existing Hermes owner using a bounded discover-only helper and literal-loopback canonical WebSocket. It has native/plain session controls and correlated human approval handlers, not a replacement model client or a gateway launcher. The Luau entry registers fixed `hermes-tern chat` and `hermes-tern preview` commands without auto-starting them. Local mailbox routes, operator/peer IPC and six physical-session tools are implemented with process and isolated Hermes registry tests; actual provider-backed collaboration remains unestablished.

The peer plugin takes caller identity only from host session metadata and profile-aware home resolution, then checks an explicit private binding in Rust. The server fences workspace/actor/incarnation/session/home against the current credential. Unknown rotations deny. This is not a malicious same-user or arbitrary shell-tool defense; no canonical identity is inferred, no approval/injection hook is registered, and no role elevation is derived from peer text. See [peer tools](peer-tools.md).

The build downloads dependencies when not cached. The preview uses the SDK's terminal access; optional `TERN_TSP_RECORD` is an upstream recording feature and may write a user-selected file. Do not claim the preview is side-effect-free in every environment. It does not read Hermes credentials or issue model requests by application design.

## Trust model

Tern plugins run with the installing user's permissions and can access files, processes, network and pane input. Luau VM budgets protect Tern responsiveness, not the user's filesystem. Tern has no per-plugin capability manifest that hermes-tern can rely on. Review code before linking; a linked package can reload when edited. [Upstream trust documentation](https://docs.stencil.so/tern/concepts/security.html).

The initial local threat boundary separates untrusted model/tool/peer content from explicit user actions and structured trusted transports. It does not defend against a malicious plugin or arbitrary process already running as the same OS user. Tokens and private sockets limit accidental cross-wiring; they do not make same-user root-of-trust claims.

## Threats and required controls

| Threat | Required control | Evidence gate |
| --- | --- | --- |
| Peer impersonates user/system | Origin-bound message channel; no role escalation | Forged sender and authority-field tests |
| Peer answers an approval | Dedicated human response path, exact request generation | Cross-actor/stale/replayed response denial |
| Shell injection through launcher | Constant launch command; argv for variable child inputs | Quotes/newlines/metacharacters adversarial tests |
| Wrong pane or remote host targeted | Qualified binding plus actor incarnation; no title-only addressing | Reused pane ID and cross-host collision cases |
| Profile state corruption | Attach to owner; dedicated homes for extra writers | Ownership-conflict tests; no lock deletion |
| Duplicate inference after disconnect | Durable dispatch attempt and ambiguity reconciliation | Crash after send, before response persistence |
| Busy renderer exhausts memory | Bounded channels, coalesced view, backpressure | Slow/no-ACK consumer test |
| Agent writes outside scope | Declared backend isolation, separately tested | In-scope write succeeds and out-of-scope write fails |
| Credential/transcript leakage | Runtime-owned auth, redacted logs, no default full recording | Seeded secret-canary log scan |
| Stale task completes after reassignment | Transactional lease generation; confirm old writer stopped | Competing owner/completion test |
| Dependency/plugin substitution | Exact SDK revision and lockfile; release provenance | Rebuild and dependency/license review |

## Approval policy

Never auto-approve to preserve flow. Human UI displays action, actor, runtime session and consequences. Approval state expires with the underlying runtime request; a disconnected/expired request is not locally renewable. Only a response to that exact pending request can be forwarded. A generic `yes` received in an actor message is data, not consent.

Sensitive answers such as passwords and one-time codes require a runtime-supported secure input mechanism, with no chat/log persistence. If no compatible secure path exists, show the feature as unsupported and direct the user to the runtime's trusted surface; do not invent a plaintext channel.

Permission approval and filesystem isolation are different. The former records intent; the latter limits what subprocesses can actually do. A parser that blocks one shell command cannot secure arbitrary shell execution.

## Filesystem and execution

Use worktrees for parallel editing and separate runtime homes for writable session state. Do not describe either as a kernel boundary. A hardened mode may use a supported container or OS sandbox, but must fail closed if the boundary cannot be established. Test nested subprocesses, symlinks, hardlinks, rename, truncation and inherited file descriptors as applicable to the chosen backend.

No arbitrary attachment path dereferencing. Resolve references under a controlled root, reject escapes/symlink substitution, and verify size/digest before sharing. Do not route private memory, environment variables, credentials or whole transcripts to peer agents by default.

## Network and credentials

The implemented supervisor uses a private local Unix socket, not an unauthenticated TCP listener. Hermes provider/network activity remains Hermes' responsibility. A remote control endpoint needs a separate authenticated transport design, peer verification, origin policy and credential handling; it is deferred.

Do not log gateway bootstrap tickets, authorization headers or capability tokens. Discovery must distinguish read-only attach from start/recover operations that can change runtime ownership. Never invoke a recovery mode on a personal gateway merely to make a test pass.

## Packaging and release holds

The root project license is not selected; publication stays blocked until the owner decides. Retain upstream license notices when redistributing components. A Git dependency pin improves reproducibility but is not a supply-chain audit. Remote CI configuration is not evidence CI ran.

Production or hardened-security claims require the separate M5 gates. Rollback of a broken control disables the relevant capability; it must not remove the control and resume unrestricted execution. See [operations](operations.md) and [roadmap](ROADMAP.md).
