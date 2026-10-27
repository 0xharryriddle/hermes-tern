# Operations and troubleshooting

Status: experimental connected client and local peer-tool runbook. Full live-actor recovery and release qualification remain open.

## Use the preview without installing

```sh
cargo run --locked -p hermes-tern -- preview
TERN_TSP=0 cargo run --locked -p hermes-tern -- preview
```

No credentials are needed. The output must say no agent is connected. A successful preview only proves the bootstrap path. To validate native rendering in an isolated environment, follow [development](development.md).

## Optional local installation

Coordinator IPC may fail after an operation has committed. The frame reader now continues a locally interrupted read (EINTR) without clearing received bytes, extending its original deadline, reconnecting or resending the operation. Reset, EOF and timeout still fail; an error is not proof of rollback. Inspect existing actor/task/message state through the authorized read paths before deciding how to proceed. Test fixtures retain bounded, closed diagnostic receipts instead of raw stderr/stdout; this improvement does not establish the cause of the earlier intermittent registration failure or provide automatic recovery.

Before a conversation, run `cargo run --locked -p hermes-tern -- doctor`. The gateway must already be running; the client never starts or repairs it. From the current checkout use `./target/debug/hermes-tern open` inside Tern for explicit New/Resume/Close,or `chat` to open directly;`chat --plain` is explicit outside Tern. `chat --resume ID` reattaches the returned runtime session identity. `sessions --stored` lists resumable sessions. The upstream `hermes` command and earlier personal installed copies are unchanged. [Native entry](configuration.md#native-session-entry-from-this-checkout) lists limits and controls.

Native: Enter sends, Shift+Enter inserts a newline, Ctrl+C requests interruption of the current turn, Ctrl+D/Escape detaches. Pointer Interrupt is bound to the displayed turn generation. Approval/clarification buttons bind an opaque local prompt instance as well as the upstream ID/generation, so a delayed old control cannot silently address a replacement—even when the upstream ID/generation repeats. Stale/expired controls refuse; use the newly displayed controls. For batch clarification, type a JSON object keyed by the shown question IDs and select Answer. Plain: `/cancel`, `/quit`, `/approve ID GENERATION CHOICE`, `/answer ID GENERATION TEXT` (batch TEXT is a JSON object). Plain explicit commands retain the upstream ID/generation contract, not the native instance token. Sensitive requests are declined, never collected as chat text.

If the owning Tern surface disappears, the native loop drops queued controls and the outer client attempts detach, not interruption or backend termination. A current/global output rejection remains a failure even when surface loss was decoded in the same read; static diagnostics never copy terminal-provided text to stderr. Shutdown classification uses a bounded decoded-inbox drain with no new terminal reads by the adapter; missing waker or excess queue fails instead of claiming clean exit. Gateway loss keeps the independent draft and disables submission; resume to reconcile, do not blindly resend. Production PTY/SDK/local-WS controls cover these paths. Actual Tern keyboard send/result and pointer Detach also passed against a synthetic gateway; real Hermes/provider and window-plugin acceptance remain separate.

`chat --plain --query TEXT --timeout 300` can call the configured provider. It explicitly denies approval prompts and skips clarification. Do not use it as an unattended auto-approval runner. An interrupted/unknown/failed turn is a nonzero exit. After ambiguity, resume and inspect rather than automatically resending.

These commands change the selected install root or Tern plugin configuration. The bootstrap session tests them in private scratch, not in the user's personal plugin directory. Read [security](security.md) before choosing a personal install.

```sh
cargo install --locked --path bin/hermes-tern --root "$HOME/.local"
tern plugin link "$PWD/plugins/hermes-tern"
tern plugin list --json
```

Ensure `$HOME/.local/bin` is on the login shell's PATH, then select `Hermes: Open SDK preview (no agent)` from Tern's palette. Do not mistake package registration for window-entry execution: without a daemon, `plugin list` can report `ready` from manifest parsing alone. With a daemon, the catalog still does not certify the window half. The actual palette action requires its own live check.

At this checkpoint a scratch-root `cargo install`, isolated `tern plugin link`, and catalog readback succeeded. The separate isolated desktop-window probe did not produce the preview: its initial state was unsigned-in behind Tern's access gate and the content assertion timed out. Do not copy personal login state into tests or bypass that gate; repeat with an explicitly authorized test account/window.

To remove only the link, leaving the source checkout intact:

```sh
tern plugin unlink hermes-tern
```

Do not use a broad plugin-folder cleanup. Personal install/uninstall is an explicit operator decision, not part of `verify.sh`.

## Owner plugin inventory from this checkout

Use the checkout binary,not an assumed updated personal installation. Open Commands/F4,explicitly load its catalog if needed,then inspect the exact builtin Read /plugins row. Check Plugin profile,Plugin owner and the visible profile-wide discovery notice before clicking:initial owner discovery may load/register plugins and refresh secret sources. This is not a strictly passive cache read.

Click Read /plugins once. The client sends `plugins.list {}` only;no command handler,plugin management,install/toggle,tool invocation or configuration write. Complete bounded plugin/toolset/portable-MCP metadata is a report,not health or execution permission. Raw errors/paths/settings are suppressed. Local output chunks and Return preserve the conversation draft without another RPC. Enter/typing/paging are not consent.

Unavailable/Unknown is not a zero inventory or rollback. Once admitted,do not blind retry an ambiguous outcome;the established Commands pending/Unknown gate keeps Send and sibling mutations withheld. Opening/closing does not cancel owner discovery. Synthetic PTY/native behavior is verified;actual-owner discovery and process/plugin effects remain unqualified. Do not load personal plugins or restart Hermes merely to close this optional panel's gate. [Verification](verification.md) separates local/native and live acceptance.

## Native MCP reload from this checkout

This is implemented in the checkout build and tested with synthetic owners, not an actual-owner MCP acceptance result. It does not update a personal installation or restart Hermes. Do not trigger a live reload merely to complete an optional panel.

1. Open Runtime details (F2), then Review MCP reload. Review is local and sends no RPC. Check the exact displayed owner/profile/session. Read the profile-wide scope: every chat can be affected, MCP server processes can change or connect remotely, and the subsequent same-connection reattach can refresh policy/adoption/hooks. Idle/reconciled session controls are required.
2. Deliberately click Confirm profile reload and reattach. Enter, paste and editor send are not confirmation. One minimal `reload.mcp {session_id,confirm:true}` is admitted, followed only after its correlated valid report by one dedicated `session.resume {session_id,display_projection:true}`. There is no `always`, `rev`, config write, durable-idempotency promise or automatic retry. The local cap is eight admitted reloads per connection.
3. Wait for MCP reattach readback complete. Applied/unchanged/busy refer to the owner's chat-policy report; unavailable names remain visible. A busy result is not rollback or evidence of no profile effects. Reattach reads current owner state, not an atomic receipt for a particular MCP configuration revision or proof of tool health. External profile changes can race.
4. Return to conversation to recover the unchanged draft. Closing the panel never cancels an admitted operation. A lost reply, malformed report/readback, timeout or disconnect stays outcome unknown across reopening and keeps Send/model/command/attachment mutation controls withheld. Do not press reload again or generate a new attempt to recover an ambiguous effect. Inspect/reconcile the existing owner explicitly before a fresh connection/consent; never delete locks, restart the owner or edit its database to bypass this state.

The client checks qualified identity/authority, safe policy and numeric readback floors before unlocking, and rejects stale local confirmation callbacks. These are local defenses, not an owner-side CAS or a hostile same-user sandbox. [Contract boundary](hermes-integration.md#mcp-qualification-boundary) and [verification](verification.md) state the evidence ceiling.

## Troubleshooting the bootstrap

| Symptom | Check | Honest interpretation |
| --- | --- | --- |
| No native UI | Run inside Tern; check explicit `TERN_TSP=0` and native-surface settings; run isolated smoke | Plain fallback can be correct behavior |
| Missing executable from plugin | Check the login shell PATH and install root | Registration can work while the command cannot launch |
| Cargo toolchain download error | `rustup show`, verify the exact pinned tools | Setup failure, not a code test failure |
| Dependency unavailable | Check network/cache and pinned revision | `--locked` still permits required downloads |
| Docs checker fails | Read its exact target/fragment/task diagnostic | Structural error; verify the predicate before editing prose |
| Native shot exists but text check failed | Preserve scenario/logs; distinguish real surface text from shell echo | A PNG file alone is not a successful smoke |
| Plugin `ready`, action absent | Inspect the window's plugin error/logs | Host/catalog state does not prove window entry loaded |
| `run` or `attach` rejected | Use help and [status](STATUS.md) | Those commands are intentionally not implemented |

## Local coordinator recovery

- Revoke a route with `coord --state DIR route --sender UUID --recipient UUID` (omit `--allow`). Queued mail is held until reallowed or TTL expiry. Already submitted content cannot be recalled; its matching incarnation can still ACK.
- On a lost send response, use `coord --state DIR inspect-message --sender UUID --message UUID`; the older `messages` list shows only the latest 128 rows and absence there does not prove non-admission. Do not generate a new message ID. A deliberate retry with the same sender, ID and exact body returns the existing receipt. Changed content conflicts. The CLI/plugin does not automatically retry.
- On a lost registration/rotation response, use operator `coord --state DIR actors` to find the original binding or actor and its current incarnation/`credential_file`. Do not issue another rotation merely to recover the reply. The snapshot gives the expected private path, not a credential value or a filesystem-health guarantee. Unknown or uncommitted candidate files grant no authority.
- Private publication syncs the containing private-directory entry before staging, then file sync, no-replace rename and directory sync before registry commit. Failed staging and old incarnation files remain private and count toward the 1024-entry directory bound; each new body is at most 4096 bytes. No automatic cleanup/recovery command deletes them. Preserve them for explicit operator reconciliation rather than sweeping by prefix.
- On restart, submitted deliveries become ambiguous and are not returned by `receive`. ACK is permitted only for an undisposed original delivery incarnation with independently established receipt knowledge. Explicit operator disposition can close accounting or create a distinct-ID successor as described below. Never edit SQLite to make an old row queued. Private archive capture itself does not settle or re-offer work.
- A full logical journal refuses new admission while reserving event slots for accepted message/task settlement. This does not reserve disk bytes. If the filesystem cannot write, settlement can still fail; preserve state, restore storage availability and reconcile before retrying. Existing older journals can lack the new headroom guarantee.
- Ordinary writes also protect a finite safety-event allowance (`min(max_events / 4, 256)`) for route denial and credential rotation. Committed safety events consume it across restarts. Exhaustion explicitly refuses further changes without revoking authority; do not interpret that error as success. Stop the coordinator to halt further dispatch, preserve state and reconcile. Settlement of older accepted work does not require retroactively creating this allowance. No unlimited emergency-write guarantee exists.
- Semantic-integrity failure refuses open before recovery changes message states. Preserve the private directory for diagnosis; do not delete the database/lock or normalize rows. Archive capture requires a functioning supervisor and validates its copy; it is not offline salvage, database repair or schema migration.
- Fresh schema/version/workspace initialization is one transaction. A store already left incomplete by an older binary is not silently repaired; preserve it for diagnosis. Local tests cover metadata-write failure and credential transaction kill boundaries, not filesystem power-loss guarantees.
- Task expiry is not proof a worker stopped. Release requires the complete inspected lease identity plus an explicit stopped attestation; the next claim advances generation and rejects old completion/release requests. The former task-ID-only release request is refused.

### Reconcile an existing private binding or operator key

After a failed `bind-peer` response, keep the original actor/home/physical-session tuple. An explicit exact retry verifies current actor authority and expected file content, completes file/directory synchronization and returns the original path only after success. Repeated sync failure still refuses after restart; do not rotate credentials or delete the retained file merely to hide the error. A conflicting or unsafe file is not overwritten. Exact reuse needs no free publication slot. Client/bridge reads remain read-only and do not establish publication durability.

On supervisor startup, the existing ownership lock is acquired before checking the operator key, and the key guard must finish before any database-file creation or SQLite opening/recovery. A missing key for any existing database refuses; it is never silently replaced. An invalid/unsafe key or persistent file/directory/parent sync failure also refuses before database recovery. After storage availability is restored, an intact key can be reused without generating authority. A subsequent key recheck before listening cannot undo effects after the initial guard passed. These are bounded local filesystem controls, not token authenticity, arbitrary same-user tamper exclusion or power-loss qualification. Never edit a real key to emulate the synthetic fixtures.

New-key bootstrap requires an absent database and a directory containing only `owner.lock`. A complete private key already published before database creation can be reused only with no entries besides that key and the lock. Unknown/pending files, sidecars or retained actor files refuse fresh initialization. If an established key is missing/damaged, preserve the complete state and stop repeated startup attempts. Only an independently identified intact original retained key qualifies for the explicit offline publication procedure below; no secret reconstruction or arbitrary replacement is supported. Do not delete the database, lock or pending files, import an archive or copy a historical key merely to bypass refusal. See [operator-key policy](operator-key-policy.md).

### Recover an intact retained original operator key

This is an offline coordinator-state operation, not a Hermes session-lease repair. Use it only when the operator independently knows the exact retained original file and its previously retained digest. The command cannot establish historical authenticity, revocation policy or stopped Hermes/OMP workers. It never reconstructs a missing secret, copies a key from another state or chooses a candidate by scanning names.

1. Explicitly stop only the selected coordinator and wait for that owned process to exit. Preserve its entire state, including the existing owner.lock, journal, sidecars, credentials, bindings and archives. State and its existing parent must both be canonical same-owner private directories. Do not delete or recreate a lock to force ownership.
2. Select the exact canonical staging UUID for `STATE/.pending-STAGING_UUID`, not the newest pending name or peer-supplied text. Supply a separately retained lowercase SHA-256 of the exact original 64 lowercase-hex key bytes, with no newline. Never paste the key itself into chat, arguments, logs or Git. If no intact original or independently retained expectation survives, stop: this command cannot recover it.
3. Deliberately run `hermes-tern recover-operator-key --state STATE --staging-id STAGING_UUID --expected-operator-key-sha256 RETAINED_HEX --attest-original-key`. It acquires the existing exclusive nonblocking owner lock before key reads/syncs, checks existing regular private journal metadata without opening SQLite, validates the bounded private candidate and digest, then syncs/rechecks and moves that same inode to operator.key without replacement. No directory, lock, journal or replacement key is created; no actor, task, binding, event or delivery row is changed.
4. A successful receipt is fixed metadata: `operator_key_recovered`, `published:true`, `new_key_generated:false`, `database_opened:false`, `runtime_launched:false`, `key_originality:operator_attested_not_independently_proven`, `staging_provenance:not_established` and `workers_stopped:not_established`. It contains no key or digest. Both staged and final names present refuse even when equal; preserve both for an operator decision. An absent staging name allows only exact matching final-file revalidation/synchronization, not a claim that the chosen staging UUID produced it.
5. An error or lost reply is not rollback: rename may already have published final before a sync failed. Inspect those same exact names and explicitly retry the same UUID/digest only after resolving the failure; there is no automatic retry or cleanup. Repeated parent/file/pre-rename/post-rename sync failure continues to refuse. Ordinary `serve` is a separate explicit decision and may then perform normal submitted-to-ambiguous startup recovery; this key command never does.

The production-process fixtures exercise same-inode publication, nonmutating refusal, repeated sync faults, byte/inode substitution, a racing final file and pre/post-rename SIGKILL with lock exclusion. Full local verification passes395Rust/367Python; independent review is unavailable because it inspected zero source rows, so this remains an author-verified partial local handoff. These tests are not hostile same-user exclusion, physical power-loss, key-history authenticity, damaged-secret repair, archive restoration or live-agent acceptance.

### Recover an intact retained current actor credential

Use this only with a functioning coordinator and operator key, an independently identified current actor/incarnation and an intact original credential retained under the fixed private staging name. `coord actors` provides the current identity and expected final path, never the secret. Files left by a failed registration or pre-commit rotation usually have no current authority and must not be promoted. Do not paste credential contents into chat, command arguments, logs or Git. If no intact current secret survives, this command cannot reconstruct it; stop and choose an explicit operator recovery/rotation policy separately.

1. Select the exact staging UUID and current actor/incarnation; do not select the newest `.pending-*` entry or guess from peer text. Run `hermes-tern coord --state DIR inspect-staged-credential --staging-id STAGING_UUID --actor ACTOR_UUID --incarnation INCARNATION_UUID`. A `staged_verified` receipt checks the full envelope/hash against current registry authority, reports `published=false` and does not sync or move the file.
2. Deliberately run `hermes-tern coord --state DIR recover-credential --staging-id STAGING_UUID --actor ACTOR_UUID --incarnation INCARNATION_UUID`. It keeps the database authority unchanged while syncing/rechecking the retained file and directory identities and publishing to the fixed current credential path without replacement. Existing matching peer bindings need no rewrite. A success receipt reports `authority_changed=false`, not a newly authorized actor or a live session.
3. After a lost reply, sync failure or coordinator restart, retry those same IDs explicitly. Staging may have moved to final already; final-only retry revalidates current authority and completes sync. `staging_provenance=not_established` prevents interpreting this as proof about the absent staging ID. Wrong/stale/partial/unsafe candidates or missing staged and final files refuse without replacement or reconstruction. Old rotated credentials remain revoked.
4. Both staged and final names present refuse and preserve both, including identical bytes. Resolve the exact conflict as an operator rather than deleting evidence or forcing overwrite. Recovery can move an existing entry at the publication entry cap but does not free storage or authorize cleanup. A failed response does not imply the file move rolled back; inspect exact identities before acting.

The command holds a registry writer reservation during bounded filesystem work, so an external SQLite writer is excluded until it ends; direct database editing remains unsupported. It does not alter accepted work, task leases, clock, events, binding files or personal Hermes configuration. Startup recovery remains separate. Intact retained-original operator-key publication is a different offline command above; damaged-secret repair, incomplete-file salvage, secret recreation, archive activation and physical power-loss qualification remain open.

### Capture and verify a private archive

Choose one canonical archive UUID before submission, then run `hermes-tern coord --state DIR archive --archive-id UUID`. A successful receipt identifies the fixed private archive directory, database hash/size, exact counts and watermarks. The bundle contains a standalone logical SQLite snapshot and manifest, not a raw main-file copy that can omit WAL. Source rows, delivery states, event sequence and clock are unchanged; the command does not free journal capacity.

Use `hermes-tern coord --state DIR inspect-archive --archive-id UUID` to verify the existing bundle. After a lost reply or coordinator restart, inspect that same ID; a deliberate `archive` retry verifies the original capture and repeats archive-root synchronization instead of replacing it with newer state. If the original rename succeeded but parent fsync failed, retries continue to refuse while fsync fails and acknowledge only after it succeeds. Inspection itself does not fsync: `status=verified` establishes byte/manifest consistency, not completion of the publication durability boundary. A valid archive is not a live replica or evidence that any worker stopped. Synchronous capture/verification can delay IPC; timeout is not evidence the operation rolled back.

Missing, partial, conflicting, corrupt or unsafe final targets refuse without replacement. Verification/querying uses one read-only image of the exact hashed bytes; migration commit and retry receipt paths recheck the retained database/manifest descriptors and bytes. Do not modify a bundle while it is in use. Refusal preserves the source, not an assurance that an arbitrary external writer has stopped. Loading can temporarily use two database-sized buffers plus SQLite/query overhead; large-store RSS/latency is not qualified. Failed private staging directories remain in `state/archives`, consume capacity and are not automatically promoted or deleted. Complete valid bundles have an explicit publication-recovery command below; partial or corrupt ones still refuse without repair. At entry/byte capacity, preserve the exact inventory for operator reconciliation; do not sweep by prefix or edit the manifest to bypass validation. Archive inspection does not need spare disk capacity for a new snapshot.

The archive holds sensitive peer content, results, workspace/actor metadata and credential hashes. It excludes private credential/key/binding files and Hermes' own database. Do not commit it or copy it into an active state directory: no supported restore, authority reactivation, retention deletion or compaction workflow is shipped yet. The explicit additive schema migration below archives the existing store; it does not activate an archive as another writer. Treat SHA-256 as consistency evidence, not a signature or tamper-proof provenance.

### Finish a complete retained archive publication

1. Use the exact private staging UUID from the retained `state/archives/.pending-UUID` inventory and the intended archive UUID. Never select the newest entry by prefix or infer an ID from peer text. Run `hermes-tern coord --state DIR inspect-staged-archive --staging-id STAGING_UUID --archive-id ARCHIVE_UUID`. A `staged_verified` receipt reports `published=false`, the digest and snapshot metadata; it performs no fsync and does not make the bundle durable or active.
2. After inspecting the receipt, run `hermes-tern coord --state DIR recover-archive --staging-id STAGING_UUID --archive-id ARCHIVE_UUID --database-sha256 HEX`, using that exact lowercase digest. Only a complete private same-workspace bundle qualifies. The command synchronizes both files and the bundle directory, rechecks retained descriptors, publishes without replacing a final name, synchronizes the archive root and rechecks again. Snapshot bytes/inodes and live logical rows stay unchanged; no new snapshot is captured.
3. If the reply fails before rename, preserve staging; after rename it may instead leave the final bundle. Deliberately retry the same identifiers/digest to finish synchronization. If staging is absent and the exact final archive is present, success confirms final ID/hash, not proof that the absent staging UUID originally produced it. If both names exist, the command refuses and preserves both; resolve the conflict explicitly rather than sweeping files or choosing a winner automatically.
4. After a lost reply/restart, inspect the exact final archive and retry recovery to complete any failed sync. Moving an existing bundle needs no new entry reservation, so it can succeed at archive entry capacity. It does not free archive space. Missing manifests, invalid/partial bytes, symlinks, unsafe modes, wrong workspace/digest or malformed IDs refuse without reconstruction or repair.

This requires a functioning v2 coordinator; v1 archive bundles are supported, but a live v1 store must still be migrated separately. The command does not recover credential files, activate an archive, stop workers, restore accepted work or compact the source. Check-then-publish windows are not a hostile same-user sandbox, and the injected syscall tests are not physical power-loss qualification.

### Offline archive diagnostic without restoration

`hermes-tern preflight-recovery --state STATE --workspace WORKSPACE --archive-id ARCHIVE_UUID --workspace-id WORKSPACE_UUID --database-sha256 ARCHIVE_HEX --expected-operator-key-sha256 KEY_HEX` is a separate offline command,not a coordinator IPC request. Supply exact canonical private state/workspace paths and independently retained expectations;never pass the operator key itself. Stop only the owned coordinator explicitly first. The command opens its existing owner.lock without creating/truncating it and refuses live ownership;it never clears/deletes a lock.

It verifies an exact private schema2archive through the same bounded read-only captured image/manifest checks. It additionally compares the complete main.sqlite_schema type/name/tbl_name/nullable-SQL inventory against the same supported initializer in memory,including automatic indexes and primary/unique/check/foreign-key/column/type definitions;only physical rootpage is excluded. Extra or altered objects,including triggers,views,indexes,internal statistics and semantically equivalent alternate SQL spelling,refuse rather than being repaired or normalized. Materialization is bounded to64objects,256UTF8bytes per identity field,16,384bytes per SQL and65,536bytes total metadata before allocation. These are not SQLite parser/RSS/latency guarantees. Any archived queued,submitted or ambiguous mail(including operator-disposed ambiguity),or an active task,refuses. Schema1archives,wrong workspace/root/hash,missing/unsafe/mismatched operator key and unsafe archive components refuse. The retained key descriptor and archive/state/lock/directory identities are rechecked. No live database or sidecar is opened,created,recovered,checkpointed,migrated or copied. The key is only read/rechecked,never generated or synchronized. The archive image can temporarily require two database-sized buffers plus SQLite overhead;maximum-size latency/RSS remains unmeasured.

Success exits0 with JSON `diagnostic_verified`,the exact archive receipt,`archive_schema_definition:exact_supported_v2` and `activation_supported:false`,`restore_authorized:false`,`key_originality:not_established`,`workers_stopped:not_established`,`live_state_inspected:false`,`live_journal_equivalence:not_established`. This proves those retained input and exact supported DDL checks,not historical key originality,a stopped worker,a matching current journal,restore safety or permission to activate authority. A caller-chosen key hash cannot prove provenance. Errors exit1 with empty stdout and bounded stderr;invalid CLI syntax exits2. No automatic repair or retry. Preserve current state and investigate rather than copying the archive into an active store. Fresh-v2 and frozen-v1 explicitly migrated to v2 are tested positive inputs;an independently authored equivalent schema is intentionally unsupported. Archive activation still requires separately designed current/archive all-table credential/counter/disposition/migration equivalence,worker quiescence and crash-reconcilable replacement.

### Upgrade an existing v1 store offline

1. Stop its coordinator through the operator `coord --state DIR shutdown` command and wait for that owned process to exit. Do not delete `owner.lock`, kill an unrelated process or confuse coordinator shutdown with stopping Hermes/OMP workers.
2. Choose one canonical archive UUID and run `hermes-tern migrate-store --state DIR --workspace DIR --archive-id UUID`. It requires the original private state and matching canonical workspace. Preserve the receipt with `from_schema=1`, `to_schema=2`, archive ID and hash. Source logical rows, delivery state, clock and events are unchanged.
3. After a lost reply or migration failure, deliberately retry the same ID. A committed migration returns its original receipt only with the intact witness/archive. A pre-commit archive whose logical contents no longer match refuses; investigate why source changed, preserve the old archive, then choose a new ID only for a new intentional migration attempt. Do not overwrite manifests or sweep retained staging to force success.
4. Start `serve` explicitly after success with the original intact private operator key still present. Migration does not create or repair that key; a migrated journal missing it refuses startup. Normal startup then converts submitted mail to ambiguous; migration itself does not. V1/v2 archives remain inspectable through the v2 coordinator.

Fresh v2 stores do not require migration and refuse a fake v1-to-v2 receipt. Unknown schema, missing state, active ownership, unsafe files or wrong workspace refuse. There is no down-migration/restore command: preserve the entire state and archive if rollback is needed; do not run an older binary against v2 or activate the archive with copied credentials.

### Decide an ambiguous message explicitly

1. Inspect the exact sender/message pair. Retain its `delivery_incarnation`, not the actor's latest incarnation after rotation. Review existing disposition/provenance before deciding. Queued, submitted, expired or consumed messages cannot be disposed by this command.
2. Choose one decision UUID and a nonblank bounded reason. To end pending accounting without claiming execution, run `coord --state DIR dispose-message --sender UUID --message UUID --delivery-incarnation UUID --decision-id UUID --action close --reason TEXT --attest-outcome-unknown`.
3. To intentionally risk duplicate external execution, use `--action reoffer --successor-id UUID --ttl 300` instead. The unused successor ID is distinct from the old ID; no original is reset. The new row copies the original sender/recipient/payload/kind/correlation, is marked as operator re-offer rather than new sender intent, and must pass current route and storage/journal limits. This is not permission to take a task lease or bypass tool approval. Do not use re-offer until its actual external effects have been assessed.
4. After a lost reply, inspect the original. Replaying the exact complete decision returns the same receipt with no additional rows/events/clock writes; changing the ID, reason, action or successor conflicts. Decisions are immutable. A new decision on a later ambiguous successor must name that successor's own delivery identity.

The old row remains `ambiguous` evidence, but its ACK is refused after disposition. `snapshot` and `messages` disclose decision/provenance fields; `unresolved_message_count` is separate from retained row totals. Close frees pending/debt slots only. Receipt `outcome=unknown` and `process_stopped=not_established` do not establish success, non-execution, stopped workers or exactly-once effects. Peers cannot issue disposition or migration commands.

### Release only the inspected task lease

1. Read `hermes-tern coord --state DIR inspect-task --task UUID`. Preserve `owner_actor_id`, `incarnation` and decimal-string `generation` from that task, not from a later actor listing.
2. Independently establish that this exact prior writer has stopped. A lease deadline, missing window or coordinator restart is not proof. The coordinator does not terminate the writer for you.
3. Submit `hermes-tern coord --state DIR release-stopped --task UUID --owner UUID --incarnation UUID --generation N --attest-stopped` with those retained fields. The response reports the exact tuple, `state=released` and `process_stopped=operator_attested_not_enforced`.
4. On a lost reply, inspect the task. An intentional exact retry is inert while the same lease remains released, even after coordinator restart. A newer generation, owner or lease incarnation refuses the old request. Do not replace the retained tuple with the new lease merely to make the command succeed; a new release needs independent evidence for its new writer. Completed tasks refuse release.

The database state and release notice commit together. A failed notice write leaves the task active; lack of a success reply alone is not rollback evidence. The CLI never automatically retries or upgrades an old unfenced request.

### Observe without changing delivery

Start with `hermes-tern coord --state DIR snapshot`. Retain its `workspace_id`, `observation_epoch` and `event_watermark`. Poll explicitly with `events --workspace UUID --epoch UUID --after N --limit 64`; use the returned `next_after` for the next page and follow `has_more`. This is a pull API, not an automatic background watcher. A `snapshot_required` response is a successful protocol response requiring recovery, not a successful replay: take a new snapshot, do not advance from null `next_after` or repeatedly submit the stale epoch.

Snapshot counts are exact, but lists disclose `truncated=true` when not all rows are shown. Use an independently known sender/message pair with `inspect-message`, or task ID with `inspect-task`, to inspect omitted records and task results. For complete enumeration without already knowing every key, use the state-page walk below. Event entity IDs alone are not enough to reconstruct a message's sender or a complete route. If `event_replay_complete=false`, preserve the journal for diagnosis; the read API does not repair missing history.

These reads never expire/deliver/ACK messages or change leases. Results and peer payloads remain untrusted attributed data, even when obtained with an operator credential. Do not paste them as shell commands, system instructions or approvals. A completed task row proves recorded completion only, not correctness or process termination.

### Enumerate all retained state coherently

1. Run `hermes-tern coord --state DIR state-index`. Retain its workspace, epoch, revision and seven collection counts. This does not pause workers or reserve a snapshot.
2. For each `actors`, `routes`, `messages`, `tasks`, `dispositions`, `migrations` and `events`, start at `--offset 0`: `hermes-tern coord --state DIR state-page --workspace UUID --epoch UUID --revision HEX --section messages --offset 0 --limit 64`. Use the exact returned identifiers; never infer them from peer text.
3. Require `status=ok` and matching workspace/epoch/revision. Accumulate complete items privately, then follow `next_offset` while `has_more=true`. A short page with `byte_limited=true` is not the end; large escaped results can occupy a page alone. Check enumerated counts against the index, separately for each section.
4. If any page returns `restart_required`, discard the entire partial assembly, including earlier sections, and obtain a new index. Do not reuse the changed response's revision with an old offset. Bound retry attempts; arrange a quiet interval if ongoing writes keep invalidating the walk. Even an empty receive can advance the persisted clock and invalidate it.

The complete walk represents one logical persisted state, not a continuously current view or evidence that an agent stopped. It includes untrusted peer payloads/results and operator reasons; keep output private and never execute it or treat it as approval. Credential values/hashes are omitted, so it is not a database backup/restore format. Migration rows are stored witnesses, not a fresh integrity check of their archives. There is no automatic watcher, compaction, accepted-work deletion or restore action. Reads do not expire/deliver/ACK messages. Synchronous whole-store validation/hash work can delay requests; maximum-size latency/RSS and starvation guarantees remain unqualified.

## Proposed connected-runtime incident handling

- Gateway ownership conflict: discover and attach to the owner, or choose an authorized dedicated home. Never delete a lock or kill a personal gateway to force attachment.
- Disconnected control channel: show disconnected, stop new dispatch, reconcile pending approvals and delivery attempts; never infer success from last visible text.
- Approval expires: remove the exact request UI and deny/cancel locally. Do not automatically approve a replacement request.
- Actor cancellation stalls: show cancelling/unknown; preserve evidence. An explicit escalation policy must verify PID/start identity before signalling an owned process group.
- Disk budget full: reject new accepted work, preserve in-flight records and pending user decisions. Do not silently prune accepted mail.
- Ambiguous peer submission: reconcile using a verified upstream request key or ask the user. Never replay an entire last prompt automatically.
- Renderer exits: reconnect to the independent owner if available. A shutdown of the runtime or daemon is a separate event.

## Diagnostics and retention

For operator provisioning, rotation and fail-closed physical-session handling, see [peer tools](peer-tools.md#operator-provisioning). Do not treat a plugin status result as gateway/session liveness, automatically rebind after compression, or retry an unknown send using a fresh message ID. The isolated loader smoke is not permission for a personal plugin install or gateway restart.

Prefer sanitized event type, IDs, sequence, state transition, error class and elapsed durations over raw transcript dumps. Store private diagnostics under the user's controlled cache/state root with a finite cap. TSP recordings may contain text, file names and tool output; opt in explicitly and redact before sharing.

The native smoke prints its retained scratch path. Do not commit scratch screenshots, logs, source checkouts, credentials or runtime data. The canonical [verification checkpoint](verification.json) records scope/results without private paths.

## Versioned local package preflight

`python3 -B scripts/distribution.py` is explicit Linux x86-64 build tooling,not an automatic installer or a published release. It packages only the selected trusted executable,`plugin.toml`,`window.luau` and a checksum/compatibility manifest. It runs that executable with `--version`,never a provider/session action. Source build observations are not future Hermes/Tern support certification. The manifest deliberately records license owner-unselected,product incomplete and external gates unestablished;checksums are not signatures.

Use new absent paths under a private scratch root first. The following command shapes assume `ROOT`,`CANDIDATE` and `PREFIX` are explicit absolute paths. Never point PREFIX at an existing manual install or at a coordinator/profile state directory.

```sh
python3 -B scripts/distribution.py stage --binary "$ROOT/target/debug/hermes-tern" --plugin "$ROOT/plugins/hermes-tern" --sources "$ROOT/docs/sources.json" --version 0.1.0 --out "$CANDIDATE"
python3 -B scripts/distribution.py verify --package "$CANDIDATE"
python3 -B scripts/distribution.py initialize --prefix "$PREFIX"
python3 -B scripts/distribution.py activate --prefix "$PREFIX" --package "$CANDIDATE" --expected-current none
python3 -B scripts/distribution.py status --prefix "$PREFIX"
"$PREFIX/bin/hermes-tern" --version
```

Read the exact generation from status before an update;pass it as `--expected-current GENERATION`. Rollback is `python3 -B scripts/distribution.py rollback --prefix "$PREFIX" --generation RETAINED_GENERATION --expected-current CURRENT_GENERATION`. Both identities are complete manifest SHA-256 values,not versions or aliases. Versioned binary/plugin payloads stay paired behind one atomically replaced current link;old running processes keep their existing image. Stage never activates. No PATH edits,personal Tern install,plugin registration,process restart or credential copy is performed. The reported plugin path is for a separate owner-authorized operation;an already copied Tern package does not follow this link automatically.

Unknown/extra/symlink/hardlinked package files,version/hash mismatch,unmanaged prefix,lock conflict,stale expected-current or32retained generations refuse. No automatic deletion/pruning;failed staging is retained and consumes capacity. Errors after a pointer swap can mean current changed but durability is unknown:read status,inspect exact bytes,then explicitly retry against that observed identity. Never assume failure implies unchanged installation or blindly reuse the old expected-current. Retry/rollback resyncs package files and directories before success. These tests inject owned fsync failures;physical power loss and a hostile same-user writer remain unqualified. Code rollback does not down-migrate or restore coordinator databases,clear ambiguous work or recover authority.

## Coordinator-load acceptance, synthetic identities only

The bounded T-13 harness has focused acceptance, with parent-verified 32 Python methods and the post-report-guard CLI matrix; full local campaign passes337Rust/333Python,two live ignored,zeroResourceWarning,20gates/ninepackagepairs andall240source/log/binary hashes match withoutdrift. Gate children reaped/no remainingownedgroups;postcampaign docs checkedseparately. Bounded review is completed and transcript/current reread hashes reconciled, without postrepair execution approval. Run it explicitly with `python3 -B scripts/verify_coordinator_load.py --out "$TMPDIR/NEW_ABSENT_OUTPUT"` from the checkout after building, where TMPDIR is a private mode-0700 directory. Each invocation retains a new independent generation; never reuse an existing output. It owns a new mode-0700 scratch output, isolated HOME/XDG/state/workspace and one production `serve` child. Never point it at personal coordinator state, an existing output, a symlink alias or a Hermes profile. One/two/four producers plus a distinct receiver means two/three/five synthetic registry records, not live workers or an execution sandbox.

The selected pressure point is the existing 128-unresolved-message mailbox bound, not a throughput target. A receive submits at most eight messages and does not free their pending slots; only an entitled ACK settles one. An explicit capacity refusal is distinct from a missing/lost response. The harness may deliberately submit its previously refused envelope once after observing one ACK, but must not retry an uncertain mutation or mint a new ID to hide its outcome. Accepted sender/message keys, payload hashes, delivery sequences and complete persisted state/event sets must agree after draining. A count alone or the bounded overview is insufficient evidence.

Retain the output and exact ownership inventory on both success and failure. Shutdown targets only the fixture's exact child; no owner gateway restart, profile change or state repair is part of the run. RSS high-water, wall time and regular-file lengths describe that finite case only. Live/mixed-agent workloads, archive maximum-size resources, filesystem ENOSPC/power loss and actual restore/key repair remain separately unqualified. A report which claims those gates from synthetic IPC must be rejected.

## Release checklist

1. Owner selects the project license and authorizes publication.
2. Read the full changed source, especially Luau process/file/pane operations.
3. `./verify.sh` passes on the release bytes; run the native and plugin-window gates separately.
4. A connected release also passes the live Hermes session/approval/cancel/recovery matrix on declared versions.
5. Actor/OMP releases carry their own routing, durable ambiguity and interoperability evidence.
6. Publish binary checksums, dependency/source pins, platform support, known limitations and upgrade/rollback instructions.
7. Verify the remote release by reading back the exact tag/assets only after an authorized publish.

Initial source-only GitHub publication and its remote CI success are verified in [STATUS](STATUS.md);they are not a release tag,qualified binary package or production claim. Native/model continuation remains uncommitted/unpushed. New release tags/assets,personal updates and publication require their own explicit authorization and exact-byte verification.
