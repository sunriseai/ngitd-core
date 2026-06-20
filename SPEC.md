# ngitd Core Product And Technical Specification

Status: draft for rebuild planning  
Audience: product, engineering, future core implementation team  
Current product name: Prism  
Current implementation names: `ngitd`, `.ngit/`  
Target implementation: language-agnostic; Rust is plausible but not required

## One-Line Product Contract

After a repo is initialized, Prism can remember meaningful software-change
events locally, bind them to evidence and externally supplied judgement, and
preserve enough lineage that a human, UI, or supervisor can later query what
happened.

## Product Thesis

Modern software work increasingly happens through short-lived AI sessions,
local edits, tool calls, generated patches, and external review or test output.
Git remains the source of code history, but it does not answer the operational
questions around a change:

- Why was this change captured?
- What repo state did Prism see at the time?
- Which files, staged diff, worktree diff, branch, and HEAD were involved?
- What checks, reviews, or human notes were attached?
- What evidence statuses were attached?
- Were any evidence refs missing or malformed?
- Which outside system or person supplied higher-level judgement?
- What terminal disposition was recorded for the draft?
- Did the operator include an override reason as context?
- What durable lineage should survive after the working tree changes again?

`ngitd core` is the local code-editing memory and evidence/lineage substrate for
those questions. It should be small, durable, inspectable, and boring. It should
record facts, evidence, and externally supplied judgements; it should not be the
planner, worker manager, product UI backend, strategic evaluator, or autonomous
delivery brain.

## Product Boundaries

### In Scope

`ngitd core` owns:

- Repo-local initialization.
- `.ngit/` directory layout and schema versioning.
- Git repository fact collection.
- Repo event observation.
- Change capture into draft records.
- Intention and rationale annotation storage.
- Evidence attachment to drafts and accepted changes.
- Neutral evidence status rollups.
- Recorded accept and reject dispositions for drafts.
- Durable lineage records.
- Local CLI and TUI for inspection and operator action.
- Machine-readable JSON output for supervisors and higher-level Prism services.
- Storage integrity checks and repair recommendations.
- Local read APIs for repo-local inspection and future UI integration.

### Out Of Scope

`ngitd core` does not own:

- Prompt-to-work planning.
- Multi-step operating sessions.
- Agent worker lifecycle.
- ACP, Codex, Claude, or other model-specific control protocols.
- App runtime management or preview containers.
- Deployment orchestration.
- Hosted services.
- Long-term project registry across repos, except optional discovery helpers.
- Product-specific control packs beyond generic evidence/check policy.
- Strategic or product-level evaluation, except when captured as external
  evidence or annotation.
- Acceptance authority for broad delivery workflows beyond recording a draft's
  terminal disposition.

Higher-level Prism layers may use core records, but they must not require hidden
chat state or model prose to make the core record store meaningful.

## Key Product Concepts

### Repository

A Git working tree initialized with `ngitd core`. The repository remains the
source of code. `.ngit/` is the source of local operational memory.

### Repo State Snapshot

A structured view of Git state at a point in time:

- repo root
- branch
- HEAD
- HEAD parent count
- changed files
- staged diff digest
- worktree diff digest
- optionally staged diff summary
- optionally worktree diff summary
- recent commit summaries

The snapshot is evidence. It is not a judgment.

### Repo Event

An observed transition in repository state. Examples:

- initial observation
- changed working tree
- staged change
- commit
- merge commit
- branch change
- metadata change

Events are append-only operational memory. Events may trigger capture, but an
event is not itself an accepted mutation.

### Captured Change

A draft record created from repo state. It is the unit of reviewable local
change. The current implementation calls this a draft mutation. The rebuilt
core may call it `change`, `draft`, or `mutation`, but it must preserve the
contract:

- captured from a concrete repo state
- has a stable id
- records trigger and capture key
- records changed paths and digests
- can receive evidence
- can be accepted or rejected

### Evidence

Structured material attached to a captured change. Evidence can come from:

- deterministic command checks
- test/lint/typecheck output
- AI review adapters
- human notes
- supervisor decisions from a higher layer
- runtime proof supplied by another component

Core validates the evidence envelope. It does not need to understand every
external system that produced it.

### Annotation

Structured intention, rationale, notes, or imported context attached to a draft,
accepted change, rejected draft, evidence item, or lineage record. Annotations
are first-class records because the product goal is to preserve code-editing
truth beyond Git, not only pass/fail check output.

Annotation examples:

- human-supplied intent
- agent-supplied rationale
- supervisor note
- imported prompt/session/task reference
- summarizer output from an external producer

Annotations do not affect evidence rollups by default. If an annotation should
gate a change, it must also be submitted as evidence. Core stores supplied or
imported intent and rationale, but it never requires inference to generate them.

### Evidence Rollup

A neutral local summary of attached evidence statuses:

- how many evidence records are attached
- which statuses are present
- which evidence refs are missing or malformed
- which producer kinds contributed evidence
- which artifacts are available

The rollup is mechanical. It is useful for filtering, badges, and audit, but it
should not translate `failed` into "revise", no evidence into "needs review", or
all passing checks into "accept". Those are workflow meanings, and they should
arrive as evidence or annotations from an external reviewer, policy engine, UI,
or supervisor.

### Terminal Disposition

The recorded outcome for a captured draft:

- `accepted`
- `rejected`

Terminal disposition is an operator or supervisor action recorded by core. It is
not proof that core itself evaluated the broader correctness of the change.

### Lineage

An append-only accepted/rejected history record that links:

- original draft id
- accepted mutation/change id, if accepted
- repo state at capture
- repo state at acceptance
- evidence ids and evidence statuses
- evidence rollup and external judgement context
- override reason, if any
- timestamps

Lineage is the durable answer to "what happened, what evidence existed, which
judgement sources were recorded, and what terminal disposition was applied?"

## Core User Stories

### Initialize A Repo

As a developer or supervisor, I can run `ngit init` in a Git repo and get a
local `.ngit/` store with default policy, schema version, and ignored runtime
scratch space.

Acceptance:

- Fails outside a Git repo unless explicit `--force-non-git` is added in a
  future version.
- Creates required durable directories.
- Writes schema manifest and default policies.
- Does not modify source files except optional `.gitignore` update when the
  operator explicitly requests it.
- Is idempotent.

### Watch Meaningful Repo Events

As an operator, I can run `ngit watch` and see meaningful repo state changes as
they happen.

Acceptance:

- Emits an initial state.
- Detects staged changes.
- Detects commits.
- Detects merge commits.
- Detects branch changes.
- Ignores `.ngit/` by default.
- Emits JSON lines with stable event fields when `--json` is set.
- Does not write drafts unless capture is enabled.

### Capture A Change

As an operator or supervisor, I can capture the current repo delta as a draft.

Acceptance:

- `ngit capture` creates a draft from current repo state.
- `ngit watch --capture` can auto-capture from configured triggers.
- Duplicate capture of the same branch, HEAD, changed files, staged digest, and
  worktree digest returns or references the existing draft.
- Empty captures are either disallowed or clearly marked as empty depending on
  policy.
- Captures are stored under `.ngit/changes/drafts/` or an equivalent stable
  path.

### Attach Evidence

As a check runner, review adapter, human, or supervisor, I can attach evidence
to a draft.

Acceptance:

- Evidence has an id, type, status, summary, producer, timestamps, and optional
  findings.
- Core validates required envelope fields.
- Evidence can include raw output references without forcing raw logs into the
  main JSON payload.
- Evidence status is one of `passed`, `failed`, `needs_review`, or `blocked`.
- Evidence can be listed and shown independently.

### Explain Evidence State

As a human, I can inspect a draft and understand what evidence has been attached
and what outside judgement, if any, has been recorded.

Acceptance:

- Evidence state is derived from stored evidence and unresolved evidence refs.
- The evidence and judgement context separates deterministic evidence,
  externally supplied review evidence, unresolved evidence, and recorded
  external judgements.
- The TUI and CLI both show the same evidence and judgement context.
- JSON output includes all fields required for another supervisor to reproduce
  the evidence rollup.

### Record Accept Or Reject

As an operator or supervisor, I can record an accept or reject disposition for a
draft and preserve lineage.

Acceptance:

- Accept writes an accepted change record.
- Accept preserves evidence refs and durable evidence artifacts.
- Accept writes a lineage record.
- Accept may include an operator-supplied override reason as context.
- Rejected drafts preserve evidence and rejection reason.
- Accepted and rejected drafts no longer appear as open drafts.

### Audit Later

As a future human or supervisor, I can inspect prior lineage and understand what
happened without the original chat session.

Acceptance:

- `ngit history` lists accepted/rejected changes.
- `ngit show <id>` resolves unambiguous ids across drafts, accepted changes,
  evidence, and lineage.
- `ngit doctor` detects missing evidence, malformed records, and terminal
  accepted/rejected records without lineage.
- Records are durable JSON or another explicitly inspectable format.

## Product Principles

### Local First

The core store is repo-local and usable offline. Networked services may enrich
evidence, but local records must remain meaningful without them.

### Git Adjacent, Not Git Replacement

Git owns source history. `ngitd core` owns operational memory around source
change. It should use Git facts directly rather than invent a parallel VCS.

### Evidence Before Interpretation

Core records facts and evidence before anyone interprets them. AI-generated
summaries, human review, supervisor decisions, and policy-engine output may be
evidence, but raw AI prose or hidden service state is not authority.

### Append-Oriented By Default

Events, evidence, and lineage should prefer append-only behavior. Mutable
records are allowed only for explicitly operational state such as lock files,
watch cursors, or cache.

### Inspectable By Humans

The store should be readable in ordinary tools. A user should be able to inspect
JSON files and understand the shape without a server.

### Stable For Supervisors

Every TUI/CLI behavior with product meaning must have equivalent structured
output. Higher-level Prism services should rely on stable JSON contracts, not
terminal scraping.

### External Judgement, Local Record

Strategic evaluation belongs outside core. Core can store a supervisor decision,
AI review, policy exception, or human judgement with provenance, but it should
not infer that authority from annotations, chat history, or unstored model
state.

### Small Core, Strong Boundary

The core is a foundation. It should expose extension points for checks,
reviews, and supervisors without owning their full lifecycle.

## Repository Layout

The current core uses this repo-local layout. Migration support imports older
`.ngit/mutations/*` and `.ngit/checks/*` records into the newer
`changes/` and `evidence/` locations.

```text
.ngit/
  manifest.json
  policies/
    capture.json
    evidence-rollup.json
    evidence.json
  events/
    event-*.json
  changes/
    drafts/
      draft-*.json
    accepted/
      change-*.json
    rejected/
      draft-*.json
  evidence/
    records/
      evidence-*.json
    artifacts/
      evidence-*/
        <artifact files>
  annotations/
    annotation-*.json
  lineage/
    lineage-*.json
  runtime/
    locks/
    watch/
    logs/
    cache/
```

Durable directories:

- `policies`
- `events`
- `changes`
- `evidence`
- `lineage`

Ephemeral directories:

- `runtime`
- `runtime/locks`
- `runtime/watch`
- `runtime/logs`
- `runtime/cache`

## Storage Requirements

### Format

Initial recommendation: line-delimited or pretty JSON records on disk.

Rationale:

- Easy to inspect.
- Easy to diff.
- Language-neutral.
- Low ceremony for early rebuild.
- Compatible with migration from the current implementation.

Future options:

- SQLite index over file records.
- Content-addressed blobs for raw logs or large diffs.
- Event log plus projections.

If a Rust implementation uses SQLite internally, it should still preserve an
exportable and inspectable record contract.

### Atomic Writes

Record writes must be crash-resistant:

- write to temporary file in same directory
- fsync where practical
- atomic rename into place
- avoid partial JSON records

### Locking

Core must prevent conflicting writes:

- process-level lock under `.ngit/runtime/locks/`
- stale lock detection
- lock metadata includes pid, command, hostname, and timestamp

Read commands should tolerate concurrent writes by ignoring incomplete temp
files and retrying when necessary.

### Durable Artifacts

Evidence records are immutable after write and stored by evidence id. Accepted
and rejected records reference evidence ids; they do not rewrite evidence
ownership. Evidence artifacts that are required for later audit must live under
durable evidence storage, not under `.ngit/runtime/`. Runtime logs may be used
as temporary command output while a producer is running, but committed evidence
must copy or move referenced artifacts into `.ngit/evidence/artifacts/<evidence-id>/`
before the record is considered valid.

### Record Identity

Ids should be stable, sortable where useful, and prefix-resolvable:

- `event-YYYYMMDDTHHMMSSZ-<short-random>`
- `draft-YYYYMMDDTHHMMSSZ-<short-random>`
- `change-YYYYMMDDTHHMMSSZ-<short-random>`
- `evidence-YYYYMMDDTHHMMSSZ-<short-random>`
- `annotation-YYYYMMDDTHHMMSSZ-<short-random>`
- `lineage-YYYYMMDDTHHMMSSZ-<short-random>`

Prefix resolution must fail closed when ambiguous.

### Hashing

Core should compute hashes for:

- staged diff
- worktree diff
- capture key
- evidence payload
- policy snapshot
- optional file contents for changed paths

Hashes should include algorithm prefixes such as `sha256:<hex>`.

## Core Schemas

These are product-level schemas, not final JSON Schema files.

### `.ngit/manifest.json`

```json
{
  "schema_version": 1,
  "store_format": "ngit-core-files-v1",
  "created_at": "2026-06-19T00:00:00Z",
  "updated_at": "2026-06-19T00:00:00Z",
  "repo": {
    "vcs": "git",
    "root": "/absolute/path",
    "init_head": "sha-or-null"
  },
  "core": {
    "implementation": "ngitd-core",
    "implementation_version": "0.1.0"
  }
}
```

### Capture Policy

```json
{
  "schema_version": 1,
  "mode": "manual_only",
  "triggers": [],
  "allow_empty_capture": false,
  "dedupe": {
    "enabled": true,
    "fields": [
      "branch",
      "head",
      "head_parent_count",
      "changed_files",
      "staged_digest",
      "worktree_digest"
    ]
  }
}
```

Allowed modes:

- `manual_only`
- `auto`

Allowed triggers when mode is `auto`:

- `on_stage`
- `on_commit`
- `on_merge`

### Evidence Rollup Policy

```json
{
  "schema_version": 1,
  "required_status_buckets": ["passed", "failed", "needs_review", "blocked"],
  "missing_evidence_status": "unresolved",
  "include_counts": true,
  "include_producer_kinds": true,
  "include_artifact_availability": true
}
```

Evidence rollup policy controls how core summarizes attached evidence. It must
not map raw evidence statuses into workflow actions such as `accept`, `revise`,
or `needs_review`. If a team wants those meanings, an external policy engine or
supervisor should submit its conclusion as evidence, for example with type
`supervisor_decision` or `external`.

### Repo State Snapshot

```json
{
  "schema_version": 1,
  "captured_at": "2026-06-19T00:00:00Z",
  "repo_root": "/absolute/path",
  "branch": "main",
  "head": "sha-or-null",
  "head_parent_count": 1,
  "changed_files": [
    {
      "path": "src/app.ts",
      "index_status": "M",
      "worktree_status": " "
    }
  ],
  "recent_commits": [
    {
      "sha": "abc123",
      "subject": "Initial commit"
    }
  ],
  "staged_digest": "sha256:...",
  "worktree_digest": "sha256:...",
  "dirty": true
}
```

### Event Record

```json
{
  "schema_version": 1,
  "event_id": "event-20260619T000000Z-a1b2c3d4",
  "event_type": "repo_changed",
  "created_at": "2026-06-19T00:00:00Z",
  "signals": ["on_stage"],
  "repo": {
    "branch": "main",
    "head": "abc123",
    "staged_digest": "sha256:...",
    "worktree_digest": "sha256:..."
  },
  "changed_files": [],
  "related": {
    "draft_id": "draft-..."
  },
  "detail": {}
}
```

### Draft Change Record

```json
{
  "schema_version": 1,
  "draft_id": "draft-20260619T000000Z-a1b2c3d4",
  "status": "draft",
  "created_at": "2026-06-19T00:00:00Z",
  "updated_at": "2026-06-19T00:00:00Z",
  "capture": {
    "source": "ngit-core",
    "trigger": "on_stage",
    "capture_key": "sha256:...",
    "deduped_from": null
  },
  "repo_snapshot": {},
  "changed_paths": ["src/app.ts"],
  "summary": "Captured change from on_stage affecting 1 file.",
  "annotation_refs": [],
  "evidence_refs": [],
  "readiness": {
    "computed_at": "2026-06-19T00:00:00Z",
    "policy_hash": "sha256:...",
    "evidence_summary": {
      "passed": [],
      "failed": [],
      "needs_review": [],
      "blocked": [],
      "unresolved": []
    },
    "unresolved_evidence": [],
    "deterministic_action": "no_evidence",
    "review_action": null,
    "final_action": "no_evidence",
    "override_required": false,
    "override_targets": [],
    "summary": "No evidence has been attached."
  }
}
```

Core creates deterministic draft summaries from Git facts. Human, agent,
supervisor, imported, or inferred intent/rationale is stored as annotation
records and referenced from the draft.

Compatibility note: the current Rust record field is still named `readiness`,
and the context still includes `deterministic_action` and `final_action`. Current
code sets those compatibility fields to neutral evidence states such as
`no_evidence` or `evidence_present`; they are not workflow instructions.

### Annotation Record

```json
{
  "schema_version": 1,
  "annotation_id": "annotation-20260619T000000Z-a1b2c3d4",
  "owner": {
    "type": "draft",
    "id": "draft-..."
  },
  "type": "intent",
  "status": "supplied",
  "summary": "Improve README clarity.",
  "body": "This change updates the README to explain the blackbox goal.",
  "producer": {
    "kind": "human",
    "name": null,
    "version": null
  },
  "created_at": "2026-06-19T00:00:00Z",
  "updated_at": "2026-06-19T00:00:00Z",
  "refs": [],
  "payload_hash": "sha256:..."
}
```

Annotation types:

- `intent`
- `rationale`
- `human_note`
- `agent_note`
- `supervisor_note`
- `imported_context`
- `external`

Annotation statuses:

- `supplied`
- `inferred`
- `disputed`
- `superseded`
- `unknown`

### Evidence Record

```json
{
  "schema_version": 1,
  "evidence_id": "evidence-20260619T000000Z-a1b2c3d4",
  "owner": {
    "type": "draft",
    "id": "draft-..."
  },
  "type": "command_check",
  "status": "passed",
  "summary": "Unit tests passed.",
  "created_at": "2026-06-19T00:00:00Z",
  "producer": {
    "kind": "command",
    "name": "npm test",
    "version": null
  },
  "command": {
    "argv": ["npm", "test"],
    "exit_code": 0,
    "duration_ms": 1234
  },
  "findings": [],
  "artifacts": [
    {
      "kind": "stdout",
      "path": ".ngit/evidence/artifacts/evidence-.../stdout.log",
      "digest": "sha256:...",
      "size_bytes": 1234,
      "truncated": false,
      "original_path": null
    }
  ],
  "payload_hash": "sha256:..."
}
```

Evidence types:

- `command_check`
- `ai_review`
- `human_note`
- `supervisor_decision`
- `runtime_proof`
- `external`

Core-required statuses:

- `passed`
- `failed`
- `needs_review`
- `blocked`

### Evidence And Judgement Context

Evidence and judgement context can be embedded in draft, accepted, rejected, and
lineage records. It should be recomputable from stored records where possible,
but persisted for audit stability.

```json
{
  "schema_version": 1,
  "computed_at": "2026-06-19T00:00:00Z",
  "policy_hash": "sha256:...",
  "evidence_summary": {
    "passed": ["evidence-..."],
    "failed": [],
    "needs_review": [],
    "blocked": [],
    "unresolved": []
  },
  "unresolved_evidence": [],
  "deterministic_action": "evidence_present",
  "review_action": null,
  "final_action": "evidence_present",
  "override_required": false,
  "override_targets": [],
  "summary": "One passing evidence item is attached."
}
```

Compatibility note: the current Rust implementation still names this structure
`DecisionContext`. The product direction is to treat it as a neutral evidence
and judgement context, not as core-owned workflow judgement.

### Accepted Change Record

```json
{
  "schema_version": 1,
  "change_id": "change-20260619T000000Z-a1b2c3d4",
  "draft_id": "draft-...",
  "status": "accepted",
  "accepted_at": "2026-06-19T00:00:00Z",
  "accepted_by": {
    "kind": "human",
    "id": null
  },
  "repo_snapshot_at_capture": {},
  "repo_snapshot_at_acceptance": {},
  "changed_paths": ["src/app.ts"],
  "annotation_refs": ["annotation-..."],
  "evidence_refs": ["evidence-..."],
  "decision_context": {},
  "override_reason": null
}
```

### Rejected Change Record

```json
{
  "schema_version": 1,
  "draft_id": "draft-...",
  "status": "rejected",
  "rejected_at": "2026-06-19T00:00:00Z",
  "rejected_by": {
    "kind": "human",
    "id": null
  },
  "reason": "Superseded by a cleaner change.",
  "repo_snapshot_at_capture": {},
  "annotation_refs": ["annotation-..."],
  "evidence_refs": ["evidence-..."],
  "decision_context": {}
}
```

### Lineage Record

```json
{
  "schema_version": 1,
  "lineage_id": "lineage-20260619T000000Z-a1b2c3d4",
  "event_type": "change_accepted",
  "change_id": "change-...",
  "draft_id": "draft-...",
  "created_at": "2026-06-19T00:00:00Z",
  "repo": {
    "capture": {},
    "acceptance": {}
  },
  "changed_paths": ["src/app.ts"],
  "annotation_refs": ["annotation-..."],
  "evidence_refs": ["evidence-..."],
  "decision_context": {},
  "decision": {
    "action": "accepted",
    "reason": "One passing evidence item is attached."
  },
  "override_reason": null,
  "links": {
    "event_ids": ["event-..."],
    "supersedes": [],
    "related_changes": []
  }
}
```

For rejected drafts, lineage uses the same envelope with `event_type:
"change_rejected"` and no accepted `change_id`:

```json
{
  "schema_version": 1,
  "lineage_id": "lineage-20260619T000000Z-b2c3d4e5",
  "event_type": "change_rejected",
  "change_id": null,
  "draft_id": "draft-...",
  "created_at": "2026-06-19T00:00:00Z",
  "repo": {
    "capture": {}
  },
  "changed_paths": ["src/app.ts"],
  "annotation_refs": ["annotation-..."],
  "evidence_refs": ["evidence-..."],
  "decision_context": {},
  "decision": {
    "action": "rejected",
    "reason": "Superseded by a cleaner change."
  },
  "override_reason": null,
  "links": {
    "event_ids": [],
    "supersedes": [],
    "related_changes": []
  }
}
```

## Lifecycle

### Initialization

```text
operator
  -> ngit init
  -> find Git root
  -> create .ngit directories
  -> write manifest and policies
  -> validate store
```

### Manual Capture

```text
operator/supervisor
  -> ngit capture
  -> collect repo snapshot
  -> compute capture key
  -> check dedupe
  -> write draft
  -> compute evidence rollup
  -> show draft summary
```

### Watch Observation

```text
watcher
  -> poll repo state
  -> compare fingerprint
  -> emit transient event
```

Watch may emit transient human or JSONL events. `watch --capture` persists event
records under `.ngit/events/*`, matches event signals against capture policy,
optionally creates drafts, and optionally runs configured built-in checks.

### Evidence Attachment

```text
check/review/human/supervisor
  -> submit evidence envelope
  -> validate owner exists
  -> validate status/type
  -> write evidence
  -> update or recompute draft evidence rollup
```

### Acceptance

```text
operator/supervisor
  -> ngit accept <draft-id>
  -> load draft and evidence
  -> recompute evidence and judgement context
  -> record override reason if supplied
  -> collect current repo snapshot
  -> write accepted change
  -> preserve evidence refs and artifacts
  -> write lineage
  -> remove draft from open set
```

### Rejection

```text
operator/supervisor
  -> ngit reject <draft-id>
  -> load draft and evidence
  -> require rejection reason
  -> collect current repo snapshot
  -> write rejected change record
  -> preserve evidence refs and artifacts
  -> write rejection lineage
  -> remove draft from open set
```

## CLI Specification

The CLI must be scriptable and stable. Human output is a view over the same
records exposed by JSON.

### Required Commands

```text
ngit init
ngit status
ngit watch [--capture] [--once] [--interval <seconds>] [--json]
ngit capture [--trigger manual] [--intent <text>] [--json]
ngit annotation add <owner-id> --type <type> --body <text>
ngit annotation list <owner-id>
ngit annotation show <annotation-id>
ngit evidence add <draft-id> --file <path>
ngit evidence run <draft-id> [--timeout-seconds N] [--max-output-bytes N] [--json] -- <command...>
ngit drafts
ngit show <id>
ngit accept <draft-id> [--override-reason <text>]
ngit reject <draft-id> [--reason <text>]
ngit history
ngit lineage <id>
ngit doctor
ngit schema export --dir <path>
ngit migrate
ngit tui
ngit serve [--bind <addr>] [--once] [--token <token>] [--allow-non-loopback] [--require-auth-for-read]
```

Compatibility aliases may exist:

```text
ngit list-drafts
ngit show-draft <id>
ngit accept-draft <id>
ngit reject-draft <id>
ngit list-lineage
```

### Command Behavior

`ngit status`

- Shows current Git facts.
- Shows whether `.ngit` is initialized.
- Shows open draft count and latest evidence state.

`ngit watch`

- Human mode prints compact event rows.
- JSON mode emits JSON lines.
- Watch emits transient events by default.
- `--capture` persists event records and writes drafts only when policy matches.

`ngit capture`

- Creates a draft from current repo state.
- Returns existing draft if deduped.
- Shows changed paths and evidence state.
- `--intent <text>` may create an `intent` annotation with human provenance.

`ngit annotation`

- Adds, lists, and shows intention/rationale/context records.
- Annotation records explain changes but do not affect evidence rollups by
  default.

`ngit evidence run`

- Runs a local command.
- Captures exit code, duration, stdout/stderr artifact refs.
- Applies timeout and output-size bounds.
- Marks truncated artifacts explicitly.
- Attaches evidence to the target draft.
- Does not accept the draft automatically.

`ngit show <id>`

- Resolves id across draft, change, lineage, event, or evidence records.
- Fails if ambiguous.
- Shows evidence and judgement context when available.

`ngit lineage <id>`

- Resolves a lineage id, accepted change id, or rejected draft id.
- Shows accepted or rejected lineage when available.

`ngit accept`

- Records an accepted terminal disposition.
- May include an override reason supplied by the operator.
- Writes accepted change and lineage.

`ngit reject`

- Requires a rejection reason.
- Writes rejected change and rejection lineage.

`ngit doctor`

- Validates schema, references, JSON parseability, evidence ownership, and
  lineage completeness.
- Detects accepted and rejected terminal records without lineage.
- Reports repair suggestions.
- Does not rewrite records unless `--repair` is explicitly implemented later.

`ngit schema export`

- Writes JSON Schema files generated from the core record types.
- Exports schemas for manifest, policies, snapshots, events, drafts,
  annotations, evidence, compatibility evidence context, terminal records,
  lineage, and doctor reports.

`ngit migrate`

- Imports legacy `.ngit/mutations/*` records into `.ngit/changes/*` when the
  legacy record already matches the new typed contract.
- Imports legacy `.ngit/checks` records into `.ngit/evidence/records` when the
  record matches the evidence contract.
- Writes a migration report and records malformed legacy files as skipped.

`ngit serve`

- Runs a foreground local HTTP adapter over the same core APIs.
- Exposes read endpoints for status, drafts, annotations, history, records,
  lineage, doctor, watch, and evidence artifacts.
- Exposes mutation endpoints for capture, evidence add/run, accept, and reject.
- Mutation endpoints require a bearer token when served over HTTP.
- Non-loopback binds require explicit opt-in.
- File and artifact routes must reject path traversal.
- Does not become the source of truth; `.ngit/` remains authoritative.
- Is not a full daemon lifecycle, repo registry, or cross-repo index.

## TUI Specification

The TUI is part of core because the product promise depends on humans being
able to inspect local evidence without a web app. It should be terminal-native,
keyboard-driven, and fully usable offline.

### TUI Goals

- Make the current repo state understandable at a glance.
- Show open drafts and their evidence state.
- Let users inspect evidence, external judgement, and evidence context.
- Let users record accept/reject dispositions or request more evidence.
- Preserve trust by making raw artifacts reachable.
- Avoid becoming a full Prism workflow console.

### TUI Non-Goals

- No chat interface.
- No multi-agent session orchestration.
- No app preview UI.
- No project planning board.
- No hidden model calls.

### Primary Screens

#### Overview

Purpose: answer "what is happening in this repo?"

Sections:

- repo identity: root, branch, HEAD
- dirty state: changed file count, staged count, unstaged count
- open drafts with evidence status badges
- latest events
- latest accepted changes
- storage health summary

Actions:

- `c` capture current change
- `w` toggle watch mode
- `enter` open selected item
- `d` open drafts
- `h` open history
- `?` help
- `q` quit

#### Draft Detail

Purpose: answer "can I trust this change?"

Sections:

- draft summary
- changed paths
- capture trigger and timestamp
- repo snapshot
- evidence status rollup
- external judgement context
- evidence list
- raw record path

Actions:

- `a` accept
- `r` reject
- `e` add evidence
- `R` run configured evidence command
- `o` accept with operator-supplied override reason
- `v` view raw JSON
- `b` back

#### Evidence Detail

Purpose: answer "what does this evidence prove?"

Sections:

- evidence status and type
- producer
- command/review metadata
- findings
- artifact refs
- raw payload hash

Actions:

- `l` open log artifact in pager
- `v` view raw JSON
- `b` back

#### History / Lineage

Purpose: answer "what happened before?"

Sections:

- accepted changes
- rejected drafts
- filters by path, status, date, trigger
- selected lineage summary

Actions:

- `/` search/filter
- `enter` open lineage
- `p` filter by path
- `v` raw JSON

#### Doctor

Purpose: answer "is the local memory healthy?"

Sections:

- missing refs
- malformed records
- accepted/rejected terminal records without lineage
- evidence without owners
- schema version warnings

Actions:

- `r` rerun doctor
- `v` raw report

### TUI Interaction Requirements

- Must work in 80x24 terminals.
- Must not require a mouse.
- Must degrade gracefully without color.
- Must expose raw file paths for records and artifacts.
- Must never record accept/reject without an explicit keystroke confirmation.
- Must treat override reason as operator-supplied context, not as a core
  readiness gate.
- Must be able to run read-only mode.
- Must support JSON export from selected item.

### TUI State Model

The TUI should be a view over core APIs:

- It should not hold authority state only in memory.
- It should refresh from disk after writes.
- It should survive external CLI writes.
- It should display stale-data warnings when a selected draft changes on disk.

## API Boundary

Even if the first rebuild exposes only CLI/TUI, implementation should define an
internal core API that can later support bindings and a live daemon:

```text
init(repo_path) -> InitResult
status(repo_path) -> RepoStatus
watch(repo_path, options) -> stream RepoEvent
capture(repo_path, options) -> Draft
list_drafts(repo_path) -> DraftSummary[]
show_record(repo_path, id) -> Record
add_evidence(repo_path, draft_id, evidence) -> Evidence
add_annotation(repo_path, owner_id, annotation) -> Annotation
list_annotations(repo_path, owner_id) -> AnnotationSummary[]
compute_evidence_context(repo_path, draft_id) -> EvidenceContext
accept(repo_path, draft_id, options) -> AcceptedChange
reject(repo_path, draft_id, options) -> RejectedChange
history(repo_path, filters) -> History
doctor(repo_path) -> DoctorReport
```

Bindings may include:

- CLI
- TUI
- local HTTP API
- MCP tools
- desktop app bridge
- language SDKs

The core library should not import UI, model-provider, or supervisor-specific
packages.

### Daemon And Live API Posture

The current `ngit serve` shape is a foreground local adapter over one repo. It
is useful for development, local UI experiments, and smoke tests, but it is not
yet a daemon product.

A future `ngitd` daemon should be a live query and coordination layer over one
or more repo-local stores:

- discover or register repo/worktree roots
- watch selected repos and refresh derived indexes
- serve read APIs for status, drafts, evidence, lineage, history, and artifacts
- accept writes only through the same core mutation APIs used by CLI/TUI
- expose subscriptions or polling-friendly cursors for UI refresh
- keep `.ngit/` as the authoritative record for each repo/worktree
- rebuild its cache/index from `.ngit/` after restart

The daemon may maintain an index for speed and cross-repo queries, but the index
is derived state. Evidence, annotations, terminal dispositions, and lineage must
be committed to the relevant repo-local store before they count as durable
memory.

For a UI that needs one or many repos or worktrees, the preferred shape is:

```text
UI
  -> ngitd daemon API
       -> repo registry or explicit repo roots
       -> per-repo core APIs
       -> per-repo .ngit/ stores
       -> optional derived cross-repo index
```

This keeps the UI responsive and avoids direct filesystem polling in the UI,
while preserving the core invariant that records are inspectable without a
server. If the daemon is offline, CLI/TUI and direct `.ngit/` inspection should
still work.

The daemon should not own higher-level judgement. A policy engine, AI reviewer,
human reviewer, or supervisor UI can submit judgement as evidence or annotation
with provenance; `ngitd` records and indexes it.

## Extension Points

### Evidence Producers

Evidence producers can be built-in or external.

Minimum producer contract:

- receives draft id or repo snapshot
- returns evidence envelope
- may provide artifact paths
- must not mutate core records directly unless using core API

Examples:

- command check runner
- AI review adapter
- human note editor
- rationale/intent importer
- external summarizer that emits annotation records
- CI import adapter
- external supervisor

### External Policy Producers

Core should avoid plugin mechanisms that turn it into the policy engine. If a
future external policy producer is integrated, it should behave like any other
evidence producer:

- producer version and input refs are recorded
- output judgement is stored as evidence or annotation with
  producer provenance
- producer failure is stored as failed or unresolved evidence, not as hidden
  control flow

### Migration

The rebuild should include an explicit migration posture from current `.ngit/`:

- read old `.ngit/mutations/drafts`
- read old `.ngit/mutations/accepted`
- read old `.ngit/mutations/rejected`
- read old `.ngit/checks`
- read old `.ngit/lineage`
- preserve old ids where practical
- write migration report
- do not destroy old records

Migration can initially be read-only import plus report generation.

## Language-Agnostic Implementation Guidance

### Rust-Friendly Shape

If implemented in Rust, recommended crates/classes of concern:

- Git integration through CLI `git` first, library later if needed.
- Serialization through strongly typed record structs.
- JSON Schema generation from types if practical.
- TUI through Ratatui or an equivalent terminal UI library.
- File locking through a cross-platform lock crate.
- Atomic writes through temp files and rename.
- Error handling with typed error enums and user-safe messages.

Rust is attractive for:

- reliable single-binary distribution
- fast file scanning
- strong schema modeling
- terminal UI quality
- low runtime dependency burden

Risks:

- faster iteration may be harder than Python while product concepts settle
- Git edge cases still require careful tests
- schema migrations require discipline

### Non-Rust Requirements

Any implementation language must provide:

- single-command install path eventually
- stable JSON contracts
- atomic file writes
- cross-platform terminal behavior
- good subprocess handling
- robust Git command integration
- strong test coverage around record lifecycle

## Error Handling

Core errors should be precise and recoverable:

- not a Git repo
- `.ngit` not initialized
- unsupported schema version
- malformed JSON
- ambiguous id prefix
- missing draft
- evidence owner missing
- unresolved evidence refs
- lock held by active process
- Git command failed
- dirty state changed since draft capture

User-facing errors should state:

- what failed
- why it matters
- what to do next

Machine-facing errors should include:

- stable error code
- message
- record id, if relevant
- path, if relevant

## Security And Privacy

Core is local-first but still needs explicit safety constraints:

- Do not upload records or source by default.
- Do not inline secrets from env vars into evidence.
- Store command argv, exit code, and log refs; avoid storing full environment.
- Redact known token patterns from captured command output where practical.
- Keep `.ngit/runtime/` suitable for `.gitignore`.
- Keep durable evidence artifacts out of `.ngit/runtime/`.
- Make network-using evidence producers explicit.
- Treat external AI review as evidence with provenance, not hidden authority.

## Performance

Targets for ordinary repos:

- `status`: under 200 ms for small repos, under 1 s for large repos.
- `watch` poll: configurable, default 1 s.
- `drafts` and `history`: under 500 ms for hundreds of records.
- TUI refresh: no visible lag for normal record counts.

Potential indexes:

- latest drafts
- path-to-change refs
- evidence owner refs
- event chronological index

Indexes must be rebuildable from durable records.

## Testing Strategy

### Unit Tests

- id generation
- prefix resolution
- evidence rollup policy loading
- schema validation
- annotation ownership and payload hashing
- atomic write behavior
- capture key dedupe
- evidence rollup calculation

### Git Integration Tests

- clean repo
- untracked files
- staged changes
- unstaged changes
- rename/copy status
- commit detection
- merge commit detection
- branch changes
- `.ngit/` ignored from changed files

### Lifecycle Tests

- init idempotence
- manual capture
- capture with intent annotation
- watch emits transient JSONL events
- watch capture persists event records and captures when policy matches
- annotation attach/list/show
- evidence attach
- accept records terminal disposition and preserves evidence context
- accept can include operator-supplied override reason
- reject preserves evidence and writes lineage
- doctor detects missing lineage for accepted and rejected terminal records

### TUI Tests

- smoke render in 80x24
- no panic on empty repo state
- no overlap in main screens
- keyboard flows for capture, accept, reject
- stale record warning when draft changes externally
- read-only mode prevents writes

### Migration Tests

- current `.ngit/mutations` to new `changes`
- current `.ngit/checks` to new `evidence`
- current lineage compatibility
- malformed legacy records reported but not destroyed

## MVP

The smallest rebuild worth shipping:

- `ngit init`
- `ngit status`
- `ngit watch`
- `ngit watch --capture`
- `ngit capture`
- `ngit annotation add/list/show`
- `ngit drafts`
- `ngit show`
- `ngit evidence run`
- `ngit accept`
- `ngit reject`
- `ngit history`
- `ngit doctor`
- durable evidence artifact storage
- durable intention/rationale annotation storage
- basic TUI overview and draft detail
- JSON storage with atomic writes
- capture/evidence rollup policies
- tests for full draft-to-lineage lifecycle

## V1

V1 adds:

- richer TUI history/evidence/doctor screens
- compatibility importer for current `.ngit`
- external evidence producer contract
- external annotation producer contract
- JSON Schema files
- read-only local API or library bindings for higher-level Prism

Daemon or cross-repo indexing design is intentionally beyond V1 unless a UI
needs it immediately.

## Explicit Non-Requirements For MVP

- no multi-repo registry
- no desktop app
- no MCP server
- no always-on daemon lifecycle
- no repo/worktree registry
- no cross-repo derived index
- no AI provider integration in core
- no worker orchestration
- no app runtime containers
- no deployment records
- no operating sessions
- no hosted sync
- no strategic judgement engine in core

## Open Product Questions

- Should the user-facing command be `ngit`, `ngitd`, `prism-core`, or another
  name?
- Should the daemon be a later `ngitd` binary, a mode of `ngit serve`, or a
  Prism-owned process that embeds the core library?
- Should `.ngit/` be committed, ignored, or partially committed by default?
- How much diff content should be stored versus referenced by digest?
- Should core include built-in AI review evidence shape but no provider, or
  should AI review be purely external?
- Should `intent` exist in core as an unknown/enriched field, or only in higher
  Prism layers?
- What is the minimum repo/worktree registry contract needed for a future UI?
- What is the exact migration promise for existing `.ngit/` records?

## Recommended Decision

Build `ngitd core` as a small local evidence and memory engine with a strong
CLI/TUI and stable record contracts. Keep it independent from Prism's
higher-level workflow features. Let Prism use core as its source of repo-local
change memory, but do not let core become the autonomous planner, strategic
judge, or worker runtime.

Treat `ngit serve` as a foreground adapter. If a product UI needs live querying
across one or many repos/worktrees, build a later daemon/API layer that indexes
repo-local `.ngit/` stores and serves derived views without becoming the
authoritative store.

The core product should be judged by one question:

Can a human, UI, or supervisor open a repo tomorrow, inspect `.ngit/`, and
understand what changed, what evidence existed, which judgement sources were
recorded, and what terminal disposition was applied?
