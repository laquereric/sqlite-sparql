# PLAN 0.4.0 — batched insert / delete

> Add `rdf_insert_many(json)` and `rdf_delete_many(json)` scalar
> functions that take a single JSON-array argument and loop in Rust.
> Per-triple SQL calls and `INSERT DATA { ... }` round-trips both pay
> per-row parse + FFI overhead; this collapses that to one call.

Driver: `CONSUMER_REQUIREMENT_MM.md` § "Array-argument batched insert
(`rdf_insert_many`)" — MM's PLAN_0_29_1 Phase B.1 copy migration
(thousands of triples in one shot) and `Semantica::Storable`'s per-save
lifecycle hooks (every Product save re-emits multiple predicates).

Depends on 0.2.0 (shared store) and 0.3.0 (named graphs). The 4-element
row form `[s, p, o, graph]` is the version of this API that lands;
shipping a graph-less variant first and re-cutting later would force
two MM submodule bumps for what is really one feature.

---

## Goal

`cargo test` passes new batch tests. A batch of 1000 triples loads
materially faster than 1000 single `rdf_insert` calls — target is
"5× or better" on cold-load microbenchmarks. The function name and
JSON shape match what `CONSUMER_REQUIREMENT_MM.md` documents so the
`rails-semantica` `Sparql.bulk_insert` convenience can land
unchanged.

---

## Phase A — JSON parsing + bulk loader path

Input shape (see CONSUMER doc):

```json
[
  ["http://e/s1", "http://e/p", "\"value 1\"", "urn:g:bhphoto"],
  ["http://e/s2", "http://e/p", "\"value 2\"", null],
  ["http://e/s3", "http://e/p", "\"value 3\""]
]
```

Rules:

- Each row is a JSON array of either 3 elements (`[s, p, o]`) or 4
  elements (`[s, p, o, graph]`).
- The 4th element is the graph IRI string, or `null` for the default
  graph.
- Term encoding is the same N-Triples-shape as the existing 3-arg
  `rdf_insert` arguments: `<iri>` is optional for IRIs since the
  current scalar accepts bare IRI strings, `"…"` for literals, `_:b0`
  for blanks. **The CONSUMER doc spec wins where ambiguous** — bare
  IRI strings on s, p, o; the `<>` wrapping is *not* required.
- Empty array → returns `0`, no error.
- Any malformed row (wrong arity, non-string element, invalid IRI)
  aborts the whole batch with a clear error and inserts zero triples.
  This is the conservative choice; partial-success modes can come
  later if asked for.

### Implementation

- `serde_json::from_str::<Vec<Vec<serde_json::Value>>>(input)` for
  the outer parse.
- Loop rows; for each, dispatch to a per-row helper that builds a
  `Quad` and accumulates it in a `Vec<Quad>`.
- Bulk-insert via Oxigraph 0.4's `Store::bulk_loader()` —
  `store.bulk_loader().load_quads(quads)?`. This is materially faster
  than per-row `store.insert(&quad)` for large batches because it
  buffers index writes and amortises the dictionary work.
- Return the count of *successfully accepted* quads. Because RDF is
  set semantics, duplicates count once. The return is *batch size
  minus duplicates already in the store*.

### Exit criteria for Phase A

```sql
SELECT rdf_insert_many('[
  ["http://e/s1","http://e/p","\"a\""],
  ["http://e/s2","http://e/p","\"b\"","urn:g:bhphoto"]
]');
-- returns 2

SELECT rdf_count_all();
-- returns 2
```

---

## Phase B — `rdf_delete_many`

Mirror of insert_many. Same JSON shape, same dispatch, same arity
rules. Deletions of triples that aren't in the store are no-ops, not
errors, but they don't count toward the return value. The return is
the number of quads actually removed.

There is no Oxigraph "bulk delete" — the inner loop is per-row
`store.remove(&quad)`. Still wins over SQL-level looping because the
JSON parse is one-shot and the FFI crossing is one-shot.

### Exit criteria for Phase B

```sql
SELECT rdf_delete_many('[
  ["http://e/s1","http://e/p","\"a\""],
  ["http://e/missing","http://e/p","\"x\""]
]');
-- returns 1 (one actually removed, one no-op)
```

---

## Phase C — tests

- `test_insert_many_3_arg_rows` — all rows are 3-element; everything
  lands in the default graph.
- `test_insert_many_mixed_arities` — some 3-element, some 4-element
  rows; default vs named graph routing works.
- `test_insert_many_dedup_return_value` — insert the same row twice
  in the same batch; return value reflects deduplication.
- `test_insert_many_malformed_aborts_batch` — feed a row with a bad
  IRI; return is an error, `rdf_count_all() = 0`.
- `test_insert_many_empty_array` — `'[]'` returns `0`, no error.
- `test_delete_many_partial` — delete one present row and one absent
  row; return value is `1`, count drops by `1`.
- `test_insert_many_perf_smoke` *(release-only, `#[ignore]` by
  default)* — 1000 rows via `rdf_insert_many` finishes in under
  100 ms wall-clock on commodity arm64. Loose so a slow CI runner
  doesn't flap, but tight enough that a regression in the
  bulk-loader path shows up.

### Exit criteria for Phase C

```
cargo test                 # all green, test count rises by ~6
cargo test --release       # same
cargo test --release -- --ignored insert_many_perf_smoke  # perf gate
```

---

## Phase D — docs

- `README.md` — extend the SQL function table; add a short
  "Batched writes" section showing the JSON shape and the
  `Semantica::Sparql.bulk_insert` consumer pattern.
- `CHANGELOG.md` — 0.4.0 entry.
- `CONSUMER_REQUIREMENT_MM.md` — graduate the batched-insert
  section from "Requested" to "SQL surfaces MM consumes" and remove
  the `(toward 0.4.0)` tag.

---

## Phase E — tag 0.4.0

- Bump `Cargo.toml` and `VERSION` to `0.4.0`.
- `cargo test` green.
- `git tag v0.4.0` and push.
- Bump MM's submodule pin + open the MM-side PR.

---

## Out of scope

- Streaming insert from a SQL cursor (`INSERT INTO triples SELECT …
  FROM source`). The `rdf_triples` virtual table already supports
  this shape and `rdf_insert_many` is for the FFI-crossing case
  specifically.
- A CSV / TSV ingest variant. JSON-array matches the CONSUMER
  contract; other formats can be added later if asked.
- Streaming JSON parsing for huge batches. 0.4.0 reads the whole
  input string into memory and parses with `serde_json::from_str`.
  At MM's planned 1000-row batch size that is roughly 100–300 KB —
  trivially in-memory. A stream-parsing variant lands only if
  someone hits a real allocation ceiling.
- Partial-success modes. The conservative all-or-nothing batch
  behaviour matches RDF's transactional intuition and keeps the
  return value meaningful. If MM later wants `rdf_insert_many_lax`
  that returns `[ok_count, error_count]`, that is additive.

---

## Risks

- **`Store::bulk_loader()` semantics.** Oxigraph's bulk loader is
  optimised for cold loads and may have surprising behaviour on a
  warm store (e.g. higher memory churn). Confirm by the perf-smoke
  test on a store that already has 100k+ triples; if performance is
  worse than per-row insert in that regime, fall back to a per-row
  loop with a tight inner allocator.
- **JSON term encoding ambiguity.** The CONSUMER doc shows
  `"http://e/s"` for IRIs (no `<>`) and `"\"value\""` for literals
  (escaped quotes). The single-row `rdf_insert(s, p, o)` already
  accepts both shapes through `store::parse_term`. `rdf_insert_many`
  must use the **same** parser — no divergence. Lock this in with a
  test that feeds an IRI with `<>` wrapping and one without, and
  asserts both produce the same triple.
- **All-or-nothing rollback.** Oxigraph's in-memory `Store` does not
  expose a transaction-rollback primitive that we use today (the
  RocksDB backend has `StorageWriter`/`commit`/`rollback`, the
  memory backend less so). The cleanest implementation is: parse and
  validate every row up front; only then start inserting. If a quad
  fails to insert mid-batch (which should be impossible after
  validation), the partial state is documented as undefined. In
  practice, with up-front validation, this never happens.
