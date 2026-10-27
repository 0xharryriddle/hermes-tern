# Configuration and resource limits

Status: experimental connected CLI is implemented; fleet configuration below remains proposed, not a supported config file.

## Local developer installation

The owner explicitly approved the latest personal TUI binary update on 2026-10-27. The installed0.1.0 debug executable is `~/.local/bin/hermes-tern`,regular mode0755,SHA-256 `5d1cc97f913856fffd0527dd4c4bfb8e4652bb26244d659dc3adaf310fd68bdb`. It is byte-identical to the verified build at source checkpoint `18b9302`:actual CI429Rust/386Python/two live ignored, and the same runtime's80native synthetic cases/212image-layout pairs. The previous binary SHA-256 `6adffee3a3a7f4c3548922b97edcf319c55b65f3300e011d17ec15e88aab2ef6` is preserved at `~/.cache/hermes-tern/update-e64d52cd89e0/backup/hermes-tern`. Earlier manual binary and plugin backups remain under `~/.cache/hermes-tern/install-2b2452721edb/backup`. No unverified release build was substituted.

`tern plugin dir --json` reports `~/.config/tern/plugins`;the installed copy is `hermes-tern` inside it. After exact source/old-file/backup checks,the supported `tern plugin install /home/harry-riddle/.cache/hermes-tern/install-2b2452721edb/candidate/plugin --force --json` replaced only that manual package. No source link,settings rewrite or shell-profile edit was used. Tern0.9.5 reports one ready Hermes for Tern0.1.0 entry and no problems;both plugin files match current checkout bytes. The standard installer reloads Tern plugins;no Hermes gateway restart or optional Hermes peer-plugin installation was performed. These copies do not auto-update when the checkout changes.

Login-shell PATH,`hermes-tern --version` and `hermes-tern open --help` pass. A pinned Luau interpreter executed the exact installed window entry with bounded callbacks:registration launches nothing,chat requests exactly `hermes-tern open`,preview requests exactly `hermes-tern preview`,and missing-pane return triggers the error callback. The installed executable also passed isolated real-Tern light/dark1100x760 preview text/layout/image checks(two pairs),with no personal plugin folder or Hermes session/provider access. This is not a real-window palette action,pixel review or live conversation acceptance. A preexisting Tern daemon was reachable at update;its sessions/titles were not retained.

The current binary-only update left both Tern plugin files unchanged. Four production-PTY methods executed the installed executable with synthetic owners:reasoning resume/Show-Hide,stream whitespace/expansion,tool-separated fallback andboth F4/pointer command opening pass. The earlier Luau/preview results above remain historical to the unchanged plugin and previous binary. No real session was created/resumed,no prompt/provider/config/DB change orHermes restart during the current update. Open a new Hermes tab from the existing palette to use it;already-running tabs do not change executable in place.

In Tern's command palette choose `Hermes: Open conversation` for the native session picker,or `Hermes: Open SDK preview (no agent)` for the static preview. From a direct Tern shell,`hermes-tern open` selects the same picker. New/Resume and prompt submission remain separate human actions;the existing qualified Hermes owner is required and never auto-started. If an already-open window still shows its prior palette,open a new Tern window;do not restart Hermes merely to refresh Tern's entry. No live session was created/resumed or prompt sent during installation.

The original2026-10-24 installation used binarySHA-256 `b75a09fce7cc248e2c1acdbdf3d7fe0cae03cf91b2eec3e54432a079dda77939`,no-force package copy and fixed `hermes-tern chat`;its PATH/plain/native-preview/no-inference-doctor evidence remains historical. The current install/update and non-inference checks do not select a license,publish a release,install the Hermes peer plugin,configure OMP handoff,copy credentials or authorize paid/live tests.

## Native session entry from this checkout

Run `cargo build --locked -p hermes-tern`,then `./target/debug/hermes-tern open` inside a direct Tern shell. `open --cwd "$PWD"` explicitly selects the new conversation workspace. The displayed path is data passed through typed session-create parameters,not a shell interpolation. New/Resume requires a human click or acknowledged keyboard selection;Close performs no session create/resume/submit. Existing `chat` still opens directly and `hermes-tern` without arguments still shows help. The upstream `hermes` command is never redirected.

The source palette command runs fixed `hermes-tern open`;the owner-authorized2026-10-27 personal update above now includes this entry and matching executable. Later source changes still require a separately verified authorized update. `open` refuses non-TTY,explicit TSP disable,multiplexers or failed native negotiation rather than opening a plain session implicitly;`chat --plain` remains explicit. The existing Hermes owner is required and is never started/restarted.

Inventory:one read each for live/stored,at most200rows per scope,12rows per local page. A cap warning means more may exist,not a server total. Titles/model/source are bounded escaped labels;only exact session_id selects Resume. The fixed listing is not auto-refreshed;reopen to fetch again. Keyboard Up/Down selects,Tab switches scope,Left/Right pages,Enter activates only after current positioning ACK,Escape/Ctrl+D closes. New-session model/provider can be supplied through `chat --model/--provider`;existing native conversations have explicit Session model/F3. Peer/OMP binding is never automatic.

Profiles explicitly requests owner-reported `profiles.list {include_sessions:false}`. At most64profiles,12rows per page;names64bytes,exact home and display labels256bytes before escaping. Relative/noncanonical/duplicate identities or a missing current owner refuse the inventory rather than showing fabricated empty success. Choose a listed existing profile by pointer or positioning-ACK-gated Enter;Left/Right pages,Escape/Ctrl+C returns,Ctrl+D closes. Selecting another home closes the old socket and discovers a fresh grant with start/recover disabled;the exact canonical home must equal grant.profile_id before any new socket. The selected owner is requalified and displayed before separate New/Resume. Missing owner is a refusal,not an automatic launch. This never creates/clones/migrates a profile,changes the sticky default,copies credentials or edits configuration. Real sibling-owner/provider qualification remains separate from the two-synthetic-owner tests.

Runtime details/F2 inspects qualified owner/profile/session and allowlisted inventory locally. Session model/F3 requests configured `model.options` on demand (`explicit_only:true,include_unconfigured:false,refresh:false`),then Select/Apply and optional human Confirm issue revision/generation-fenced session.mutate. No profile config is written. Inventory may probe the current custom endpoint or warm catalogs;not an inference/health test. Limits64providers/4096models,12rows/page,256byte provider/1024byte model identities. Auth-unreported/unauthenticated/limited entries are disabled. Owner info then complete snapshot supplies effective route/fences;unknown disables Send and changes until reconnect/resume. Left/Right pages;Escape/Ctrl+C returns unchanged draft. See [model UX](ux.md#current-slice-session-model-controls). Historical local/native synthetic full200Rust/222Python/56casesPASS;live/provider/window remains open. The current matched personal binary includes these controls;installed real-provider qualification remains separate.

Model review corrections are focused-verified:dedicated postcommit snapshot with receipt revision/generation floors;owner-resolved guard tuple shown before Confirm while preserving exact catalog request identity;unsupported deferred guard remains unknown/pending and cannot re-enable Send. Earlier200/222native and200/235local totals are historical; these repairs are included in commands-autorun-3 (220Rust/276Python,64native cases/148pairs). Live qualification remains open.

Commands/F4 explicitly loads at most4096owner entries/twelve perlocal page. Ten fixed built-in empty-argument reads run handlers/pre-command hooks in Hermes. Exact built-in /tools adds a separate `tools.show {session_id}` catalog read,not command.dispatch or tool execution. Catalog128groups/4096unique tools,names128/256bytes,descriptions1024bytes,complete output64KiB;integer total must equal projected unique rows. Missing/malformed/oversized is unavailable,not zero. Catalog entries grant no health or permission. Arbitrary aliases/plugin commands,toolset/config changes andMCP reload remain unsupported. Exact owner/session/revision/generation/catalog/panel-opening/visible-page fences apply. Control-safe output uses local4096byte chunks;completed output precedes catalog rows,butlarge chunks/lower rows still scroll. `send`/`skill` never becomes a prompt. Pending/unknown withholdSend;unknown is not retryable,unsent queue refusal permits explicit reload. Model pending/deferred mustsettlefirst. Escape/Ctrl+C returns the unchanged draft;Ctrl+D detaches. See [command UX](ux.md#current-slice-explicit-owner-commands). Tool-catalog focused 21 Rust/46 related Python/four native cases/16 pairs PASS; full campaign 233 Rust/293 Python/two ignored/24 gates/nine package pairs/64 native cases/152 pairs PASS. Source review reconciled, not independent final-source approval. No personalcopy update orlive-owner qualification.

## Owner plugin inventory

Commands/F4 exposes exact builtin /plugins through one explicit `plugins.list {}`. Before the pointer read,the panel names the qualified owner/profile and discloses profile-wide initial discovery/load/registration/secret-source refresh. No local plugin import,management,install/toggle,configuration write or automatic refresh. Canonical plugins/toolsets/portable-MCP arrays cap128each;plugin key/name/version256UTF8bytes,toolsetkey128/label256/description1024,strictbool and unsigned registration counts<=4096,rawerror<=16384discarded for a fixed marker,complete escaped output<=64KiB. Duplicate/unsafe/malformed/missing/oversized reports refuse rather than truncate or fabricate zero. Existing4096byte localchunks,current runtime/catalog/panel/page fences,unchanged draft and pending/Unknown/no-retry guards apply. Local/native synthetic verified,not actual-owner plugin,health,permission or installed-copy acceptance. [Operations](operations.md#owner-plugin-inventory-from-this-checkout) owns the procedure.

## Native MCP reload

Runtime details → Review MCP reload is a local inspection step. A separate Confirm profile reload and reattach consents to every-chat profile MCP reconciliation, possible server-process/network effects and a same-connection reattach that may refresh policy/adoption/hooks. Exactly one minimal reload request per consent, followed by a dedicated postresult snapshot; no configuration write, automatic retry, passive-readback or atomic MCP-revision claim. Eight admissions per connection. Unknown survives panel close/reopen and keeps Send and sibling mutations withheld; busy/unavailable does not imply rollback. This checkout control is local/native synthetic verified, not installed-copy or actual-owner MCP/provider/window acceptance. [Operations](operations.md#native-mcp-reload-from-this-checkout) owns the procedure; Commands/F4 itself still does not dispatch MCP reload.

## Owner-reported task plan

Task-plan display is local under Runtime details → Show task plan. Only existing snapshot.todo_state and sequenced todo.updated are consumed; no polling RPC, todo-tool invocation, state write, provider call or prompt. Cap256tasks,256UTF8bytes/exactID,4000Unicodecharacters/16000UTF8bytes content,64KiB complete escaped display. Four exact reported statuses, strict unsigned task revision and optional existing acyclic parent labels; unknown/private fields omitted. Empty revision-zero/null/missing is unavailable,positive-revision empty is reported clear. Lower task revisions cannot regress the current scope; conflict/malformed current reports invalidate until newer data. Generation/authority/replay change, gap ordisconnect invalidates rows;pending-receipt generationcatchup cannot resurrect oldsnapshot rows. Openings/snapshots fence localpageactions. No task-edit/approval authority follows. Focused 60 Rust/59 Python and four actual-Tern cases/16 pairs pass. Full campaign passes 337 Rust/328 Python, 24 gates/nine package pairs/68 native cases/176 pairs; all 238 source and log/binary/pair hashes match, 68 zero exits/no owned survivors. Review completed/reconciled, not independent approval of repaired bytes. Pixels/live/provider/window remain unestablished.

## Delegated-agent inventory

Runtime details/F2 includes Read delegated agents:one explicit subagent.list {session_id} snapshot,no automatic polling orchildcontrol. Cap128unique rows/64KiBcompleteprojectedtext;knownnullablefields typed andbounded,unknownprivatefields omitted,malformed/refused/unsupported unavailable rather thanzero. Onechild/localpage;Read resets tofirst,Next/Previous bind the exactsnapshot+opening. Closing/reopening invalidates oldresults butkeeps apendingread untilits singleoutcome;reopenneverduplicates it. Runtime/editor preservesdraft/inspectmode,ordinarySendoutsidepanel isnotlockedbythisread. Parent/childlabels/status/start/toolcounts areownerreports,notindependentpeers,liveness,completion orcontrolpermission. No Steer/Stop/tail/process/provider/configwrites. Focused 34 Rust/52 Python and four actual-Tern cases/twelve pairs passed. Full local/native synthetic campaign now passes 283 Rust/321 Python, 24 gates/nine package pairs/68 native cases/172 pairs; 234 source and all log/binary/pair hashes match, 68 zero exits/no owned survivors. Review completed/reconciled, not independent final-source approval. Current pixels still HTTP402; live/provider/window not established.

## Text-file attachments

Text attachments: press F5 in the native conversation or choose Attach text file inside Commands. Enter an exact absolute canonical local UTF-8 text-file path (<=4096 bytes), then click Review local file, Stage reviewed bytes and Add reference to draft separately. File cap 256 KiB; basename cap255bytes. No relative/expanded/symlink/directory/binary path acceptance, owner path reopening or automatic Send. Stage snapshots do not change if the original file changes later. The validated owner reference is appended once only when the full draft still fits64KiB; Send remains a separate user action. Eight admitted uploads per connection;pending/unknown disables Send/model/handler mutations and never auto-retries. Closing/removing the reference is local,not deletion of staged owner files. Personal installation does not receive this checkout feature automatically;real-owner/provider/window qualification remains open.

## Versioned local distribution

`scripts/distribution.py` provides explicit stage/verify/initialize/activate/status/rollback for a new private managed prefix;see [package preflight](operations.md#versioned-local-package-preflight). It does not adopt or modify the historical personal installation above,register the Tern plugin,edit PATH or restart Hermes/Tern. The two-file plugin and binary are checksummed together;compatibility remains exact source/local observations,other versions unqualified. Thirty-two retained generations are a hard refusal limit,not a pruning policy. Code rollback never restores or migrates coordinator/profile state. Current local subset:15behavioral methods and historical scratch CLI readback PASS;source review schema omission repaired with shared strict stage/verify validation. Included in commands-autorun-3 local verification; license, publication and live installation remain owner decisions.

## Existing CLI

| Invocation | Effect |
| --- | --- |
| `hermes-tern` | Help; never auto-starts an actor |
| `hermes-tern --help` or `-h` | Help |
| `hermes-tern --version` or `-V` | Build version |
| `hermes-tern preview` | Static native/plain SDK output |
| `hermes-tern doctor` | Discover existing Hermes owner, verify identity/capabilities; no inference |
| `hermes-tern open [--cwd ABSOLUTE_DIR]` | Native session picker;bounded listing only until explicit New/Resume;Close does not mutate a session |
| `hermes-tern sessions [--stored]` | Explicit live/saved session listing |
| `hermes-tern history --session ID [--plain] [--page N]` | One bounded read of an already-live session's display projection, then local immutable page inspection; never resume/create/send |
| `hermes-tern chat [--plain] [--resume ID]` | Native Tern or plain conversation through existing gateway |
| `hermes-tern chat --plain --query TEXT --timeout 300` | One bounded turn; denies approvals and skips clarify; may incur provider cost |
| `hermes-tern serve --state DIR --workspace DIR` | Explicit foreground local mailbox supervisor; starts no agents |
| `hermes-tern migrate-store --state DIR --workspace DIR --archive-id UUID` | Offline v1-to-v2 upgrade with a verified pre-upgrade archive; refuses live ownership or stale archive |
| `hermes-tern preflight-recovery --state DIR --workspace DIR --archive-id UUID --workspace-id UUID --database-sha256 HEX --expected-operator-key-sha256 HEX` | Offline exact schema2archive/key diagnostic;no live database access,repair or authority activation;[limitations](operations.md#offline-archive-diagnostic-without-restoration) |
| `hermes-tern recover-operator-key --state DIR --staging-id UUID --expected-operator-key-sha256 HEX --attest-original-key` | Offline publish the same intact retained original inode under existing exclusive ownership;no SQLite,key generation,overwrite or runtime launch;[limits](operations.md#recover-an-intact-retained-original-operator-key) |
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

Supervisor startup requires an intact private operator key for any existing database. New-key generation is only for an absent database with no entries besides `owner.lock`; complete key-first retry may additionally retain `operator.key`. Pending/unknown entries,sidecars andactor remnants refuse fresh initialization. No `--force`,automatic rotation or secret-reconstruction option exists. The separate offline recover-operator-key command accepts only an intact operator-attested original at exact `.pending-UUID` or a matching final-only retry,with an independently retained digest;state andparent must be private,existing owner.lock/journal metadata safe,one key name only. It opens no SQLite,creates no key andnever overwrites. Hash/attestation are not independent historical proof. See [key policy](operator-key-policy.md) and [recovery procedure](operations.md#recover-an-intact-retained-original-operator-key).

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
