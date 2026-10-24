# Coordination protocol and persistence

Status: local subset implemented; the full contract below remains a target where marked proposed. This protocol belongs to hermes-tern, not to the upstream Tern SDK or Hermes gateway. The static preview itself still starts no coordination service.

## Implemented local subset

The current Linux supervisor uses a token-authenticated abstract Unix socket derived from the canonical private state path and effective UID; it leaves no filesystem socket to delete. It stores its own SQLite actor/routes/messages/tasks/events, not Hermes data. One JSONL request/response per connection carries version/id/auth and a closed operation; request/response limits are 256/512 KiB. Peers cannot use operator operations. Empty struct variants reject unknown fields even for no-argument operations.

Pull delivery currently uses queued → submitted → consumed, with submitted → ambiguous on restart and explicit incarnation-bound ACK. Submitted means the mailbox response was selected/committed, not that a model understood it. The current envelope uses decimal epoch-second timestamps, sequence, trust and state; it does not yet implement the proposed causation/hop/attachment/attempt/replay model below. The pipe bridge additionally checks an operator-attested physical Hermes session/home binding before every bound operation. [Peer tools](peer-tools.md) gives its exact usable boundary and failure rules.

Route permission is checked both at admission and at queued-message selection in the delivery transaction. Revocation leaves accepted queued mail stored but undeliverable until the operator re-allows the route or its TTL expires. It cannot recall previously submitted content, and does not remove the recipient's incarnation-bound right to ACK it. An exact sender retry may return an existing admission receipt after revocation; it does not admit or deliver new work.

Every new event checks the remaining event-row obligations inside its mutation transaction: two slots per queued message (submit and ACK), one per submitted/ambiguous message, and one per active task (complete or operator release). Admission and management writes cannot spend those slots. A separate safety allowance of `min(max_events / 4, 256)` slots is protected from ordinary writes for route denial and credential rotation; each committed safety event consumes the allowance and restart does not replenish it. Settlement reduces its own debt and does not demand retroactive safety headroom from older journals. This is finite logical event capacity, not reserved filesystem space, unlimited revocation, proof that a peer will ACK, or a complete archival/recovery facility. Once the allowance and ordinary spare capacity are exhausted, further safety writes explicitly refuse without mutation; stop the coordinator and reconcile rather than claiming revocation succeeded. Legacy journals can lack either reserve; no automatic pruning or repair is performed.

Startup runs SQLite page/foreign-key checks and bounded semantic validation of identity, message digest/envelope, delivery incarnation, clock watermark, task state and event vocabulary before changing submitted mail to ambiguous. Schema v2 also validates bounded dispositions, source/successor relationships and migration witnesses. The semantic scan and recovery update share a write transaction. Invalid rows refuse open without that recovery update; they are not silently normalized. This is not an authenticated tamper-proof ledger or an exhaustive schema validator.

First initialization commits schema, version and all workspace metadata in one SQLite transaction. Coordinator registration and rotation publish the private credential before committing its actor/event transaction. Publication writes a same-directory private staging file, syncs it, renames with Linux `RENAME_NOREPLACE`, then syncs the parent. A pre-commit failure/crash leaves no new registry authority; an orphan candidate alone cannot authenticate. A committed registration/rotation whose reply is lost is reconciled through the operator-only actor snapshot, which reports the current incarnation's expected credential path, never its secret. Rotation is not blindly retried and the snapshot does not assert that a file damaged externally is usable.

Private publication first validates the containing private directory and syncs its entry in the parent, including reuse after a prior attempt. This is a local entry boundary, not recursive synchronization of arbitrary caller-created ancestors.

Operator keys, actor credentials and peer bindings share the no-clobber publisher: at most 4096 body bytes and refusal at 1024 existing entries in the publication directory. Failed staging files and superseded credentials are retained, not automatically deleted; they count toward the bound. The record is authoritative, not the mere presence of a candidate file. These rules have local error and process-kill tests, not host power-loss, every-filesystem or every-fsync-failure qualification.

Supervisor startup checks/provisions its operator key under the store ownership lock, before creating/opening the database file or running SQLite recovery. Any existing database name with a missing key refuses without replacement. With no database, only `owner.lock` and optionally a valid final `operator.key` may be present: generate only when the key is absent, otherwise reuse and sync it. Unknown/pending entries, sidecars and retained actor files refuse fresh initialization. The initial guard's failure leaves existing database/sidecar bytes unopened; after it passes, normal store opening can have effects. A later retained-key recheck before listening does not roll back those effects or exclude external same-user changes. See [operator-key policy](operator-key-policy.md).

Exact `bind-peer` reuse revalidates current actor/credential identity and expected binding content before finishing synchronization. It and operator-key startup retain the read descriptor and compare named identity, private same-owner/single-link metadata and exact bytes after sync; failures do not return success or create replacement authority. Ordinary client and peer-bridge reads remain bounded and read-only, with no fsync/publication. A matching binding can be reused at the 1024-entry publication cap because it creates nothing. Invalid contents refuse before their private-file sync. This does not establish operator-token authenticity, arbitrary same-user exclusion, physical power loss or repair of missing/damaged credentials. Direct store and offline migration paths do not require an operator key and do not activate a listener.

### Implemented retained current-credential recovery

`coord inspect-staged-credential` and `coord recover-credential` require canonical `staging_id`, `actor` and `incarnation` UUIDs. They are operator-only IPC commands, not peer tools. The candidate is resolved only as `state/credentials/.pending-STAGING_UUID`; the destination is only `state/credentials/ACTOR_UUID-INCARNATION_UUID.json`. No arbitrary path or secret string is accepted through the command. Inspection requires staging; recovery may reconcile a final-only retry. If both names exist, recovery refuses even when bytes match, preserving both without choosing a winner.

The complete file must be a private same-owner regular single-link UTF-8 file of at most 4096 bytes. A strict schema-1 envelope carries workspace, actor, incarnation and credential; duplicate/unknown fields, malformed IDs or token shape refuse. The peer bridge and recovery share this parser. The envelope must match the requested identity, and the credential hash must authenticate that exact current SQLite actor/incarnation. An old rotation or complete file from an uncommitted registration is not recoverable authority. An absent random secret cannot be reconstructed from its hash. A retained file is neither a registry row nor evidence that a runtime still owns that identity.

The existing supervisor lock remains held. A SQLite immediate transaction validates current authority and excludes registry writers across the filesystem-only callback. No actor, message, task, disposition, clock or event rows change; no new credential/incarnation or binding is generated. Inspection uses the same reservation but only rechecks descriptors/identities and returns `status=staged_verified`, `published=false` and metadata/paths, without fsync or rename. It is not a promise that the candidate will still qualify later.

Recovery retains the candidate and credentials/state-directory descriptors, synchronizes the credentials-directory entry in its state parent, then the file and credentials directory, rechecks, renames relative to the retained directory with no-replace semantics, syncs the directory and rechecks bytes/inodes/private metadata before success. The original file inode and bytes are preserved; this move needs no new publication slot and works at the 1024-entry admission cap. It does not reclaim space. Repeated failures retain staged or final files and never return a success receipt. A later transaction-completion error cannot undo the filesystem move; exact identity retry is required rather than assuming rollback.

A successful receipt contains `status=credential_recovered`, `published=true`, the exact workspace/actor/incarnation and final path, `authority_changed=false` and `staging_provenance=not_established`. The last field remains explicit even for an absent-staging retry: an intact current final file proves final identity, not which vanished staging UUID produced it. Receipts contain no credential value or token hash. Valid existing peer bindings can resume with that same current credential; recovery does not rebind a rotated session. Operator-key recovery, incomplete-file salvage, arbitrary secret import, reviving old authority, cleanup and archive restoration are not implemented by these commands. Named syscall/substitution controls are not physical power-loss or hostile same-user exclusion guarantees.

### Exact-lease operator release

`release-stopped` requires the task UUID, recorded owner actor UUID, recorded lease incarnation UUID, canonical positive decimal generation and explicit stopped attestation. The store compares that complete tuple under its write transaction before changing `active` to `released` and recording one event. It never releases whichever lease happens to own a task ID now. The old task-ID-only CLI/wire request is intentionally refused; update operator clients with the fields from `inspect-task`.

An exact retry while that same tuple remains `released` returns the same attributed receipt without another event or clock update, including after restart or journal saturation. A later claim advances generation; an old release then refuses rather than affecting the new owner. Completed tasks are not releasable. Matching the actor's current credential incarnation would be wrong after rotation: the attestation concerns the incarnation stored on the lease. The operation is operator-only and does not signal, stop, inspect or prove the state of any Hermes/OMP process. Expiry alone never supplies the attestation.

### Implemented immutable message disposition

`coord dispose-message` is operator-only. Its strict `disposition` request carries `sender`, `message`, `delivery_incarnation`, `decision_id`, `action`, `successor_id`, `ttl_seconds`, `reason` and `attest_outcome_unknown`. Identifiers are canonical UUIDs; reason is nonblank, control-free and at most 512 UTF-8 bytes. Only the exact stored ambiguous delivery can be decided. The attestation accepts an unknown outcome; it neither proves prior non-execution nor authorizes a tool approval or task-lease takeover.

`close` requires no successor/TTL. `reoffer` requires an unused sender-scoped successor ID distinct from the original and TTL 1–3600 seconds. It copies the original recipient, kind, payload and correlation into a new queued row with a new creation/expiry time. The route must currently be allowed. Pending, total-row, encoded-payload and journal budgets still apply; all retained originals count toward global storage. One decision per original and one source per sender-scoped successor are enforced. Decisions use globally unique decision UUIDs. The API never changes or deletes an accepted decision or rewrites the original message body/state.

Decision, optional successor, clock and `message.closed`/`message.reoffered` notice commit together. Exact accepted request replay returns the original receipt without updating the clock, running expiry or emitting another event. Changed fields conflict. An ACK committed first makes disposition refuse; a disposition committed first makes ACK of that original refuse. A successor is acknowledged only by its own sender/message key and observed delivery incarnation. These rules prevent stale ACK confusion, not duplicate external execution.

Decided originals remain `ambiguous` as retained evidence but no longer consume pending-message or settlement-debt slots. Close uses the original settlement slot and remains possible at a full reserved journal; re-offer needs new admission headroom. A decision cannot free physical storage. Receipt `outcome=unknown` and `process_stopped=not_established` preserve that distinction. Receive, exact inspection and duplicate-send reconciliation expose an `origin` with `authorization=operator_reoffer_not_new_sender_intent`; exact inspection/duplicate reconciliation also expose an original's disposition. The delivery envelope stays schema version 1; database version 2 is independent.

### Implemented offline v1-to-v2 migration

Fresh stores initialize v2. Normal service open refuses v1 with an explicit migration requirement and refuses unknown versions. `migrate-store --state DIR --workspace DIR --archive-id UUID` is a local owner operation, not a peer/IPC command: it requires existing private store files, the same exclusive owner lock and matching canonical workspace. It launches no agents or coordinator service and does not perform submitted-to-ambiguous recovery.

The migrator verifies and syncs a v1 archive, then compares all legacy logical tables and allocated sequences under the live database's write transaction before adding the disposition/migration tables and committing version 2 plus the archive-ID/hash witness. A stale reused archive refuses rather than legitimizing newer source rows. DDL failure rolls back the schema/version/witness and retains the archive. A committed retry requires that same intact original archive and witness even after source advancement; it never fabricates a replacement archive. Existing actor/message/task/clock/event rows are unchanged by migration. Restarting service afterward performs ordinary delivery recovery separately.

Migration is additive, not restoration or credential provisioning. A copied archive is not safe to activate: the runtime lock and private credential files are not in it. There is no down-migration or supported automatic rollback to v1. Physical power loss, hostile same-user races and all migration syscall cut points are not qualified by the local tests.

## Transport and negotiation

The proposed local supervisor uses a private Unix-domain socket in a user-owned runtime directory. Check file ownership/mode and peer credentials where available. Bind each admitted connection to an actor or a human client and to its allowed operations. Same-user hostile processes remain outside the initial security guarantee.

Messages use UTF-8 newline-delimited JSON with an enforced encoded-byte limit before allocation. Embedded newlines are JSON-escaped. Reject malformed JSON, duplicate object keys in authority-bearing messages, unsupported schema versions, unknown command fields, missing required fields and oversized bodies. Advertise protocol version and capabilities before accepting work; incompatible clients cannot silently downgrade.

Use opaque UUID strings for workspace, actor, incarnation, task and message identities. Upstream runtime session IDs remain opaque strings. Sequence and generation counters cross JSON/Luau boundaries as canonical unsigned decimal strings to avoid double-precision loss. Store them in checked integer form internally; never perform arithmetic on pane IDs.

## Proposed accepted-message envelope

Illustrative normalized record, not a currently supported request:

```json
{
  "schema_version": 1,
  "workspace_id": "afc51e18-e2b5-4688-a008-b1de62505b01",
  "message_id": "e0cd21bb-a606-4078-8338-0ed912263c59",
  "sender_actor_id": "c0d117c6-cbfd-4adb-9b88-0436725cae09",
  "recipient_actor_id": "7b0622ed-45e1-4c11-a1ea-d52072d8b67d",
  "kind": "task_offer",
  "correlation_id": "388dff45-d9c7-4389-b14d-08a824a8ec54",
  "causation_id": null,
  "sequence": "1",
  "created_at": "2026-10-23T00:00:00Z",
  "expires_at": "2026-10-23T00:05:00Z",
  "payload": {"text": "Review the parser changes; do not edit files."}
}
```

The supervisor derives sender identity and timestamps from the authenticated connection, not from caller-controlled fields. It validates recipient membership and routing policy. The stored envelope contains the derived sender; a client request must not override it. `kind` is a closed enum: context, task_offer, task_result, question and answer for the initial version. It is not a command or approval enum.

A unique `(workspace_id, sender_actor_id, message_id)` key identifies retries. Reusing that key with different canonical content is a conflict. A bounded hop count and causation chain detect relay loops. TTL is checked before admission and delivery; process-local deadlines use monotonic time, while persisted expiry is reconciled conservatively after clock changes.

## Delivery state is not execution state

```text
validated -> queued -> dispatching -> submitted -> consumed -> result_recorded
                 |          |             |
              expired    ambiguous     ambiguous

validation failure -> rejected (never admitted to the queue)
```

- `queued`: committed to the supervisor store, subject to the declared host/disk durability assumptions.
- `dispatching`: a durable attempt exists before the external runtime call.
- `submitted`: the runtime accepted a request under its actual contract.
- `consumed`: the runtime/agent hook explicitly acknowledged receipt at a safe boundary.
- `result_recorded`: a correlated terminal result was committed; it does not prove task correctness.
- `ambiguous`: a connection failed after an external effect may have happened; no automatic resubmission without reconciliation/idempotency support.

Adapters that cannot observe `consumed` must never emit it. A native OMP idle observation or a queued Hermes steer is not consumption. Deduplicated mailbox admission is achievable; exactly-once inference/tool execution across unrelated runtimes is not promised.

## Proposed storage model

Use one supervisor-owned SQLite database, foreign keys enabled, WAL with a declared synchronization setting, and transactional migrations. Do not share Hermes' session database. Core tables:

| Table | Key and material fields |
| --- | --- |
| workspaces | id, canonical root, policy version, created time |
| actors | actor id, workspace id, runtime kind, current incarnation, state, capabilities |
| bindings | actor/incarnation, upstream session id, transport identity, qualified pane binding |
| messages | message id with sender-scoped uniqueness, recipient, kind, payload, expiry, state |
| attempts | message id, attempt id, target incarnation, upstream request id, outcome, timestamps |
| tasks | task id, owner actor, lease generation, deadline, status, bounded result reference |
| events | workspace sequence, entity id, event kind, redacted payload, time |
| attachments | opaque id, relative controlled path, digest, size, media type, expiry |

Never store provider secrets, auth tickets, sudo responses or hidden reasoning in this journal. Runtime transcripts remain in the runtime; retain only bounded coordination content and user-visible references. TSP recordings are opt-in debugging data and may contain sensitive text.

## Transaction boundaries

1. Admission checks routing and capacity; a transaction inserts the message and event. Reply `queued` only after commit.
2. Dispatch claims a queued record via a conditional update, inserts its attempt and commits. The external runtime call occurs outside the SQLite transaction.
3. Persist the observed response with the same attempt ID. On a crash between the call and commit, recover to `ambiguous`, not queued.
4. Receiver acknowledgement is idempotent, bound to the expected actor/incarnation and message. Duplicate acks do not create new state transitions.
5. Task completion compares actor, generation and active lease in one conditional transaction; zero affected rows means stale ownership.

No database transaction can atomically commit an arbitrary runtime's model/tool action. If the adapter supplies a proven idempotency key or lookup, reconcile through it; otherwise require a human decision. Never hold a database lock while waiting on a model or UI.

## Implemented operator observation and replay

`coord snapshot`, `coord events`, `coord inspect-message` and `coord inspect-task` are operator-only read operations. Each reads within one SQLite transaction and neither advances the clock nor expires, delivers, acknowledges or reassigns work. Results are JSON; no raw peer text is copied to a shell or privileged input channel. These commands are not model tools.

The metadata snapshot contains `workspace_id`, a fresh `observation_epoch` for this store-open lifetime, `event_watermark`, `event_replay_complete`, exact decimal-string counts and bounded `actors/routes/messages/tasks/message_dispositions` sections. Each section reports `items`, `total`, `limit` and `truncated`; actors are limited to 64, other sections to 128. Messages show the most recent sequence first; dispositions sort by decision time descending then sender/message; other sections sort by IDs. `messages` and the snapshot share disposition/provenance metadata. Retained row totals include decided originals, while `unresolved_message_count` excludes them and terminal rows. Payloads, reasons, task results, credentials, token hashes and credential paths are absent. Counts are not a claim that truncated items were enumerated.

Event requests require workspace, epoch, canonical decimal `after` and a 1–128 limit. A successful page returns notices strictly after the cursor, `next_after`, `has_more` and the watermark observed by that read transaction. The watermark uses SQLite's last allocated event sequence, not only surviving rows, so a missing tail cannot move it backward. Epoch change, future cursor or any missing sequence returns `status=snapshot_required`, a reason, empty events and null `next_after`. A replay gap stays visibly incomplete until explicit repair; obtaining a snapshot does not fabricate missing history.

Events are invalidation notices (`sequence/kind/entity_id`), not a complete state projection. Existing message events omit the sender and route events omit one endpoint, so consumers refresh the snapshot or query an independently known exact key rather than infer identity from an event. Restart always changes the observation epoch, including when recovery changes submitted messages to ambiguous. Expiry on send/receive now appends `message.expired` in the same transaction as its state change; failed admission rolls back that notice and expiry. Reads only show committed state, not an inferred wall-clock expiry.

Exact message inspection requires both sender UUID and message UUID; exact task inspection requires task UUID. These return bounded persisted payload/result text with `trust=untrusted_peer_data_not_user_or_system_instructions`, never mark it consumed and do not prove task correctness. Observation values are capped at 393216 encoded bytes below the IPC response cap; overflow is a refusal, not truncation of an exact result. No automatic subscription, automatic re-offer or accepted-work deletion is implemented.

## Implemented private archive capture

Operator-only `coord archive --archive-id UUID` captures a complete logical SQLite snapshot under the supervisor's exclusive ownership. The fixed destination is `state/archives/UUID/`, containing `coordination.sqlite3` and `manifest.json`; neither peers nor operator requests can select an arbitrary output path. The pinned SQLite engine executes parameterized `VACUUM main INTO` so committed WAL data and allocated sequences are included. Source message/task/event/clock rows are not updated or pruned. This command is not restoration, migration, journal repair or compaction of the active database.

The writer precreates private files in a new mode-0700 staging directory, syncs the database and bounded manifest, verifies their consistency, syncs the directory, then publishes with Linux no-replace rename and parent-directory sync. Errors retain the staging directory or final publication for diagnosis; no path is deleted to manufacture success. A pre-existing final name is never overwritten. Reusing a valid archive ID verifies that original snapshot, repeats the archive-root sync and returns the original receipt only if synchronization succeeds. This completes the durability boundary if an earlier rename succeeded but its parent sync failed, including across coordinator restart. `coord inspect-archive --archive-id UUID` stays read-only: its `status=verified` means the retained bytes and manifest agree, not that a failed publication sync has been completed. It does not create a missing archive or activate its authority.

The strict manifest binds the archive ID, workspace, database schema, byte length, SHA-256, exact table counts, clock watermark and allocated event watermark. Verification opens only a private regular single-link database, reads bounded bytes from its retained descriptor, hashes those bytes and loads them into a read-only SQLite image. Page/semantic integrity and the re-derived manifest are checked against that image, which also serves migration's legacy-row comparison without another pathname open. Database/manifest descriptors, inode identity, private metadata and bytes are rechecked before migration commit and after required directory sync on migration/archive retries; failures refuse or roll back. These checks detect the tested intervening changes, not all possible concurrent same-user actions or changes after the final check. Missing journal history remains missing. Hash agreement establishes consistency, not an authenticated signature or task correctness.

`coord inspect-staged-archive --staging-id UUID --archive-id UUID` verifies only the selected complete `.pending-UUID` bundle and returns `staged_verified`/`published=false` without syncing. `coord recover-archive` also requires `--database-sha256 HEX`; it verifies the expected digest, syncs retained file descriptors and bundle directory, rechecks, uses root-descriptor-relative no-replace rename, syncs root and rechecks the same descriptors at the final name. Before-rename failures retain staging; post-rename failures retain final publication. Both names present refuse. Missing staging with exact final ID/hash is retryable final-identity reconciliation, not proof of historical staging association. No archive scan, deletion, manifest repair, source mutation or runtime activation occurs; moving an existing entry does not require spare capture capacity. No universal same-user race exclusion follows.

Both v1 and v2 archives remain inspectable. V2 manifests additionally bind `message_dispositions` and `schema_migrations` counts; those fields are absent in legacy v1 manifests. Strict comparison to re-derived fields prevents an omitted v2 count from being accepted as an empty table.

Admission permits at most eight retained archive-root entries, a 256 MiB database, a 4096-byte manifest and 512 MiB aggregate retained file lengths. It conservatively reserves the full per-database cap plus manifest allowance before a new capture. Unknown private regular files and failed staging consume this budget; unsafe entries refuse. Image deserialization temporarily holds two database-sized encoded buffers (up to 512 MiB combined) plus SQLite/query overhead. Allocation errors propagate, but maximum-size RSS/latency and kernel OOM behavior are unqualified. These are application bounds, not a filesystem quota or total-memory/transient-I/O limit. Synchronous capture/verification can delay other requests; a client timeout means inspect the same archive ID, not assume rollback or generate a replacement ID.

Archives contain sensitive coordination payloads/results, actor metadata and credential hashes. They exclude operator keys and credential/binding files, do not copy Hermes data and do not provision a second writer. Do not share or activate them as ordinary diagnostics. There is no supported restore command or archive deletion/retention service yet.

## Implemented complete operator state pages

`coord state-index` returns protocol schema 1, database schema 2, workspace and store-open epoch, an opaque lowercase SHA-256 `revision`, exact decimal-string counts for seven collections, clock/message/event allocation watermarks and event replay completeness. It does not create a persistent snapshot, new database table or restore capability. Only the operator may use this API; credentials/token hashes are not returned. Payloads, results and actor metadata can still be sensitive.

`coord state-page --workspace UUID --epoch UUID --revision HEX --section NAME --offset N --limit N` enumerates `actors`, `routes`, `messages`, `tasks`, `dispositions`, `migrations` or `events`. Actors/tasks sort by ID, routes by sender/recipient, messages/events by sequence, dispositions by sender/message and migrations by version. The ordering is ascending and keys are unique. Offsets are canonical nonnegative decimal strings, at most the selected collection's count; equality yields an empty final page. Limit is 1–128 (CLI default 64, offset default 0). A section name is selected from literal SQL, never interpolated into it.

Each request holds a SQLite read transaction across bounded semantic validation, count/revision calculation and page extraction. Revision domain-separates schema/projection version and hashes typed, length-delimited logical cells from all nine current tables in deterministic order, including metadata, token hashes and allocated sequences. Only the resulting digest is exposed, not secret hash cells. Missing counters for retained records, duplicate/unknown allocation rows, counters behind retained records, unexpected tables and invalid persisted values refuse the response. An unused empty collection can have no allocation row and reports zero. This is logical coherence detection, not a schema attestation, signature, anti-rollback proof or same-user sandbox; identical logical rows can have the same revision after intervening writes. Restart separately changes epoch.

Matching reads return `status=ok`, the same revision/epoch, `items`, exact `total`, `offset`, requested `limit`, `next_offset`, `has_more` and `byte_limited`. Pages return complete records only, capped at 393216 encoded bytes; a byte limit may produce fewer items than requested while `has_more=true`. Use `next_offset`, never offset plus requested limit. Message/task items share exact-inspection projection, including untrusted-data attribution and disposition/re-offer provenance; migration entries are retained witnesses, not renewed archive verification. Counts, generations and timestamps retain decimal-string precision. Event entries remain invalidation notices.

Changed revision or epoch returns `status=restart_required`, `reason=state_changed` or `epoch_changed`, empty items and null continuation. This is not a successful complete walk. Discard every previously collected section/page and obtain a new index; do not take the new revision in that refusal as permission to continue an old offset. No page expires/delivers/ACKs work or changes tasks, events or clock. These requests synchronously validate/hash the whole bounded store; maximum-size time/RSS is unqualified and sustained writes can prevent a complete walk. Consumers need bounded retries or an operator-arranged quiet interval. A completed walk represents one persisted logical state, not runtime liveness, physical durability, an executable backup or current authorization for later mutation.

## Proposed retention and incremental projection

Incremental state projection and automatic subscriptions remain proposed beyond the complete operator walk above. Existing event fields cannot reconstruct all state, and the walk does not make them sufficient. Consumers verify incarnation/generation before applying actions. Private SQLite archives exist, but automatic restoration and journal compaction remain unimplemented. Any future compactor must preserve accepted-work evidence and atomic cursor/state consistency.

Keep accepted unconsumed messages and pending human requests until resolved or explicitly expired. Evict diagnostic history and already-terminal bounded outputs first. When storage budget is reached, reject new admission before acknowledging it; never delete accepted work to manufacture a healthy queue. Attachment paths are supervisor-owned references, not arbitrary filesystem reads supplied by peers.

## Recovery and migration

On startup, acquire exclusive supervisor ownership, check schema compatibility, and reconcile nonterminal attempts against runtime evidence. Migrations make a backup of the coordination database in a quiescent window, test the full chain on a copy, and refuse unsupported future versions. Rollback cannot restart unsafe execution with missing policy fields: stop admissions and restore the prior approved database/binary pair or stay read-only.

Kill-point tests cover before/after every commit and external dispatch. Include disk-full, busy database, partial NDJSON, oversized input, duplicate IDs, stale incarnation, replay gaps and clock movement. [Verification](verification.md) owns the test layers; [configuration](configuration.md) owns proposed limits.
