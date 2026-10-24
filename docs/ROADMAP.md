# Roadmap

Status: implementation authorized; connected client, local coordination and physical-session peer tools delivered. [tasks.json](tasks.json) is the task-state authority; [STATUS](STATUS.md) is measured evidence. The full roadmap remains open. No calendar estimates or throughput promises are implied.

## Milestones and release gates

| Milestone | Scope | Required tasks | Exit gate |
| --- | --- | --- | --- |
| M0 | Buildable SDK preview, thin launcher package and complete docs baseline | T-01, T-02 | Local gate and real preview render pass; launcher live-window status remains explicit |
| M1 | Qualified Hermes attachment and controlled real session | T-03, T-04, T-05, T-06 | Review approved; real attach/create/prompt/deny/expiry/cancel/resume; malformed-frame/EOF/timeout/shutdown controls; no duplicate owner |
| M2 | Native connected single-actor experience | T-07, T-16 | Interactive renderer lifecycle, capability-aware UI, accessibility, failure states and reproducible local/native autorun |
| M3 | Parallel Hermes peers with durable coordination | T-08, T-09, T-10 | Real agent tool/message provenance; bounded queues; crash ambiguity; independent actors |
| M4 | Opt-in OMP collaboration | T-11, T-12 | Native lane qualified; direct RPC either demonstrated or explicitly deferred as unnecessary |
| M5 | Scale, recovery and releasable distribution | T-13, T-14 | Measured fault/load envelope, security controls, license and release evidence |

M0 does not include a live Hermes or OMP session. Its Lua package exists, but a real-window launcher action remains an open T-03 gate; this cannot be hidden by the native Rust preview smoke.

## Completed workspace split — T-15

The owner-directed [workspace layout](workspace-layout.md) is implemented: `bin/hermes-tern`, `bin/testing`, and six subsystem libraries under `crates/`; no monolithic compatibility crate remains. All 53 Rust test identities were preserved (51 pass, 2 live opt-ins ignored); all 8 packages passed independent locked tests/clippy and both binaries built. The 131 external lock entries are unchanged. Full local verification and scratch installation passed. This organizational change does not close any live-runtime gate. At the split checkpoint the native session-fixture recheck failed in its isolated TTY harness for both executables. The later native smoke below now renders current bytes successfully; T-07 still requires interactive and live acceptance.

## Dependency order

The historical split-regression review was reconciled against its 60 source hashes at T-15 handoff. Later peer changes are a separate tranche and do not inherit that review. T-15 completion does not alter outstanding live/native/peer/OMP gates.

```text
T-01 bootstrap -> T-02 docs
       |             |
       v             v
T-03 real launcher  T-04 owner architecture review + first-slice plan
                         |
                         v
                  T-05 Hermes attach -> T-06 runtime contract -> T-07 native UI
                                              |
                                  T-08 peer provenance -> T-09 durable mail
                                                               |
                                                               v
                                                        T-10 parallel Hermes
                                                               |
                                                        T-11 native OMP
                                                               |
                                                        T-12 RPC evaluation
                                                               |
                                                        T-13 load/failure
                                                               |
                                                        T-14 release
```

The machine ledger holds the exact dependency list, including additional safety prerequisites not drawn in the simplified diagram. A dependent task is not completed merely because its predecessor exists. A deferred optional lane must leave its unmet capabilities clearly unavailable.

## Current checkpoint and remaining slices

Owner explicitly authorized the initial full source/docs commit and push to `https://github.com/0xharryriddle/hermes-tern`, then continued implementation of native Hermes plus opt-in OMP cooperation through Tern SDK. The remote is public and empty; source inventory/credential-pattern screening and current `./verify.sh` pass178Rust/191Python,2liveignored. This publication slice does not complete T-14 or authorize a release, license choice, provider budget, personal runtime changes or automated approvals. Native product work remains the next priority rather than more unrelated storage hardening.

Owner-authorized developer installation completed: exact verified0.1.0 debug binary on login-shell PATH,0755 regular file,and Tern package copy with byte parity. Installed plain/native offscreen preview and discover-only `doctor` pass without inference or session mutation. No Tern daemon was running at install;catalog ready is parsing-only. T-03 actual-window launcher and T-14 release acceptance stay open;peer plugin,provider prompts and OMP dispatch were not installed/run. See [local installation](configuration.md#local-developer-installation).

T-07 [history projection viewer](history-viewer.md) local/native synthetic verification passes178Rust/191Python,2liveignored,0ResourceWarning,24gates,9package pairs and36native cases/80pairs. One canonical read then immutable bounded local paging, no invented server cursor or resume/submit/approval. Five gateway/four UI/one core/one CLI and ten Python methods added. First174/190 pass is historical after page-key/bidi RED/GREEN;second178/191 canonical pass failed a standalone helper fixture;the third full run passes after test-only startup separation. Partial acceptance: complete delayed bounded review reconciled,all9original hashes match,one bidi finding already parent-remediated;not independent post-fix approval. Current pixel review unavailableHTTP402. Native Left/Right/Home/End and a supplemental line-scroll probe differ from PageUp/PageDown PTY-only evidence. Full-history/upstream paging,max-load,mux/accessibility,live/provider/window and release gates remain open. No repeated implementation approval needed.

Historical T-07 local keyboard slice is verified:167Rust/181Python,2liveignored,0ResourceWarning,9locked package pairs and24command gates PASS. Full32case actual Tern synthetic matrix yields68image/layout pairs, including keyboard motion Off/On;four current keyboard images inspected. Review3findings parent RED/GREEN:split flush clocks,layout/reveal invalidation,activation-before-positioning-ACK. Post-ACK ready ring and exact editor-mode projection make native readiness observable without sleeps/retries. Earlier165/181 native-failed attempt and8case subsets retained;no deadline/pin changes. T-07 stays in_progress for full-history,max-load,broader keyboard/OS/provider/window gates. No independent final-source or live acceptance;following155/171 is historical.

T-07 bounded transcript slice is implemented and review parent-reconciled: total retention covers in-place updates; lost prefixes are not spliced; local fold/expand preserves draft and dispatch authority. Receipt fallback, stale same-ID controls, receipt/info generation reset and late-partial races have RED/GREEN regressions. Gateway/UI packages pass 34/16; three transcript and eighteen adjacent native-PTY methods pass. Complete post-remediation autorun passes 155 Rust/171 Python, two live ignored, nine package pairs,24 gates and24 native cases/44 pairs. Four current long-output images inspected across widths/themes. Pre-remediation150/170 remains historical. Key-policy/OMP totals below are also historical; [UX contract](ux.md#current-slice-bounded-long-transcript) owns scope. No independent final-source, full-history, keyboard-only folding, reduced-motion or live/provider acceptance is claimed.

T-09 operator-key startup guard is locally verified after observed missing-key and pre-validation recovery RED. Three Rust/ten Python focused methods and store/supervisor 73/14 tests pass. Complete autorun: 143 Rust/166 Python, two live ignored, nine package pairs and all 24 gates, with 20 native cases/32 image-layout pairs. Bounded final-source review found no concrete defect; parent repeated focused selectors and four supplemental probes, including keyless migration and established-journal barrier checks. The guard runs under owner exclusion before database creation/open, permits only empty bootstrap or complete key-first retry, and refuses missing established keys or retained initialization remnants. Missing-key repair/pinning, restoration/compaction and live acceptance remain open. [Key policy](operator-key-policy.md) owns this slice; the OMP aggregate below is historical.

T-11's local handoff subset is implemented and verified; T-10 and actual-window acceptance dependencies remain open. Four Rust and 17 Python methods pass, including actual Luau execution with 31 synthetic callback scenarios and production-binary race/fsync/SIGKILL controls. Design review is parent-reconciled: source-label and producer-region residuals fixed with RED/GREEN, armed uncertainty derived without callbacks, pane-level queued semantics disclosed rather than conditional-send guarantees. Six scratch mutants fail against a passing pristine harness. Final autorun passes 140 Rust/156 Python, nine package pairs and all 24 gates; the pre-remediation 140/153 run remains historical. No provider calls or personal installation. [Handoff contract](omp-handoff.md) owns this slice.

T-16 is complete. Its historical handoff autorun passed 22 gates with 136 Rust/139 Python, two live ignored, eight package pairs and 20 native cases/32 image-layout pairs. Eight readiness methods include delayed-launch and blocked-status RED/GREEN controls. Review `deleg_89e43e4e` was reconciled as earlier-loop guidance with parent remediation, not independent final-source approval. Only read-only status retries; production/operation/expiry/fault/shutdown deadlines unchanged. Failed and earlier green generations remain retained; no live/release gate promoted.

OMP prerequisite discovery is reconciled for the minimum no-inference transport: parent verified the installed binary against the official18.12.6 release digest, inspected pinned startup/discovery/RPC sources, then exercised ready→negotiate v2→get_state→EOF/exit0 inside a fresh HOME and tested network namespace. State showed zero messages/tools and settled/no pending work. Two failed attempts remain: startup budget initially included slow binary verification,then the probe rejected source-confirmed unsolicited available_commands_update. This closes only the transport prerequisite,not T-11 native connector or T-12 managed RPC/prompt/approval/cancel/chunk acceptance.

The preceding native chat review was reconciled with F1 (P2) parent-remediated, not a clean independent verdict. A waker barrier preserves decoded current/global errors while dropping closed-surface actions; missing-waker/oversize refuses. Historical handoff gate: 135 Rust/119 Python, two live ignored, no ResourceWarning, eight standalone packages passed. Three SDK probes/four scratch mutants and four real-Tern synthetic conversation images were separate evidence. Native UI production Rust remains unchanged in the OMP tranche; the current rebuilt binary passes the existing native matrix without promoting live acceptance.

The preceding native fixture smoke remains reproducible via `scripts/smoke_native.py --fixture`, with `--size 480x760` for the narrow case. Its historical full gate was 129 Rust/101 Python, two live opt-ins ignored, no ResourceWarning, eight standalone packages passed. That tranche changed only the smoke/test harness, not Rust/SDK/credential-review bytes. Its four inspected fixture images and deliberately absent-text failure remain historical evidence. Current rebuilt preview and wide/narrow fixture text/geometry checks passed again; the production-chat input/render evidence above is separate. The earlier TTY diagnostic stays historical rather than being assigned an unproved universal root cause.

The physical-session peer slice implements operator-only binding, six standalone Hermes tools, a bounded Rust bridge and server-side identity/credential fencing. Its isolated real loader/registry and read-only review are historical tranche evidence, not a review of later recovery changes or live-agent acceptance.

T-09 recovers a complete retained credential only for exact current actor/incarnation/hash authority. Fixed paths, strict peer-envelope parsing and SQLite writer exclusion span filesystem sync/publication without registry/message/task/clock/event changes. Exact retry/capacity/lost-reply/restart preserve original bytes/inode; stale/uncommitted/partial/unsafe/conflicting files refuse. Four Rust and nine Python tests remain passing. Ten-source review `deleg_21b276f0` was reconciled without a concrete defect established at its historical source/binary checkpoint; later startup and shared-fixture changes do not inherit it. Parent's 4 Rust/9 Python/3 supplementary repeats are excluded from totals. No missing-secret/operator-key repair, rotation, cleanup, authority activation or live-agent acceptance follows; same-model review is not blanket security/product approval.

The preceding state-projection tranche provides `state-index`/`state-page` for complete operator enumeration of seven retained collections. Exact counts, payload/result attribution, migration/disposition provenance, typed logical-state revision and transaction-consistent pages supplement the existing bounded overview. Clock-only or other state changes and restart invalidate the walk; consumers discard partial assembly. Complete records are bounded by both count and encoded bytes. Ten new Rust and six Python tests pass, including 130-row enumeration, nonempty legacy migration, separate committing WAL writer, precise counters and no-write controls. Historical state-projection full gate: 125 Rust passed plus 2 live opt-ins ignored, 84 Python passed without ResourceWarning; all 8 packages independently pass locked test/clippy and Cargo.lock is unchanged. Historical seven-source review `deleg_11fd2e82` was reconciled without a concrete defect established: source hashes/sizes and parent-built binary matched then; parent repeated 10 Rust/6 Python and 4 supplemental probes, excluded from totals. Whole-store validation/hash scans are synchronous and maximum-size latency/RSS is unqualified; sustained mutations may prevent a complete walk. This is not incremental replay or a restore format. Remaining work includes controlled restore/compaction, operator-key pinning/repair, missing-secret recovery, incomplete-staging salvage, incremental projection/subscriptions and parallel actors; native/live/OMP and physical power-loss gates stay separate.

The preceding owner-private-file tranche remains implemented and passing: three production regressions fixed, four new Rust/seven Python controls, historical full gate 115 Rust/78 Python. Its four-source review `deleg_24ef3b0c` was reconciled with matching source/binary and focused reruns, without a concrete defect established. New state-command wiring changes that supervisor cohort; the earlier review is not coverage of the projection delta. Same-model review and local fixtures do not establish product or blanket security approval.

The plan must resolve U-01 before exposing an attach command. Discover-only and ensure/recover paths differ in side effects. Isolated stdio can validate NDJSON parsing but cannot stand in for attaching to a personal gateway. No live profile/gateway modification is authorized by the current docs.

The delayed pre-v2 design report is reconciled separately from the historical bounded migration/disposition review. Its supplemental legacy-journal, tenth-table and sequence-only stale-archive controls passed on that cohort. The remaining verify-then-reopen issue now has parent RED/GREEN remediation, not a universal concurrent-tampering guarantee. Rechecks do not lock a malicious same-user writer out after validation. The bounded image copy also costs up to two 256 MiB database buffers plus SQLite/query overhead; maximum-size RSS/latency qualification is still open.

Source-rechecked prerequisites for the next streams: T-08 must bind host-supplied physical session IDs to an independently verified actor/profile and deny unknown rotations; T-11 must preserve window/callback lifetime and human confirmation without claiming native durable receipts; T-12 must separate no-inference RPC transport acceptance from network isolation and provider-backed behavior. The late research did not complete these tasks.

## Parallelizable work after review

Protocol fixture capture, native renderer development and read-only documentation review can proceed independently once contracts are frozen. Authoritative task state, supervisor persistence and runtime ownership changes must have one owner. Give editing agents separate worktrees and bounded scopes; peer communication is not permission for concurrent edits everywhere.

## Scope ceilings and deferred work

- No remote-host control, browser/mobile plugin parity or Windows/macOS release claim in the initial local milestone.
- No auto-approval, auto-merge, credentials copied between actors or anonymous broadcast bus.
- No full universal-agent schema until real adapters justify common fields.
- No repeated review fan-out as a substitute for completing the next vertical slice.
- No production/hardened sandbox claim before backend adversarial tests.

Each tranche updates task status, roadmap and current build evidence together. Do not turn `planned` into `done` from document existence alone; the [verification matrix](verification.md) defines the behavioral boundary.
