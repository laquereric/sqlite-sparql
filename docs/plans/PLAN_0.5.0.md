# PLAN 0.5.0 — SPARQL UPDATE

> Expose Oxigraph's `Store::update` as a `sparql_update(query)` scalar
> so consumers can issue arbitrary SPARQL 1.1 UPDATE forms — `INSERT
> { … } WHERE { … }`, `DELETE { … } WHERE { … }`, `INSERT DATA`,
> `DELETE DATA`, `CLEAR`, `CREATE`, `DROP`, etc. — not just the
> handful of cases that the existing scalar surface covers.

Driver: `CONSUMER_REQUIREMENT_RS.md` § 5 — "SPARQL UPDATE — optional,
post-v0.2.0" — and the long-standing `CLAUDE.md` § 3 carry-over. RS's
own `PLAN_0.3.0` is gated on this. MM's CONSUMER doc explicitly
*excludes* arbitrary UPDATE from the substrate's expectations (MM
mutates via `Storable` lifecycle hooks), so this release primarily
serves RS, not MM.

Depends on 0.2.0 (shared store), 0.3.0 (named graphs — UPDATE forms
naturally reference named graphs), and 0.4.0 (batched-insert path is
a strict subset of what UPDATE can express, so no rework there).

---

## Goal

`cargo test` passes a new set of UPDATE tests. Round-trip works:
issue `INSERT DATA { … }` via `sparql_update`, read back via
`sparql_query`. RS can drop its
`Semantica::Sparql.execute → INSERT DATA / DELETE DATA / CLEAR ALL`
special-casing in favour of routing every non-DATA UPDATE through
the new scalar.

---

## Phase A — `sparql_update` scalar

`functions/sparql_query.rs` already houses `sparql_query / sparql_ask
/ sparql_construct`. Add a fourth in the same file:

```rust
pub fn sparql_update_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let query_str = api::value_text(values.get(0).expect("UPDATE query"))?;
    let delta = execute_sparql_update(query_str)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int64(context, delta);
    Ok(())
}
```

Body of `execute_sparql_update`:

- Take `rdf_count_all()` before and after.
- Call `store.update(query_str)` between them.
- Return `(after as i64 - before as i64)` — the **net change in
  store size**.

### Return value

The CONSUMER doc wording is *"count of affected triples"*. Oxigraph
0.4's `Store::update` returns `Result<(), EvaluationError>` — there is
no first-class "affected" count, and computing it correctly for a
mixed `DELETE/INSERT { … } WHERE { … }` operation would require
re-running the WHERE pattern. So `sparql_update` returns the **signed
net change**, not the count of every quad touched:

| UPDATE shape                           | Return value             |
|----------------------------------------|--------------------------|
| `INSERT DATA { … }`                    | `+N` (newly inserted, post-dedup) |
| `DELETE DATA { … }`                    | `-N` (removed) |
| `INSERT { … } WHERE { … }`             | `+N` (effective inserts) |
| `DELETE { … } WHERE { … }`             | `-N` (effective deletes) |
| Mixed `DELETE/INSERT { … } WHERE { … }`| `inserts - deletes` (may be 0 even when both happened) |
| `CLEAR DEFAULT` / `CLEAR ALL`          | `-N` (count cleared)     |
| `CREATE GRAPH <…>` / `DROP GRAPH <…>`  | net effect on quad count |

Document this clearly. RS's consumer-side facade can `.abs` the
return when the caller knows the UPDATE is one-direction.

`api::result_int64` (i64) rather than `result_int` (i32) because
`rdf_count_all()` is `usize` — at large stores the delta wouldn't
fit in `i32`.

### Network safety

SPARQL 1.1 `LOAD <iri> [INTO GRAPH <g>]` makes Oxigraph fetch the IRI
over HTTP. That is a *network surface* opened by a SQLite scalar —
not what extension users expect.

**Decision:** for 0.5.0 we do *not* try to pre-validate the parsed
AST. Whether `LOAD` succeeds or fails depends on Oxigraph's reqwest
feature flags at build time (today: no HTTP, so `LOAD` returns an
evaluation error). Document under "Limitations". A future release can
add a hard reject if the threat model changes.

### Exit criteria for Phase A

```sql
-- INSERT DATA
SELECT sparql_update('INSERT DATA { <http://e/a> <http://e/p> "x" }');
-- → 1
SELECT rdf_count();
-- → 1

-- DELETE DATA
SELECT sparql_update('DELETE DATA { <http://e/a> <http://e/p> "x" }');
-- → -1
SELECT rdf_count();
-- → 0

-- WHERE-based INSERT into a named graph
SELECT sparql_update(
  'INSERT { GRAPH <urn:g:dst> { ?s ?p ?o } } WHERE { ?s ?p ?o }'
);
```

---

## Phase B — tests

All under `tests/integration_test.rs`, all `#[serial]` like the rest.

- `test_sparql_update_insert_data` — single-quad INSERT DATA, delta
  `+1`, `rdf_count()` matches.
- `test_sparql_update_delete_data` — insert via `rdf_insert` first
  then DELETE DATA; delta `-1`; round-trip back to empty.
- `test_sparql_update_dedup_on_insert_data` — INSERT DATA the same
  quad twice in one call; net delta is `+1` (RDF set semantics).
- `test_sparql_update_where_insert` — `INSERT { … } WHERE { … }`
  derives new triples from existing ones; delta matches WHERE
  cardinality after deduplication.
- `test_sparql_update_modify` — `DELETE { … } INSERT { … } WHERE
  { … }` runs both halves in one transaction; assert *the final
  store state*, not the delta (because the delta can lie for mixed
  ops, and we'd rather pin the observable behaviour).
- `test_sparql_update_named_graph` — INSERT DATA into a named graph
  via `INSERT DATA { GRAPH <urn:g:bhphoto> { … } }`; assert the
  triple is in the named graph (via `rdf_count('urn:g:bhphoto')`)
  and *not* in the default graph (via `rdf_count()`).
- `test_sparql_update_clear_default` — populate default graph, run
  `CLEAR DEFAULT`, assert default empty but named graphs untouched.
- `test_sparql_update_clear_all` — `CLEAR ALL` empties every graph.
- `test_sparql_update_parse_error_surfaces` — malformed update
  returns a `SQLITE_ERROR`-class error string (not a panic). The
  message should mention "parse" so consumers can pattern-match.
- `test_sparql_update_evaluation_error_surfaces` — a syntactically
  valid update that fails at evaluation (e.g. `LOAD` against an
  unreachable URI under Oxigraph's no-HTTP default) returns an
  error string, not a panic.

### Exit criteria for Phase B

```
cargo test               # all green
cargo test --release     # same
```

Test count goes from 27 + 1 ignored (0.4.0) to **37 + 1 ignored** —
ten new tests.

---

## Phase C — docs

- `README.md` — add `sparql_update(query)` to the features list and
  the SQL surface examples. Tick the matching roadmap checkbox.
  Limitations section gains a one-line note about `LOAD` and HTTP.
- `CHANGELOG.md` — 0.5.0 entry. Lead with the new function and
  spell out the signed-delta return contract, since that is the
  most likely source of confusion.
- `CLAUDE.md` § "Completing the Implementation" — mark #3 (SPARQL
  UPDATE) as DONE in 0.5.0 with a pointer to this plan.
- `CONSUMER_REQUIREMENT_RS.md` — graduate § 5 (SPARQL UPDATE) from
  "Requested" into the live SQL surface; document the signed-delta
  return.
- `CONSUMER_REQUIREMENT_MM.md` — add `sparql_update` to the live
  list with a note that MM does not currently exercise it. (Adding
  it to "SQL surfaces MM consumes" is wrong since MM doesn't
  consume it; add it to a small new "Available but not exercised"
  section so the doc reflects what the engine ships.)

---

## Phase D — tag 0.5.0

- Bump `Cargo.toml` and `VERSION` to `0.5.0`.
- `cargo test` green.
- `git tag v0.5.0` and push.

---

## Out of scope

- **Computing a precise "rows affected" count for mixed UPDATEs.**
  Would require re-evaluating the WHERE pattern outside of Oxigraph's
  update machinery or upstreaming a counting hook into Oxigraph
  itself. Net delta is a useful enough signal for the common cases;
  callers with mixed-shape needs can run two separate updates.
- **Pre-validating the parsed UPDATE AST** (e.g. rejecting `LOAD`
  before evaluation, or rejecting `CLEAR ALL` based on policy).
  Possible via `spargebra::Update::parse`, but not in scope for
  0.5.0. Sandbox the engine by Oxigraph build features instead.
- **A bulk update form taking JSON.** Single string in, signed-i64
  out. If a future consumer wants array semantics, `rdf_insert_many`
  / `rdf_delete_many` (0.4.0) already cover the value-driven case.
- **Exposing UPDATE results via the `rdf_triples` virtual table.**
  Vtab writes still go through the 3-/4-column INSERT / DELETE paths
  from 0.3.0; SPARQL UPDATE is a scalar-only surface.

---

## Risks

- **Signed return type confuses naive callers.** A consumer expecting
  "always a positive count" will misread DELETE DATA results.
  Mitigation: README example pairs each UPDATE shape with its
  return value; the CHANGELOG leads with the contract. RS's facade
  layer is the place that absorbs this.
- **`LOAD` against an HTTP IRI inside a SQLite scalar.** If the
  Oxigraph build ever enables HTTP, `sparql_update('LOAD <…>')`
  becomes a network call from inside the database. Document the
  exposure; revisit with a pre-validation pass if Oxigraph defaults
  change in a future release.
- **Transactional atomicity surprises.** Oxigraph 0.4's `update`
  wraps the whole UPDATE in one internal transaction — good. But a
  panic mid-evaluation (which we don't expect, but can't prove
  absent) would leave the store in an undefined state. The shared
  store from 0.2.0 makes this a process-wide consistency hazard,
  not a per-connection one. Lock down via `test_sparql_update_parse_error_surfaces`
  and `test_sparql_update_evaluation_error_surfaces` — if either
  ever triggers a Rust panic, the test harness will catch it and
  surface as a regression.
- **Concurrent UPDATE and read.** The shared-store model means a
  long-running UPDATE can interleave with concurrent reads via
  `sparql_query` on another thread. Oxigraph's in-memory storage
  serialises within a transaction, so reads either see the
  pre-UPDATE state or the post-UPDATE state but never a torn one.
  No additional locking on our side is needed; pin this with a
  follow-up if a CI flake suggests otherwise.

---

## Re-numbering downstream milestones

After 0.5.0 ships:

| Version | Topic | Status |
|---|---|---|
| 0.5.0 | SPARQL UPDATE | this plan |
| 0.6.0 | Persistent RocksDB backend via `rdf_open(path)` | (future) |
| 0.7.0 | `sqlite-sparql-ruby` gem wrapper | (future) |
| 0.8.0 | SPARQL HTTP endpoint | (future) |

No re-shuffle relative to PLAN_0.2.0's table; this is the orderly
continuation.
