# Configuration and resource limits

Status: experimental connected CLI is implemented; fleet configuration below remains proposed, not a supported config file.

## Local developer installation

The owner authorized a personal installation on 2026-10-24. The exact verified 0.1.0 debug binary was copied without replacing an existing path to `~/.local/bin/hermes-tern` as a regular file with mode0755. This deliberately reuses the tested binary rather than implying an unverified release build. Its SHA-256 is `b75a09fce7cc248e2c1acdbdf3d7fe0cae03cf91b2eec3e54432a079dda77939`.

From the checkout root, `tern plugin install ./plugins/hermes-tern --json` installs a package copy at the directory reported by `tern plugin dir --json` (this Linux installation: `~/.config/tern/plugins/hermes-tern`). No `--force`, source link, personal settings rewrite or shell-profile edit was used. Binary and plugin copies do not update automatically when this checkout changes; future updates must reverify exact bytes and preserve any existing installation before replacement. The optional Hermes `hermes-tern-peer` package was not installed.

Both `hermes-tern --version` and the login-shell PATH check pass. `hermes-tern doctor` attaches to the already-running owner without starting/restarting Hermes, creating/resuming a session or calling a provider. `TERN_TSP=0 hermes-tern preview` checks plain output; the installed executable also passed isolated real-Tern light/dark offscreen text and image/layout checks. EGL driver warnings were retained; pixel inspection and the actual window launcher are not qualified by that render.

Tern reported no running daemon during installation, so the package applies at its next start. Open Tern and choose `Hermes: Open SDK preview (no agent)` in the command palette for the non-inference entry point. `Hermes: Open conversation` launches `hermes-tern chat`; sending a prompt is a separate inference action and was not exercised. The fixed launcher needs `hermes-tern` on the pane's login-shell PATH. `tern plugin list --json` currently reports the package ready with no problems; without a daemon this is a manifest/settings check, not proof the Luau window entry executed.

The authorization covered installation and non-inference checks only. No peer binding, OMP handoff environment, Hermes gateway restart, credential copy or paid test campaign was performed. This local installation is not a published release.

## Existing CLI

| Invocation | Effect |
| --- | --- |
| `hermes-tern` | Help; never auto-starts an actor |
| `hermes-tern --help` or `-h` | Help |
| `hermes-tern --version` or `-V` | Build version |
| `hermes-tern preview` | Static native/plain SDK output |
| `hermes-tern doctor` | Discover existing Hermes owner, verify identity/capabilities; no inference |
| `hermes-tern sessions [--stored]` | Explicit live/saved session listing |
| `hermes-tern history --session ID [--plain] [--page N]` | One bounded read of an already-live session's display projection, then local immutable page inspection; never resume/create/send |
| `hermes-tern chat [--plain] [--resume ID]` | Native Tern or plain conversation through existing gateway |
| `hermes-tern chat --plain --query TEXT --timeout 300` | One bounded turn; denies approvals and skips clarify; may incur provider cost |
| `hermes-tern serve --state DIR --workspace DIR` | Explicit foreground local mailbox supervisor; starts no agents |
| `hermes-tern migrate-store --state DIR --workspace DIR --archive-id UUID` | Offline v1-to-v2 upgrade with a verified pre-upgrade archive; refuses live ownership or stale archive |
| `hermes-tern coord --state DIR [--credential-file FILE] COMMAND` | Operator/peer IPC operations; credentials are read from private files, not inline CLI tokens |
| `hermes-tern coord --state DIR bind-peer --actor UUID --hermes-home HOME --physical-session ID --attest-binding` | Operator-only private binding for an existing explicitly verified physical Hermes session |
| `hermes-tern coord --state DIR inspect-staged-credential --staging-id UUID --actor UUID --incarnation UUID` | Operator-only read/recheck of one complete retained current credential; no secret output or sync |
| `hermes-tern coord --state DIR recover-credential --staging-id UUID --actor UUID --incarnation UUID` | Sync/publish the same current-authority file at its fixed name without replacement, rotation or registry writes |
| `hermes-tern coord --state DIR snapshot` | Operator-only consistent metadata/counts; bounded lists explicitly disclose truncation |
| `hermes-tern coord --state DIR state-index` | Operator-only complete-state revision, exact counts and persisted watermarks; no retained snapshot |
| `hermes-tern coord --state DIR state-page --workspace UUID --epoch UUID --revision HEX --section NAME [--offset 0] [--limit 64]` | Complete-record pages of one collection; count and byte bounds, stale state/epoch requires restarting the whole walk |
| `hermes-tern coord --state DIR archive --archive-id UUID` | Capture a private bounded SQLite snapshot; never prune source rows or restore authority |
| `hermes-tern coord --state DIR inspect-archive --archive-id UUID` | Read-only identity/hash/count verification; does not complete publication fsync or activate authority |
| `hermes-tern coord --state DIR inspect-staged-archive --staging-id UUID --archive-id UUID` | Read-only verification of one complete retained staging bundle; not published or durable |
| `hermes-tern coord --state DIR recover-archive --staging-id UUID --archive-id UUID --database-sha256 HEX` | Complete exact retained bundle publication; no overwrite/deletion/source restore |
| `hermes-tern coord --state DIR events --workspace UUID --epoch UUID --after N [--limit 64]` | Operator-only event notices, at most 128; cursor/epoch gaps require a snapshot |
| `hermes-tern coord --state DIR inspect-message --sender UUID --message UUID` | Exact sender-scoped persisted message and attributed data; does not deliver or ACK |
| `hermes-tern coord --state DIR dispose-message --sender UUID --message UUID --delivery-incarnation UUID --decision-id UUID --action close --reason TEXT --attest-outcome-unknown` | Immutable operator close; preserves original ambiguity and does not claim consumption/stopped worker |
| `hermes-tern coord --state DIR dispose-message --sender UUID --message UUID --delivery-incarnation UUID --decision-id UUID --action reoffer --successor-id UUID --ttl 300 --reason TEXT --attest-outcome-unknown` | Explicit new-ID successor with provenance, fresh TTL and current route/capacity checks |
| `hermes-tern coord --state DIR inspect-task --task UUID` | Exact persisted task ownership/result; does not grant or extend a lease |
| `hermes-tern coord --state DIR release-stopped --task UUID --owner UUID --incarnation UUID --generation N --attest-stopped` | Operator release of the exact inspected lease; prior writer must independently be known stopped |
| `hermes-tern peer-tool --binding FILE` | One bounded JSON invocation on stdin; narrow peer operations only |
| `hermes-tern omp-handoff exchange --state DIR` | One strict bounded JSON document on stdin; immutable offer, prepare, arm, record or inspect; no agent launch or send |
| Unknown command or extra arguments | Exit 2, diagnostic on stderr |
| SDK/terminal I/O failure | Nonzero exit |

Connected commands accept `--hermes-root`, `--python` and `--home`. Defaults locate `$HOME/.hermes/hermes-agent` and its `venv/bin/python`; `HERMES_TERN_ROOT` and `HERMES_TERN_PYTHON` override installer layout. Paths must be absolute. Chat accepts an existing absolute `--cwd`, and explicit `--model`/`--provider` for new sessions. Changing model/provider during resume is not implemented. No `run`, `attach`, `actor` command or config-file loader exists yet.

History `--page` is one-based (default1); a page outside the fetched snapshot fails. Non-TTY/`--plain` prints only that page; another invocation fetches a new snapshot, not a stable server cursor. Limits:4096projected rows,8MiB escaped retained text,32fragments/16KiB text per page,unchanged8MiB transport cap. Above-bound snapshots refuse,not silently truncate. Native Left/Right/Home/End navigate;Up/Down scroll is direct-TTY qualified. SDK PageUp/PageDown work in real PTY but native Tern0.9.5 PageDown forwarding is not established. See [history viewer](history-viewer.md).

`coord --help` lists registration, binding, routes, observation/replay, complete state pages, message send/read/ack, fenced task operations and shutdown. Omitting `--credential-file` selects operator access; peer calls require an explicitly provisioned credential file. Registration records an operator-attested binding, not runtime liveness. The local supervisor never launches Hermes/OMP, and peer callers cannot register actors, authorize routes or inspect operator snapshots/events/state pages. Observation responses are capped at 393216 encoded bytes; snapshots show at most 64 actors and 128 rows per other section. State pages enumerate all retained rows using limits 1–128, canonical decimal offsets and a matching revision/epoch. Large records can shorten a page before its row limit; follow `has_more` and `next_offset`. Each request scans the bounded store synchronously; there is no max-size latency or full-walk liveness guarantee under sustained writes. The optional Python package still exposes only six peer/task tools. [Peer tools](peer-tools.md) documents configuration and the still-open live-agent gate; [protocol](protocol.md#implemented-operator-observation-and-replay) defines overview cursors and [state pages](protocol.md#implemented-complete-operator-state-pages) defines complete enumeration.

Retained-credential commands take only canonical staging/actor/incarnation UUIDs, resolve fixed private names and return metadata, never secret content. They require an intact schema-1 envelope of at most 4096 bytes matching current SQLite authority; the operator key and running supervisor must already be usable. Recovery moves an existing entry without a new admission slot, so the 1024-entry directory cap does not prevent an exact move/retry. Both names present refuse; missing staging may reconcile the exact final file without proving staging provenance. No automatic scans, secret reconstruction, credential rotation or deletion. See [credential recovery](operations.md#recover-an-intact-retained-current-actor-credential).

Supervisor startup requires an intact private operator key for any existing database. New-key generation is only for an absent database with no entries besides `owner.lock`; complete key-first retry may additionally retain `operator.key`. Pending/unknown entries, sidecars and actor remnants refuse fresh initialization. No `--force`, automatic rotation or missing-key repair option exists. See [key policy](operator-key-policy.md) and [reconciliation guidance](operations.md#reconcile-an-existing-private-binding-or-operator-key).

The client refuses yolo resumes, sends `yolo:false` on create, never starts/restarts a gateway, and never copies credentials. Native recording is disabled. Query timeout is 1–3600 seconds; it requests interruption when the running generation is known and detaches, not a guarantee all tool effects stopped.

The unfenced `release-stopped --task UUID --attest-stopped` syntax is no longer accepted. The owner/incarnation/generation arguments are mandatory; no fallback looks up and releases the current lease on the caller's behalf. See [operator recovery](operations.md#release-only-the-inspected-task-lease).

Archive destinations are fixed beneath the selected state directory. Limits are eight retained entries, 256 MiB per database, 4096 bytes per manifest and 512 MiB retained file-length admission budget; new capture reserves the full database allowance. These are not configurable flags or an OS disk quota. Failed staging is retained and counted. Read-only image loading can temporarily hold two database-sized buffers (up to 512 MiB combined) plus SQLite/query overhead; no total-RSS or maximum-size latency claim follows. Capture/verification is synchronous, so other requests may wait or time out; reuse the exact archive ID for reconciliation. See [archive operations](operations.md#capture-and-verify-a-private-archive).

New stores use database schema v2; a v1 store requires the explicit offline command before `serve`. Migration does not launch a service, recover deliveries or change credentials. Disposition adds no peer tool or automatic retry. Reason is nonblank/control-free and at most 512 UTF-8 bytes; re-offer TTL is mandatory and 1–3600 seconds, while close refuses successor/TTL fields. Disposition rows are bounded by retained messages (default maximum 4096); decisions release only pending/debt accounting, never storage. See [recovery operations](operations.md#decide-an-ambiguous-message-explicitly).

## Existing upstream environment

The opt-in OMP palette bridge reads `HERMES_TERN_HANDOFF_BIN` (absolute trusted executable) and `HERMES_TERN_HANDOFF_STATE` (absolute dedicated private directory). Neither has a personal-profile default. No environment variable can bypass explicit pane selection and Send-once consent. The executable inherits the Tern process environment; this is not a subprocess sandbox. See [handoff protocol and limits](omp-handoff.md).

- `TERN_TSP=0`: force plain output.
- `TERN_TSP_RECORD`: SDK recording destination when enabled; potentially sensitive, not set by default.
- SDK terminal/multiplexer detection: use the pinned upstream behavior and handshake, not a project-specific imitation.
- `TMPDIR`: scratch root used by verification tooling; otherwise scripts choose a private user cache directory.
- `TERN_CONFIG_DIR`, `TERN_DAEMON_SOCKET`, HOME and XDG roots: the native smoke overrides these for isolation; ordinary preview does not modify personal configuration.

Rust toolchain and SDK pins live in [rust-toolchain.toml](../rust-toolchain.toml), [Cargo.toml](../Cargo.toml) and `Cargo.lock`. Do not silently select another toolchain in CI. Updating a pin requires rerunning all relevant gates.

## Proposed runtime configuration shape

Illustrative only; no parser ships for this example:

```toml
schema_version = 1

[workspace]
root = "/absolute/project/root"

[hermes]
mode = "attach"
profile = "selected-by-user"

[actors]
max_running = 4
max_pending_messages = 128
max_message_bytes = 32768

[storage]
max_journal_bytes = 268435456
max_attachment_bytes = 67108864

[routing]
default = "deny"
```

The future parser must reject unknown keys, relative escape paths, unbounded/zero-invalid limits and unsupported schema versions. Secrets do not belong in this file. Runtime endpoint/grant discovery comes from a qualified Hermes attachment procedure, not a copied token literal.

## Proposed guardrails, not performance claims

| Resource | Initial proposed bound | Behavior at limit |
| --- | --- | --- |
| Simultaneously running independent actors | 4 | Queue or refuse a new start; do not kill existing work |
| Pending accepted messages per actor | 128 | Reject before durable admission |
| Encoded peer payload | 32 KiB | Reject or require a controlled attachment |
| Encoded local protocol frame | 256 KiB | Reject before unbounded parse/allocation |
| Pending peer payload budget across a workspace | 16 MiB | Reject admission; do not rely on actor count alone |
| Coordination journal | 256 MiB | Compact terminal history, then stop new admission if still full |
| Shared attachments | 64 MiB | Refuse new attachments without deleting in-use references |
| Peer message TTL | 5 minutes | Expire before dispatch; preserve outcome metadata |
| Idle connect deadline | 10 seconds | Report connection failure; no arbitrary fallback runtime |
| Graceful stop observation deadline | 5 seconds | Escalation requires explicit policy; do not claim stopped |

The payload-only bound at four full actor queues is `4 × 128 × 32768 = 16777216` bytes (16 MiB). This excludes JSON overhead, journal entries, runtime/model context and render state; it is not a total RSS claim. A separate workspace budget protects against many registered but inactive actors.

Every numeric bound needs a boundary test and a measured operating envelope before release. Provider costs are unknown until the runtime reports them. Configure a user-approved inference budget separately; no default paid test campaign is authorized by these values.

## Isolation modes

`attach` means reuse an explicitly selected runtime owner. `dedicated` is a future separately provisioned runtime home, never a clone of a live DB. `sandboxed` must not be exposed until a backend enforces and demonstrates its boundary. The default local process model does not constrain all filesystem/network effects.

No runtime configuration is enabled by the preview, so changing this document changes no enforcement. See [security](security.md) for the controls and [roadmap](ROADMAP.md) for ownership.
