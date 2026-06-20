# Add Decision Data to Code Changes

<img src="assets/project_logo.png" width=300 alt="abstract logo">

This project, `ngitd core` (EN-git-dee), is a local memory layer for software changes. It records what a
Git repo looked like, what change was captured, what evidence was attached, and
what terminal disposition a human or supervisor recorded later.

It is intentionally small. This project gathers and preserves evidence; it does not
decide whether a product direction is good, whether a rule should be broken, or
whether a broader workflow should proceed.

## Getting Started

Build and test the workspace:

```text
cargo test --all
```

The examples below use `ngit`. From a source checkout, you can run the same
commands as `cargo run -p ngit-cli -- <args>`.

Initialize a Git repo for local memory:

```text
ngit init
```

Capture the current repo delta as a draft:

```text
ngit capture --intent "Why this change exists"
```

Attach evidence by running a command:

```text
ngit evidence run <draft-id> -- rustc --version
```

Inspect drafts and records:

```text
ngit drafts
ngit show <draft-id>
```

Record a terminal disposition when a human or supervisor is ready:

```text
ngit accept <draft-id>
ngit reject <draft-id> --reason "Superseded by another change"
```

Check store health:

```text
ngit doctor
```

## What we store, what we focus on

- repo-local `.ngit/` initialization
- Git state snapshots and repo event observation
- draft change capture
- intention, rationale, and note annotations
- evidence records and durable evidence artifacts
- neutral evidence status rollups
- accepted/rejected terminal records and lineage
- CLI, TUI, and foreground local HTTP inspection surfaces


## Evidence, Not Judgement

Evidence has statuses such as `passed`, `failed`, `needs_review`, and
`blocked`. This app rolls those statuses up so users can query and audit them. It
does not translate them into workflow meaning like "accept this", "revise this",
or "block the direction".

If an AI reviewer, policy engine, human, or supervisor makes that judgement, it
should be recorded as evidence or annotation with explicit provenance.

## Current Implementation Notes

- Rust workspace: `ngit-core`, `ngit-cli`, `ngit-tui`, and `ngit-serve`
- User-facing binary: `ngit`
- Main commands: `init`, `status`, `watch`, `capture`, `annotation`,
  `evidence`, `accept`, `reject`, `history`, `lineage`, `doctor`
- Contract tools: `schema export`, `migrate`
- Local API: `ngit serve`, a foreground adapter over one repo-local `.ngit/`
  store

Some JSON field names still use compatibility terminology, especially
`readiness` and `decision_context`. In the current product model those fields
carry neutral evidence-rollup context, not core-owned workflow judgement.

## Local API

`ngit serve` is useful for local experiments and UI integration:

```text
ngit serve --token dev-token
curl -H "Authorization: Bearer dev-token" \
  -d '{"intent":"Captured from local UX"}' \
  http://127.0.0.1:7878/capture
```

It is not a daemon lifecycle yet. A future daemon can index and query one or
more repo/worktree `.ngit/` stores for a UI, while each repo-local store remains
the durable source of truth.

## More Detail

Start with [SPEC.md](./SPEC.md) for the product and technical contract, and
[docs/release-readiness-review.md](./docs/release-readiness-review.md) for the
current hardening review.
