# PLAN 0.9.1 — Incident write-up for the 0.9.0 publication mishap (option C reconciliation)

> The `v0.9.0` tag was first pushed at a commit that contained only
> the two `CONSUMER_REQUIREMENT_*.md` doc updates — an external
> `git reset` between staging and commit cleared the index of the
> intended 0.9.0 source / test / CHANGELOG / README files.
>
> This file documents the incident **and the option-C reconciliation
> that was eventually applied**: the broken docs-only commit was
> dropped from history, the actual 0.9.0 payload squashed onto a
> single clean commit, the `v0.9.0` tag force-updated to point at it,
> and the short-lived `v0.9.1` rollover tag deleted. After
> reconciliation, the public history is **identical to what it
> would have been had 0.9.0 published cleanly the first time** —
> the `v0.9.0` tag points at one self-contained payload commit, and
> the `v0.9.1` tag does not exist.
>
> No new design; no surface change beyond what PLAN_0.9.0 specified.
> This file exists for the historical record + so future readers
> who notice the `v0.9.1` tag in an old fetch's reflog or in a
> mirror have a written explanation.

## What happened

1. PLAN_0.9.0 Phases A–F landed locally: 15-rule OWL 2 RL library
   (`src/functions/rdf_owl_rl/rules.rs`), fixpoint loop +
   provenance emission + RFC3339 helper (`src/functions/rdf_owl_rl.rs`),
   8 integration tests, version bumps, docs, CR graduations.
2. All 14 modified-or-new files were staged via
   `git add -u && git add <new files>`. `git status --short`
   confirmed every file was in the staging area (left-column `M`
   / `A` / `R`).
3. Between the staging command returning and the `git commit`
   command running, an external `git reset` cleared the index
   (visible in reflog: `6bacedf HEAD@{1}: reset: moving to HEAD`)
   while leaving the working tree intact.
4. `git commit` proceeded with whatever was *still* in the index
   — only the two `CONSUMER_REQUIREMENT_*.md` files (which had
   been re-staged by some intermediate step or hook), producing
   commit `8bbb907` with 2 files changed, 533 insertions(+).
5. `git tag v0.9.0` and `git push origin v0.9.0` pushed the
   docs-only commit as the `v0.9.0` release.

The PLAN_0.9.0 source/test/doc files (`rdf_owl_rl.rs`, `rules.rs`,
`tests/integration_test.rs`, `README.md`, `CLAUDE.md`,
`CHANGELOG.md`, `Cargo.toml`, `VERSION`, `docs/plans/PLAN_0.2.0.md`,
`docs/plans/PLAN_0.9.0.md`, etc.) all stayed in the working tree
uncommitted — so the *intent* of 0.9.0 was preserved on disk; only
the *publication* was wrong.

## Three options considered

| Option | Mechanic | Destructive? | Outcome |
|---|---|---|---|
| **A — Roll forward as v0.9.1** | New commit on top with the actual payload, tag `v0.9.1`, push. | No — `v0.9.0` stays as-is. | Public history shows two 0.9.x tags; `v0.9.0` is permanently docs-only; consumers must learn to pin `v0.9.1`. |
| **B — Force-update the `v0.9.0` tag** | New commit on top, `git tag -f v0.9.0`, `git push --force origin v0.9.0`. | Force-push on a published tag. | Tag content swaps under any consumer who fetched between the broken push and the recovery. Commit graph keeps the docs-only commit as parent of the new payload commit — confusing in `git log`. |
| **C — Reset main + retag** (executed) | Reset `main` to pre-`8bbb907`, restage, single clean commit, force-push branch + tag, delete `v0.9.1`. | Force-push on `main` and both `v0.9.0` / `v0.9.1` tags. | Public history is identical to a clean first publication; no `v0.9.1` exists; consumers who never fetched between the broken push and the recovery never see the mishap at all. |

A was applied first (the `v0.9.1` rollover tag was published) and
later superseded by C. C is more destructive in the short term —
it force-pushes `main` and rewrites a published tag — but is
cleaner long-term: there is no `v0.9.1` to consume, no broken
`v0.9.0` to warn about, and `git log` reads naturally with one
0.9.0 commit and one tag at it.

## Why C over A in the end

After the rollover (option A) shipped, two friction points
remained:

1. **Consumer pinning ambiguity.** A repo wanting to pin "0.9
   stable" had to know to write `v0.9.1`, not `v0.9.0`, because
   `v0.9.0` was docs-only. This is the kind of detail that
   silently rots — a Rails app's `Gemfile` written six months
   later by a different operator would naturally reach for
   `v0.9.0`. Option C eliminates the trap: `v0.9.0` now points
   at the real payload, so pinning it works.
2. **`git log` readability.** Under option A, `git log --oneline
   v0.9.0..HEAD` showed the docs-only commit as the parent of
   the payload commit — a 533-line "0.9.0" commit followed
   immediately by a 2354-line "0.9.1 — publication fix" commit.
   Readers had to read the `PLAN_0.9.1.md` doc to understand why
   the split existed. Under option C, the history reads
   linearly: 0.8.0 → 0.9.0 (one self-contained commit) → next
   release. The doc you are reading now is the only artefact of
   the mishap that survives.

The cost of force-pushing a published tag was bounded — `v0.9.0`
had been live for less than an hour before the rollover and the
rollover had been live for under two weeks before the
reconciliation; no downstream pin existed yet. If consumer
pinning had been established by the time the C-vs-A
re-evaluation happened, option A would have been the only safe
choice and this doc would have been a permanent warning instead
of a postmortem.

The git-safety convention in this repo requires explicit
operator authorisation for force-pushes on published tags and
branches. That authorisation was given before C executed.

## What the reconciliation did

1. **Branched off `6bacedf` (pre-0.9.0 base).** A fresh branch
   `reconcile-0.9.0` was created at the pre-incident commit so
   the rewrite could be staged and verified before touching
   `main`.
2. **Applied `4a88673`'s tree wholesale.** All payload files
   (source, tests, docs, CR updates, CHANGELOG 0.9.0 entry)
   land in one staging step.
3. **Deflated the 0.9.1 bump.** `VERSION` and `Cargo.toml` were
   set back to `0.9.0`. The 0.9.1 CHANGELOG entry was removed
   (it warned consumers off `v0.9.0`, which is no longer
   needed). This file (`PLAN_0.9.1.md`) was rewritten to the
   form you are reading.
4. **Re-applied later commits.** Anything that had landed on
   `main` after the `v0.9.1` rollover (queued plans, `v0.10.0`
   work, etc.) was cherry-picked or replayed on top of the new
   `v0.9.0` commit.
5. **Re-pointed tags + branch.** `v0.9.0` was force-updated to
   the new payload commit. `v0.9.1` was deleted locally and
   remotely. `main` was force-pushed.

After the dust settled, the public state was: `v0.9.0` →
self-contained payload commit; no `v0.9.1` tag; `main` → wherever
the active branch had advanced to (typically with queued plans
+ 0.10.0 work on top).

## Operational lesson (for future releases)

The `Shell cwd was reset` notification after each bash invocation
hints at — but doesn't guarantee — that the next invocation
starts with a clean slate. The git index, however, *persists*
across invocations, and any external process (hooks, parallel
edits in the user's IDE, GUI git tools) can mutate it between
calls. The defensive move for any "stage → commit" pair: run
both in a single chained bash invocation (`git add … && git commit …`)
so no other process can slip in between. Or at minimum, re-verify
`git status --short` immediately before the commit.

Not adopting this as a policy in `CLAUDE.md` — single incident,
the C-reconciliation eliminates the consumer-facing trace, and
the fix is to re-verify staging before commit. If it happens
again, the policy gets formalised.
