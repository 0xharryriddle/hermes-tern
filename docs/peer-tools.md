# Hermes peer mailbox tools

Status: implemented, deterministic process/IPC tests passed, and pinned Hermes directory-loader/registry dispatch exercised in an isolated home. No provider-backed agent turn or personal plugin installation has been performed. This is an explicit physical-session binding, not automatic canonical-session continuity or an OS sandbox.

## Components and authority

- `plugins/hermes-tern-peer/` is a standalone Python Hermes plugin, distinct from the Luau Tern window plugin in `plugins/hermes-tern/`. It uses `register(ctx)` and registers only six narrow tools.
- `crates/supervisor/src/peer.rs` owns the bounded Rust pipe bridge. `coord bind-peer` provisions a private binding; `peer-tool --binding FILE` accepts one bounded JSON document on stdin followed by EOF.
- `crates/store` validates the credential against the current workspace, actor, incarnation, runtime kind, profile-home path and physical session ID. Validation occurs inside the single supervisor before every bound operation. The plugin never reads Hermes' database.
- The model can choose only permitted message/task fields. Executable path, binding path, calling session, home, caller actor and credentials are not tool arguments. The `sender` field on ACK identifies the already-observed message's sender, not the caller.
- Host `session_id` is taken from a handler keyword supplied by Hermes. `get_hermes_home()` resolves the host profile namespace for that call. `ctx.profile_name`, model arguments and session environment variables are not substitutes for identity.

The process containing a plugin and arbitrary same-UID code remain trusted. An agent with unrestricted filesystem/shell tools could read or change same-user files; this implementation does not claim to prevent that. Tool schemas and private files prevent cross-wiring at the tool boundary, not malicious same-user execution.

## Tools

| Tool | Operation | Limit and meaning |
| --- | --- | --- |
| `tern_peer_status` | Bound identity and permitted recipient IDs | At most 64 routes; no liveness assertion |
| `tern_peer_send` | Send context/task_offer/task_result/question/answer | Canonical caller-chosen UUID, 1–16384 UTF-8 text bytes, TTL 1–3600 seconds; queued is not consumed |
| `tern_peer_receive` | Pull pending messages | At most 8; explicit untrusted-data attribution, no injection hook |
| `tern_peer_ack` | Acknowledge an exact sender/message delivery | Incarnation-bound, idempotent; does not execute contents |
| `tern_task_claim` | Claim a task lease | Canonical task UUID and bounded TTL; expiry is not proof the former writer stopped |
| `tern_task_complete` | Record the owned task's result | Exact generation, bounded text, no claim of correctness |

The send tool defaults kind to `context` and TTL to 300 seconds. Results are JSON strings with `ok`, a trust label and structured `result`; failures use bounded diagnostic codes. A launched bridge failure is conservatively `outcome_unknown`. There is no automatic retry, approval, route grant, actor registration, shutdown, runtime launch or shell operation exposed as a model tool.

## Operator provisioning

Provision only for a session and home whose physical identity the operator has independently verified. A canonical/logical session ID from another API must not be substituted. The local tests use synthetic IDs and therefore do not prove that a particular live session has been bound correctly.

1. Build/install `bin/hermes-tern` to a trusted absolute executable path. The plugin refuses a symlink, group/world-writable executable or one not owned by the current user/root. Cargo may produce mode 0775 under umask 0002; use a deliberately installed private executable rather than weakening this check.
2. Start the foreground supervisor with an absolute private state directory and canonical workspace. Do not use Hermes' own database/home as coordination state.
3. Register the existing actor using `coord register --kind hermes`, with `--profile` equal to the canonical absolute Hermes home and `--session` equal to the verified physical session ID; supply the actual model/provider labels. This records an operator attestation, not runtime liveness.
4. Call `coord bind-peer --actor UUID --hermes-home HOME --physical-session ID --attest-binding`. The supervisor returns a private `binding_file` path; the token is not printed. Repeating the same binding validates exact content, finishes file/directory synchronization and rechecks retained bytes/inodes before returning the same path, without allocating another file. Persistent sync failure still refuses after coordinator restart. Existing conflicting or unsafe content refuses rather than overwriting; normal bridge reads remain read-only.
5. Explicitly allow each direction using `coord route --sender UUID --recipient UUID --allow`. Routes otherwise deny. A bound peer cannot provision bindings or grant routes.
6. Only with installation authorization, place the standalone package under the chosen Hermes home's plugin directory and enable `hermes-tern-peer` using the supported Hermes plugin workflow. No personal install or gateway restart is part of this repository's verifier.

Example settings fragment, with placeholders replaced by operator-selected values:

```yaml
plugins:
  enabled:
    - hermes-tern-peer
  entries:
    hermes-tern-peer:
      settings:
        binary: /absolute/trusted/hermes-tern
        bindings:
          verified-physical-session-id: /absolute/private/state/bindings/actor-incarnation.json
```

Merge into existing configuration; never replace other enabled plugins/settings with this illustrative fragment. The adapter reads settings through `ctx.get_config`; it has no ambient binary/binding fallback. Enabling tools in a running session follows Hermes' own reload/new-session lifecycle and is not automatic.

## Rotation, failure and recovery

Compression may rotate the physical session ID. An unknown ID denies even when the logical conversation continues. Do not auto-rebind, infer an identity from peer text, or read Hermes SQLite directly. Operator reprovisioning after verifying the new runtime identity is required; preserving canonical actor continuity across compression is an open gate.

`coord rotate --actor UUID` revokes the old credential/incarnation; existing bindings fail. An operator may provision a new binding for the same verified physical session, then update only that selected configuration entry. Rotation is not session migration or proof that an old worker stopped.

If the current credential file is absent but an intact original survives under an explicitly identified private staging name, operator `inspect-staged-credential`/`recover-credential` can verify and finish that file's publication without changing registry authority or the existing binding. The complete envelope and secret hash must still match the current workspace/actor/incarnation; old rotations, uncommitted candidates and partial files refuse. Recovery shares the bridge's strict envelope parser and is never a model tool. It cannot reconstruct a lost secret, repair an operator key, revive a rotated binding or authorize a new physical session. Follow [the credential runbook](operations.md#recover-an-intact-retained-current-actor-credential); never pass secret text in tool arguments or logs.

The Python adapter sends only anonymous-pipe data to a fixed argv, clears ambient credentials from the child's environment, caps request/response bytes, discards stderr, and kills/reaps only its own bridge child on timeout. Rust bounds stdin time/bytes, private-file reads, IPC frames and server deadlines. Unknown fields, duplicate keys, unsafe files and invalid result shapes fail closed. Neither a zero process exit nor arbitrary valid JSON is treated as successful mailbox work.

Receive commits `submitted` before returning. Lost responses are not rolled back or blindly redelivered; after supervisor restart those deliveries become `ambiguous`. Acknowledgment must follow an actually observed message. For an unknown send outcome inspect the existing sender/message key with operator tooling before considering retransmission; never mint a new UUID to conceal ambiguity. An explicit operator decision may close accounting or create a new-ID re-offer while preserving the original. The returned successor's `origin.authorization=operator_reoffer_not_new_sender_intent` is provenance, not new peer authorization; old ACKs refuse after disposition. Database v2 does not change the v1 delivery envelope or the six tool schemas. Full state-projection replay and fault-matrix qualification remain open; [operator recovery](operations.md#decide-an-ambiguous-message-explicitly) gives the decision workflow.

## Reproducible checks

```sh
./verify.sh
python3 -B -m unittest discover -s tests -p 'test_peer_*.py' -v
python3 -B scripts/smoke_peer_plugin.py \
  --hermes-root /absolute/pinned/hermes-checkout \
  --python /absolute/hermes-python
```

The mandatory gate builds the real binary before Python process tests. The optional smoke requires the pinned Hermes revision and dependencies; it copies only this package into a fresh scratch home, calls the real directory loader/registry, executes status/send/read/ack through the Rust supervisor, checks missing/forged/rotated identity refusal, and unloads registrations. It launches no Hermes gateway or agent, makes no inference call and never scans personal plugins. Retained outputs stay in the printed private scratch directory.

Still open: actual provider-backed tool calls, live two-agent collaboration, canonical-session continuity, host-isolation-mode qualification, OMP and native-window acceptance. [STATUS](STATUS.md), [ROADMAP](ROADMAP.md) and [tasks.json](tasks.json) own current evidence and completion state.
