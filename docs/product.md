# Product and requirements

Status: proposed product scope; the SDK bootstrap is implemented. Requirement IDs and task mapping are canonical in [tasks.json](tasks.json).

## Purpose

Let a developer use Hermes as a first-class native agent in Tern, rather than watching an unrelated terminal UI inside a pane. Preserve Hermes' runtime strengths while letting several independently addressable Hermes and OMP agents collaborate under the developer's control.

The primary actor is the developer operating a local workspace. Hermes and OMP are workers with explicit authority, not autonomous owners of every pane. The first target platform is Linux desktop with local Tern; remote hosts, browser and mobile parity are later work.

## Core workflows

### A single Hermes session

The user opens Hermes from Tern, selects an existing profile or a dedicated one, and sees which runtime will own the session. Prompt text, streaming output, tool activity and approval requests appear natively. Resume, interrupt and model changes use Hermes' actual control interface. Unsupported features are named, not simulated.

### Parallel independent actors

The user opens two Hermes actors in separate approved workspaces. Either actor may propose a task or send context to the other through an explicit mailbox capability. The user can inspect the routing, pause a recipient and deny an action. A child created by Hermes' own delegation remains a child of that runtime, not silently a second peer under the supervisor.

### Mixed Hermes and OMP

An OMP pane can coexist with Hermes. Participation in coordinated work requires opt-in binding and a capability report. A native Tern OMP adapter offers only what AgentCx actually provides; a richer OMP RPC adapter is enabled only after its schema, lifecycle and approval semantics have been measured. Merely finding a pane named `omp` confers no authority to submit prompts.

### Human review and recovery

A completed actor task produces a bounded result with source references and an observable completion state. It does not merge, commit, publish or approve its own output. A disconnected actor stays disconnected/unknown until the actual runtime is reconciled. Reopening a terminal window is not proof a process resumed.

## Requirements

| ID | Requirement | Acceptance boundary |
| --- | --- | --- |
| R-01 | Native Rust Tern surface with readable fallback | Real SDK native render and forced-text run; no false live status |
| R-02 | Explicit Tern plugin entry point | Luau command loads and opens the intended binary without auto-starting agents |
| R-03 | Structured Hermes session control | Create/resume/prompt/stream/cancel via a version-qualified runtime interface |
| R-04 | Preserve approvals and interactive requests | Correlated human responses; timeout, stale request and disconnect fail closed |
| R-05 | Preserve Hermes capabilities | Feature matrix for providers, profiles, skills, memory, tools, plugins, MCP and commands |
| R-06 | Independent parallel Hermes actors | Separate identities and write scopes; no duplicate profile ownership or implicit model drift |
| R-07 | Attributed inter-actor communication | Bounded durable routing with duplicate detection and honest delivery states |
| R-08 | OMP coexistence and opt-in cooperation | Native AgentCx lane verified separately from direct RPC lane |
| R-09 | Recovery without duplicate side effects | Crash-point tests, reconciliation and no blind resubmission after ambiguity |
| R-10 | Security and data minimization | No automatic approval, credential leakage or unsupported sandbox claim |
| R-11 | Observable bounded scaling | Queue, process, memory, disk and inference budgets; measured load envelope |
| R-12 | Reproducible development and release | Pinned dependencies, local/CI parity, docs/task evidence and explicit release holds |

## Success and non-goals

A first connected release succeeds when one real Hermes conversation, including a denied tool request and a cooperative cancellation, works in native Tern and resumes through a verified runtime path. A coordinated release additionally needs two independent Hermes actors and an opted-in OMP actor to exchange bounded, attributable work without approval escalation or ambiguous completion claims.

Not goals: rewrite Hermes in Rust; replace Tern's compositor; implement another provider client; invent a universal agent protocol before adapters exist; run untrusted third-party code safely merely by putting it in a worktree; promise unlimited actors; scrape terminal prose to infer authoritative lifecycle; auto-merge code; or present a benchmark dashboard as the product.

## Release depth

The intended full product covers session UX, actor coordination, interoperability, recovery and operations. It is staged rather than claimed all at once. [Roadmap](ROADMAP.md) assigns every requirement to work; [security](security.md) constrains all stages. Numeric defaults in [configuration](configuration.md) are proposed guardrails, not measured throughput claims.
