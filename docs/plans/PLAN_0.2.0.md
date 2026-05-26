# PLAN 0.2.0 — shared process-wide store

> Replace `thread_local! { RefCell<Store> }` with
> `OnceLock<Arc<Store>>`. One Oxigraph store per process, shared across
> every SQLite connection and every thread. The SQL surface does not
> change. The named-graphs work that was tentatively pencilled in as
> 0.2.0 in PLAN_0.1.0 shifts to 0.3.0; batched insert moves to 0.4.0.

## Reconciliation with `CONSUMER_REQUIREMENT_MM.md`

The MM consumer document lists named-graph support and
`rdf_insert_many` under "Requested extensions (toward 0.2.0)". This
plan retasks 0.2.0 for the shared-store correctness fix surfaced in
`docs/reviews/REVIEW_0.1.0.md` and explicitly demotes the
consumer-named features:

- Named graphs → `PLAN_0.3.0.md`.
- Batched insert (`rdf_insert_many`) → `PLAN_0.4.0.md`.

Why this is safe for MM:

- The CONSUMER doc states (§ Thread / connection model) that
  *"per-connection store isolation is acceptable for v0.29.x"* — MM's
  current scope tolerates the old model. The shared-store change is
  strictly *additive* for MM: writes that were previously isolated to
  one pooled thread now become visible to every pooled thread. That
  is the bug REVIEW_0.1.0 names, and fixing it cannot regress a
  consumer that already tolerated the buggy version.
- The SQL surface MM consumes (`rdf_insert`, `rdf_delete`,
  `rdf_count`, `sparql_query`, `sparql_ask`, `sparql_construct`,
  `rdf_triples`) does not change in shape or in JSON envelope.
- The two consumer-named features still ship — just as 0.3.0 and
  0.4.0 instead of 0.2.0. MM updates its submodule pin once per
  release.

`CONSUMER_REQUIREMENT_MM.md` is updated in this plan's Phase C to
re-label those sections "toward 0.3.0" / "toward 0.4.0" so the two
files don't drift.

`REVIEW_0.1.0.md` lays out why this is the highest-leverage follow-up to
0.1.0: it removes the per-thread memory blow-up, removes the
"insert-on-A-invisible-from-B" footgun, and is a non-breaking change at
the SQL boundary.

Oxigraph 0.4's in-memory `Store` is already `Send + Sync` and uses
Arc-shared indexes internally — every mutating method is on `&Store`, so
a shared `&'static Store` is enough. No additional `Mutex` or `RwLock`
is needed for the in-memory back-end.

---

## Goal

`cargo test` stays green after the change. A new test proves that two
SQLite connections on two different threads see the same triple. The
release `.dylib` still loads in `sqlite3` and `examples/demo.sql` still
runs end-to-end.

---

## Phase A — rewrite `src/store.rs`

The whole file is small; replace it wholesale rather than try to patch.

- Drop `use std::cell::RefCell;`. Add `use std::sync::{Arc, OnceLock};`.
- Replace the `thread_local! { static STORE: RefCell<Store> }` block
  with `static STORE: OnceLock<Arc<Store>> = OnceLock::new();` and a
  private `fn store() -> &'static Store` helper that calls
  `STORE.get_or_init(...)` on first access.
- `with_store(f)` becomes `f(store())`. `with_store_mut` was always a
  misnomer (it took `&Store`); collapse it into `with_store` and update
  the call sites in `functions/rdf_triple.rs` and `functions/
  sparql_query.rs` if any survive.
- `clear_store()` must keep working. Oxigraph 0.4 `Store` exposes
  `Store::clear() -> Result<_, StorageError>`. Call that on the shared
  store rather than replacing the `OnceLock`'s contents (which is not
  possible by design). Surface any error through `SparqlError::StoreError`
  in `rdf_clear_fn` rather than `unwrap`.
- `insert_triple`, `delete_triple`, `triple_count` all already take
  `&Store` and need no signature change — only the body becomes
  `store().insert(&quad).map_err(…)` etc. without the `RefCell::borrow`
  dance.

### Exit criteria for Phase A

```
cargo build              # 0 errors, 0 warnings
cargo build --release    # same
```

---

## Phase B — adjust the tests

The 0.1.0 test suite encodes the *old* invariant. Two things have to
move:

- **Delete `test_thread_local_isolation`.** It explicitly asserts that
  thread B sees an empty store after thread A inserts. After 0.2.0 that
  assertion is wrong on purpose. Removing it is the point of the
  release.
- **Add `test_cross_thread_visibility`.** Same shape as the old test
  but flipped: thread A inserts, thread B reads, assertion is that
  thread B sees the row thread A wrote. Use one `rdf_clear()` at the
  start of the test to isolate it from other tests (because now they
  all share the same store).
- **Add `test_shared_store_across_connections`.** Open two SQLite
  connections on the *same* thread (the common Rails case where the
  pool hands out two checked-out connections) and verify that an
  insert through one shows up through the other.
- Audit the other 11 tests for hidden assumptions of an empty
  starting store. They almost certainly rely on it. The cheapest fix
  is to call `rdf_clear()` as the first statement of each test via a
  small `open_with_extension()` helper change rather than rewriting the
  bodies.

### Exit criteria for Phase B

```
cargo test               # all green on macOS arm64
cargo test --release     # same
```

The test count goes from 12 to 13: drop one, add two.

---

## Phase C — docs

- `README.md` — the "Limitations" section needs the thread-local
  language replaced. The new caveat is much weaker: "one in-memory
  graph per process; data is lost on restart". Removing the per-pool
  warning is the visible win.
- `CHANGELOG.md` — add the 0.2.0 entry. Lead with the behavior change
  ("connections now share a single process-wide graph") and call out
  that `clear_store()` is now a real clear, not a swap.
- `docs/reviews/REVIEW_0.1.0.md` — leave alone. It is a historical
  record of why this plan exists; rewriting it would erase the
  reasoning trail.
- `src/store.rs` doc comment at the top — currently describes the
  thread-local design. Rewrite to describe the OnceLock design and
  why the Oxigraph in-memory store is safe to share.

### Exit criteria for Phase C

A user following `README.md` from a clean clone reaches the same
working SPARQL query as 0.1.0, and the "Limitations" section no longer
mentions threads.

---

## Phase D — tag 0.2.0

- Bump `Cargo.toml` and `VERSION` to `0.2.0`.
- Confirm `cargo test` green at the bumped version.
- `git tag v0.2.0` and push.

---

## Re-numbering downstream milestones

PLAN_0.1.0's "Post-0.1.0" section pinned five later versions. With the
shared-store work taking the 0.2.0 slot and the consumer-requested
features each getting their own dedicated plan, the post-0.1.0
roadmap looks like:

| Version | Topic | Plan |
|---|---|---|
| 0.2.0 | Shared process-wide store | `PLAN_0.2.0.md` (this file) |
| 0.3.0 | Named graphs (4th `graph` column on `rdf_triples`) | `PLAN_0.3.0.md` |
| 0.4.0 | Batched insert (`rdf_insert_many` / `rdf_delete_many`) | `PLAN_0.4.0.md` |
| 0.5.0 | SPARQL UPDATE — `sparql_update(query)` | `PLAN_0.5.0.md` |
| 0.6.0 | Graph-scoped bulk loading (`rdf_load_*_to_graph`) | `PLAN_0.6.0.md` |
| 0.7.0 | RDF-star / SPARQL-star round-trip | `PLAN_0.7.0.md` |
| 0.8.0 | Batched CONSTRUCT (`rdf_construct_many`) | `PLAN_0.8.0.md` |
| 0.9.0 | Native OWL 2 RL pass (15-rule subset) | `PLAN_0.9.0.md` |
| 0.10.0 | Full OWL 2 RL derivation coverage (60 rules) | `PLAN_0.10.0.md` |
| 0.11.0 | Native SHACL Core validator (VG CR #7) | `PLAN_0.11.0.md` |
| 0.12.0 | Native DRed dependency index (VG CR #8) | `PLAN_0.12.0.md` |
| 0.13.0 | OWL 2 RL inconsistency detection (`rdf_owl_rl_consistent`) | deferred from 0.10.0 |
| 0.14.0 | `sqlite-sparql-ruby` gem wrapper | (future) |
| 0.15.0 | SPARQL HTTP endpoint | (future) |
| Deferred | Persistent RocksDB backend via `rdf_open(path)` | revive on first consumer ask |
| Deferred | Differential dataflow at store layer (VG CR #10) | out-of-reach for incremental engine work |

Do not edit `CHANGELOG.md`'s historical 0.1.0 entry — its
future-roadmap mentions were accurate *as of the 0.1.0 release* and
should stay that way.

---

## Risks

- **Oxigraph in-memory `Store` is concurrent in practice, not just on
  paper.** Verify by reading the 0.4 docs for `Store::insert` and
  scanning for `Sync` impl. If for any reason concurrent writers
  serialise inside Oxigraph rather than running in parallel, 0.2.0
  still ships — the correctness story holds — but write-heavy
  workloads will see contention. That is a tuning issue for a later
  release, not a blocker.
- **`Store::clear` semantics.** Oxigraph's `clear` removes all quads
  but keeps the store instance. That matches what existing callers
  want (`rdf_clear()` returns `1`, count goes to zero). Confirm by a
  unit test that `rdf_clear()` followed by `rdf_count()` returns `0`
  and that subsequent inserts still work. (This is just the
  pre-existing `test_rdf_clear` — it will keep passing.)
- **Test isolation under shared state.** Cargo runs integration tests
  in parallel within one binary, all in one process, all now sharing
  one Oxigraph store. Without `rdf_clear()` per-test the suite becomes
  order-dependent. The cleanest fix is to bake the clear into
  `open_with_extension()`. Per-thread fixtures are no longer enough.
- **Rails connection-pool surprise of a *different* shape.** Once the
  store is shared, two HTTP requests racing each other can interleave
  writes and reads. That is the right correctness story (it matches
  what every database does), but it is a *new* concurrency surface
  for downstream apps that previously had per-thread isolation.
  Mention this in the CHANGELOG so consumers like
  `vendor/rails-semantica` know to look for it.

---

## Out of scope for 0.2.0

- Named graphs (now 0.3.0).
- Any persistence story — the in-memory restriction is unchanged.
- A `Mutex`/`RwLock` wrapper. Oxigraph's `Store` does its own
  concurrency control; wrapping it would only add contention. If a
  future Oxigraph release walks that back, revisit then.
- Sharding the store by SQLite connection or by `db` handle. The
  whole point of the change is that there is one store; introducing
  another partitioning axis would undo it.
