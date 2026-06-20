# Release Readiness Review

Date: 2026-06-20

Scope: Rust workspace in this repo, including `ngit-core`, `ngit-cli`,
`ngit-serve`, `ngit-tui`, docs, and test coverage.

## Executive Summary

The current implementation is a solid V1 prototype: the workspace is cleanly
split, the file-backed `.ngit/` contract is understandable, and the product loop
works end to end: initialize, capture, annotate intent, attach evidence, record
terminal disposition, preserve lineage, and inspect with doctor.

Post-hardening status: the initial blocker set has been addressed in code and
covered by regression tests. The store now surfaces unresolved evidence without
turning it into core-owned workflow judgement, terminal dispositions leave
transaction markers, doctor reports malformed records and payload hash
mismatches, evidence artifacts are durable and bounded, and `ngit serve` has
localhost defaults, bearer-token mutation auth, path containment, and more
accurate HTTP status mapping.

Remaining release caution: this is still a compact V1 implementation rather than
a production service framework. Before broad distribution, keep validating real
repo workloads, larger evidence artifacts, interrupted-process recovery, and
Windows path behavior under CI.

## Blockers Reviewed For Public Release

### 1. Missing evidence refs can disappear from the evidence rollup

Location: `crates/ngit-core/src/lib.rs:1298`

`compute_readiness_for_refs` silently ignores evidence refs that fail to load.
If a draft contains only missing evidence refs, or a mix of passed and missing
refs, the context can omit the unresolved records and make later audit weaker.

Recommended fix:

- Track every missing or malformed evidence ref in `unresolved_evidence`.
- Include unresolved refs in the neutral evidence rollup.
- Do not translate unresolved refs into core-owned workflow judgement.
- Add regression tests for missing evidence refs and malformed evidence JSON.

Status: implemented. Missing or malformed evidence refs populate
`unresolved_evidence`, appear in the `unresolved` rollup bucket, do not block
terminal disposition recording, and are covered by regression tests.

### 2. Accept/reject writes are not transactional across terminal records,
lineage, and draft removal

Locations:

- `crates/ngit-core/src/lib.rs:816`
- `crates/ngit-core/src/lib.rs:857`

`accept` writes an accepted record, writes lineage, then removes the draft.
`reject` has the same shape. A crash or IO error between these steps can leave a
terminal record without lineage, or both a draft and terminal record.

Recommended fix:

- Introduce a small store transaction/finalization helper.
- Write all new records to temp files first.
- Commit by durable rename in a defined order.
- Leave a recoverable marker if finalization is interrupted.
- Make `doctor` detect and optionally repair interrupted transitions.

Status: implemented for detection and safer finalization. Terminal transitions
write transaction manifests and staged records before final records; doctor
reports incomplete transaction markers. Automatic repair remains intentionally
deferred unless the recovery path is deterministic.

### 3. Local API can run arbitrary commands without authorization

Locations:

- `crates/ngit-serve/src/lib.rs:154`
- `crates/ngit-core/src/lib.rs:731`

`POST /evidence/run/:draft_id` accepts a command array and runs it in the repo.
That is intentional for evidence capture, but it is not safe as an unauthenticated
HTTP endpoint, even on localhost.

Recommended fix:

- Require a per-session bearer token for all mutation endpoints.
- Bind to `127.0.0.1` by default and reject non-loopback binds unless explicitly
  opted in.
- Add an allowlist/confirmation layer for commands launched through HTTP.
- Add command timeout and output-size limits.
- Consider disabling command execution through `serve` until the local UX has a
  secure broker contract.

Status: implemented for V1 local middleware. Mutation endpoints require bearer
auth, non-loopback binds require explicit opt-in, and evidence command execution
uses timeout/output bounds.

### 4. Artifact and evidence file routes allow path escape

Locations:

- `crates/ngit-serve/src/lib.rs:142`
- `crates/ngit-serve/src/lib.rs:220`

The API joins user-supplied path strings onto repo paths without canonicalizing
and checking the final path. `artifact_body` can traverse out of
`.ngit/evidence/artifacts`, and `evidence/add` can read arbitrary local files if
given an absolute path or enough `..` segments.

Recommended fix:

- Reject absolute paths and any path component equal to `..`.
- Canonicalize the final path and verify it remains under the intended base.
- Add tests for `../`, absolute paths, URL-encoded traversal, and symlink escape.

Status: implemented for path traversal and absolute/parent components, with
canonical containment checks and regression tests.

### 5. Doctor suppresses malformed JSON instead of reporting it

Location: `crates/ngit-core/src/lib.rs:981`

`doctor` calls `read_dir_records(...).unwrap_or_default()`. If a directory has a
malformed record, the error is discarded and doctor can report `ok`.

Recommended fix:

- Add a tolerant record scanner that returns `{ parsed, issues }`.
- Report malformed JSON, unsupported schema versions, missing required fields,
  payload hash mismatches, and unknown schema versions.
- Keep `read_dir_records` strict for normal command execution.

Status: implemented. Doctor uses tolerant scans and reports malformed records
instead of converting scan errors into empty sets.

## Medium-Risk Hardening

### CLI JSON watch mode is not JSONL

Location: `crates/ngit-cli/src/main.rs:220`

Continuous `ngit watch --json` currently emits pretty-printed JSON documents.
That is pleasant for humans but awkward for stream consumers and conflicts with
the spec language that calls for JSON lines.

Recommended fix:

- Use compact `serde_json::to_string` for watch output.
- Keep pretty output for one-shot non-stream commands.
- Add a test that two watch iterations produce line-delimited JSON objects.

Status: implemented for compact JSONL emission in watch JSON mode.

### `ngit evidence run` had no human output mode

Location: `crates/ngit-cli/src/main.rs:305`

The original command always emitted JSON because `EvidenceRunArgs` had no
`--json` flag and the handler forced JSON output.

Recommended fix:

- Add `--json` for consistency.
- Default human output to `evidence <id> <status>`.
- Update the walkthrough and CLI smoke test.

Status: implemented. `ngit evidence run` supports `--json`; human mode emits a
compact `evidence <id> <status>` line.

### Git status parsing should use porcelain v2 with NUL delimiters

Location: `crates/ngit-core/src/lib.rs:1402`

The current porcelain v1 parser is enough for simple paths but will mishandle
renames, quoted paths, newline-containing paths, and some status combinations.

Recommended fix:

- Use `git status --porcelain=v2 -z`.
- Store structured path records for ordinary, rename, copy, and unmerged states.
- Add tests for spaces, renames, deletes, and binary files.

Status: partially implemented. Status parsing now uses porcelain v2 with NUL
records and has path-with-spaces coverage. Rename/delete/binary coverage should
continue to expand.

### Snapshot digests are based on textual diff output

Location: `crates/ngit-core/src/lib.rs:565`

Textual `git diff` is useful, but not ideal as a stable capture key for all
content. Binary changes, external diff config, color settings, and diff context
can affect the result.

Recommended fix:

- Use `git diff --no-ext-diff --no-color --binary` at minimum.
- Consider hashing path plus blob IDs for staged content and worktree file bytes
  for unstaged content.

Status: implemented at the V1 digest level with `--no-ext-diff --no-color
--binary`. Blob/content-addressed digest design remains a future schema upgrade.

### Evidence artifacts are not bounded

Location: `crates/ngit-core/src/lib.rs:731`

`Command::output()` buffers all stdout/stderr in memory. A noisy or hanging
command can consume memory or never return.

Recommended fix:

- Add timeout support.
- Stream stdout/stderr to bounded artifact files.
- Record truncation metadata in `ArtifactRef`.

Status: implemented with timeout/output bounds and truncation metadata.

### External evidence files are not preserved as durable artifacts

Location: `crates/ngit-core/src/lib.rs:704`

`add_evidence_from_file` reads a file, stores only a summary, and attaches no
artifact reference. That weakens the blackbox contract because the source
evidence can disappear.

Recommended fix:

- Copy the file into `.ngit/evidence/artifacts/<evidence-id>/`.
- Record digest, original filename, media type if known, and truncation status.
- Enforce `EvidencePolicy.durable_artifacts_required`.

Status: implemented for local file evidence copying and artifact metadata.

## Structural Improvements

- Split `ngit-core/src/lib.rs` into focused modules: `store`, `git`, `records`,
  `evidence_context`, `evidence`, `lineage`, `doctor`, and `migration`.
- Replace stringly typed statuses/actions with enums using serde renames.
- Add schema-version checks during every record load.
- Add payload-hash verification to `doctor`.
- Introduce a `Store` struct that owns `root`, locking, paths, and IO helpers.
- Normalize public errors into user/input/not-found/conflict/internal categories
  so the CLI and API can return accurate exit codes and HTTP status codes.
- Decide whether `ngit-serve` is a development adapter or a release API. If it
  is a release API, move to a tested HTTP stack and define a versioned API
  contract.

## Test Expansion

Recommended release-gating tests:

- missing evidence ref appears as unresolved and does not block disposition
- malformed JSON is reported by doctor
- accept crash/interruption recovery
- path traversal rejection for artifacts and evidence file inputs
- command timeout and output truncation
- status parsing for paths with spaces, renames, deletes, and staged plus
  unstaged combinations
- schema export validates with a JSON Schema validator
- API returns 404 for unknown routes and 400 for invalid request bodies
- concurrent capture/evidence/accept attempts preserve store consistency

## CI/CD Added In This Review

The added GitHub Actions configuration provides:

- Rust formatting, clippy, locked dependency fetch, tests, and schema smoke check
  on Linux, macOS, and Windows.
- Tag-based release artifacts for Linux, macOS, and Windows when pushing
  `v*.*.*` tags.
- Dependabot updates for Cargo and GitHub Actions dependencies.

Windows is now in the CI matrix. The test suite avoids Unix-only commands in
CLI/evidence smoke tests.
