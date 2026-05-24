# PLAN 0.8.0 — `rdf_construct_many`

> Add `rdf_construct_many(queries_json TEXT) → TEXT` — accepts a JSON
> array of SPARQL CONSTRUCT query strings, evaluates each against the
> process-wide store, and returns a JSON array of per-query N-Triples
> blobs. One FFI crossing replaces N. Continues the `_many`
> convention established by `rdf_insert_many` (0.4.0) and
> `rdf_load_*_to_graph` (0.6.0).

Driver: `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item
**#9 — Batched SHACL Rules execution**, added in the post-v0.7.0
edit (commit `4753453`). RS's `Shacl::Rules.materialise!` issues one
`sparql_update` per SHACL Rule per fixpoint iteration; a shape with
~50 rules pays the SQL + SPARQL parser cost 50× per iteration. The
batched path reduces FFI / SQL-parse overhead to 1×. Per-rule
SPARQL parse still happens N× (Oxigraph parses each query at
evaluation time) — the engine doesn't ship a prepared-statement
model in 0.8.0.

The review (`docs/reviews/REVIEW_0.1.0.md`) has no live bearing on
this scope. Its five open items at write-time are now: (1) docs
landed in 0.1.0 README, (2) shared store landed in 0.2.0, (3 + 4)
persistence deferred indefinitely as of 0.7.0, (5) bulk loader
landed in 0.4.0. Mentioning the review here only to honour the
"consumer doc and review" framing of this plan's brief.

RS posture (verbatim from the requested-extensions preamble):

> None of the items below are blockers — RS PLANs
> 0.9.0 / 0.10.0 / 0.11.0 / 0.12.0 all ship against the engine's
> existing 0.7.0 surfaces … These asks would unlock a *next
> horizon* of work — predominantly performance — if substrate-side
> telemetry … shows that the SPARQL-driven shape is the
> bottleneck. Priority is "revisit on first concrete bottleneck
> signal," not "schedule a release."

So this release is **forward-leaning** in the same shape PLAN_0.7.0
was: substrate ships ahead of telemetry. The capability lands; the
adoption decision stays with RS PLAN_0.12.0 and waits for a real
signal.

Depends on 0.2.0 (shared store — same `Store::query` call site as
`sparql_construct`), 0.4.0 (`rdf_insert_many` / JSON-array argument
convention and parser-parity test pattern), and 0.6.0
(graph-routing plumbing if the variant lands; not needed for the
0.8.0 surface itself — see "Out of scope").

---

## Goal

`cargo test` passes a new round-trip test that:

1. Inserts a small store via `rdf_insert_many`.
2. Calls `rdf_construct_many` with a JSON array of two CONSTRUCT
   queries.
3. Parses the returned JSON array, asserts each element is a
   syntactically-valid N-Triples blob, and asserts each blob
   round-trips through `rdf_load_ntriples` to the expected quad
   count.
4. Pins parser parity: the same query passed through
   `sparql_construct` (1-arg) and as a 1-element batch through
   `rdf_construct_many` produces byte-identical output.

---

## Why a separate scalar, not "teach `sparql_construct` to take a JSON array"

Same logic as PLAN_0.6.0's `_to_graph` decision and PLAN_0.4.0's
`_many` decision:

- The SQLite scalar-arity model accommodates overloads, but
  conflating `sparql_construct(text)` and
  `sparql_construct(json_array)` under one name means the call site
  has to sniff the argument shape. Honest naming costs one extra
  identifier and removes the sniff.
- The return shape differs structurally — `sparql_construct(text)`
  returns N-Triples TEXT; `rdf_construct_many(json)` returns a JSON
  array. Same-name overloads should not change return shape.
- The `_many` convention is established and recognisable
  (`rdf_insert_many`, `rdf_delete_many`). Future readers know what
  `rdf_construct_many` does before reading the function body.

---

## Return-shape decision — JSON array of N-Triples blobs, not a flat blob and not a count

Three candidates considered:

| Return shape | Pros | Cons |
|---|---|---|
| **JSON array of N-Triples blobs** (chosen) | Per-query attribution preserved. RS can annotate per-rule downstream. Symmetric with `sparql_construct`'s wire form (each element is what a single 1-arg call would return). | Two passes on the consumer side: parse JSON, then per-element parse N-Triples. |
| Concatenated N-Triples blob | Single parse downstream. Smallest output. | Per-query attribution lost — RS can't tell which rule produced which triple. Fatal for RS's `:derivedBy <rule_iri>` annotation use case. |
| Integer count | Tiny. Matches `rdf_insert_many` shape. | Implies the engine inserts the results into the store directly. That's a write side-effect on a function named `_construct_many`; mismatched with `sparql_construct`'s read-only contract. RS would also lose the triples themselves and would have to re-evaluate to annotate. |

JSON array wins on attribution and symmetry. RS's downstream pattern:

```ruby
results = JSON.parse(conn.select_value("SELECT rdf_construct_many(?)", queries_json))
# results: ["<s1> <p1> <o1> .\n", "<s2> <p2> <o2> .\n"]
results.each_with_index do |ntriples_blob, query_idx|
  rule_iri = rules[query_idx].iri
  # Parse ntriples_blob, attach <s,p,o> :derivedBy <rule_iri> annotations,
  # call rdf_insert_many once with all rows from all queries.
end
```

Total FFI crossings RS pays for a 50-rule materialise:
**1** (construct_many) + **1** (one bulk insert_many for all asserted + annotation rows) = **2**, regardless of N. Down from 50×
`sparql_update` today.

---

## Provenance shape stays out of the engine

RS's compatibility constraint (verbatim):

> Either: the engine accepts a `[query, rule_iri]` pair list and
> emits provenance triples itself, or returns a per-query breakdown
> RS uses to emit annotations gem-side.

Pick the second. Same reason PLAN_0.7.0 §6 / `StarExts.md` §6 left
the annotation-vs-occurrence-node decision to MM: the provenance
predicate IRIs (`:derivedBy`, `:derivedAt`, the RDF-star vs.
named-graph encoding choice) are RS-domain concerns that belong in
`rails-semantica`, not in the substrate. The engine emits the data;
the consumer gives it meaning.

This also matches the rdf_insert_many shape — that function returns
counts, not metadata about who inserted what — and stays consistent
with the engine's "domain-agnostic SQL surface" posture across all
0.x releases.

If RS later decides the engine-side annotation path is worth the
coupling (e.g., to amortise the per-row FFI cost of attaching
annotations), a separate scalar (`rdf_construct_many_with_provenance`)
can land in a future release; the surface added here doesn't
foreclose that.

---

## Atomicity — all-or-nothing on parse, partial on evaluation

Two failure modes:

1. **Parse error in one of the N queries.** Pre-flight every query
   through Oxigraph's parser before evaluating any. If any fails,
   raise a SQLite error with the prefix
   `SPARQL parse error (query index N): …` and execute none. Same
   atomicity contract as `rdf_insert_many` (PLAN_0.4.0): malformed
   input aborts the whole batch before any side effect.
2. **Evaluation error mid-batch.** CONSTRUCT is read-only — no
   store mutation, no rollback question. If query N fails at
   evaluation (e.g., an UPDATE-only built-in slips in), abort the
   batch with `SPARQL evaluation error (query index N): …`. The
   queries before N have already been evaluated; nothing has been
   inserted because CONSTRUCT doesn't insert.

The error-message prefix scheme matches `sparql_update`'s
`SPARQL parse error: …` / `SPARQL evaluation error: …` convention
from 0.5.0; the only addition is the `(query index N)` suffix that
tells RS which query in the batch failed. RS pattern-matches the
prefix for its refusal envelope; the index suffix is informational.

---

## Phase A — internal helper

Touch `src/functions/sparql_query.rs`. Add a new helper alongside
the existing `execute_sparql_construct`:

```rust
fn execute_sparql_construct_many(queries_json: &str) -> crate::error::Result<String> {
    let queries: Vec<String> = serde_json::from_str(queries_json)
        .map_err(|e| SparqlError::InvalidArgument(
            format!("rdf_construct_many: expected JSON array of query strings: {e}")
        ))?;

    // Pre-flight: parse every query before evaluating any. Surface
    // a parse error at the first malformed query with its index.
    // (Reuses Oxigraph's parser by calling Query::parse via the
    // same path Store::query takes internally — see PLAN_0.4.0
    // for the parser-parity precedent.)
    for (i, q) in queries.iter().enumerate() {
        if let Err(e) = oxigraph::sparql::Query::parse(q, None) {
            return Err(SparqlError::ParseError(
                format!("SPARQL parse error (query index {i}): {e}")
            ));
        }
    }

    with_store(|store| {
        let mut results: Vec<String> = Vec::with_capacity(queries.len());
        for (i, q) in queries.iter().enumerate() {
            let qres = store.query(q).map_err(|e| {
                SparqlError::EvalError(
                    format!("SPARQL evaluation error (query index {i}): {e}")
                )
            })?;
            match qres {
                QueryResults::Graph(triples) => {
                    let mut blob = String::new();
                    for t in triples {
                        let t = t.map_err(|e| SparqlError::EvalError(
                            format!("evaluation (query index {i}): {e}")
                        ))?;
                        blob.push_str(&format!(
                            "{} {} {} .\n",
                            term_to_ntriples_subject(&t.subject),
                            format!("<{}>", t.predicate.as_str()),
                            term_to_ntriples(&t.object),
                        ));
                    }
                    results.push(blob);
                }
                _ => return Err(SparqlError::InvalidArgument(
                    format!("rdf_construct_many: query index {i} is not a CONSTRUCT")
                )),
            }
        }
        serde_json::to_string(&results).map_err(SparqlError::JsonError)
    })
}
```

The CONSTRUCT serialisation loop is copied from
`execute_sparql_construct`'s `QueryResults::Graph` arm rather than
factored — same shape, but factoring it out would require a
borrow-aware lifetime dance that obscures the read for marginal LoC
savings. If a future release adds a third CONSTRUCT call site (e.g.
a streaming variant), refactor then.

RDF-star round-trip from 0.7.0 — `term_to_ntriples_subject` /
`term_to_ntriples` already emit `<< s p o >>` for quoted-triple
terms. No change needed here; star CONSTRUCT outputs flow through
unchanged.

### Exit criteria for Phase A

`cargo build` clean. No new test failures (no new tests yet — the
function is dead code until Phase B registers it).

---

## Phase B — the scalar + register

```rust
pub fn rdf_construct_many_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let queries_json =
        api::value_text(values.get(0).expect("1st argument: JSON array of CONSTRUCT queries"))?;
    let json_result = execute_sparql_construct_many(queries_json)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_text(context, &json_result)?;
    Ok(())
}
```

Register in `register()` alongside the existing SPARQL functions:

```rust
define_scalar_function(db, "rdf_construct_many", 1, rdf_construct_many_fn, FunctionFlags::UTF8)?;
```

`FunctionFlags::UTF8` only — **not** `DETERMINISTIC`. The store is
mutable (other connections may insert / delete between calls); a
deterministic flag would invite SQLite to cache results that
shouldn't be cached. Matches the existing `sparql_query` /
`sparql_construct` / `sparql_update` flag set.

### Exit criteria for Phase B

`cargo build` clean. `sqlite3 :memory:` after `.load …` exposes
`SELECT rdf_construct_many('["CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }"]')`
and returns a JSON array containing one N-Triples string.

---

## Phase C — integration tests

Add to `tests/integration_test.rs`, beside the existing
`test_sparql_construct` tests. Group under a
`// ── 0.8.0 rdf_construct_many ──` banner near the bottom.

### `test_rdf_construct_many_basic`

1. `rdf_clear()`.
2. Insert two triples via `rdf_insert_many`.
3. Call `rdf_construct_many` with a JSON array of two CONSTRUCTs.
4. Parse the returned JSON; assert it's an array of two strings.
5. Assert each string is a syntactically valid N-Triples blob (run
   it through `rdf_load_ntriples` into a fresh store and confirm
   the count matches the CONSTRUCT's expected output size).

### `test_rdf_construct_many_parser_parity_with_single`

Same CONSTRUCT query passed through `sparql_construct` (1-arg) and
as a 1-element batch through `rdf_construct_many`. Strip the JSON
array wrapper from the batch result and assert byte-identical to
the 1-arg result. Pins that the new path doesn't drift the
serialiser.

(Mirrors `test_insert_many_parser_parity_with_single` from 0.4.0
and `test_rdf_load_ntriples_to_graph_parser_parity` from 0.6.0.)

### `test_rdf_construct_many_empty_array`

`SELECT rdf_construct_many('[]')` returns `'[]'`. Zero queries, zero
results. No store mutation. Pins the degenerate case.

### `test_rdf_construct_many_parse_error_aborts_batch`

A 3-element batch where query index 1 is malformed SPARQL.
`rdf_construct_many` raises a SQLite error whose message starts
with `SPARQL parse error (query index 1):`. Query index 0 must
**not** have been evaluated (observable via a side-channel:
inject a SPARQL function call that increments a counter — not
clean; instead, just pin that the function errored and the store
state is unchanged, which is trivially true since CONSTRUCT
doesn't write). Pins the all-or-nothing pre-flight contract.

### `test_rdf_construct_many_rejects_non_construct`

A batch containing a SELECT query (not CONSTRUCT) errors with the
prefix `rdf_construct_many: query index N is not a CONSTRUCT`.
Pins that the function strictly requires CONSTRUCT shape.

### `test_rdf_construct_many_rejects_non_array_json`

`SELECT rdf_construct_many('not json')` and
`SELECT rdf_construct_many('{"not": "array"}')` both error with the
prefix `rdf_construct_many: expected JSON array of query strings:`.

### `test_rdf_construct_many_with_rdf_star`

A CONSTRUCT that emits quoted-triple subjects round-trips through
the batch path. Confirms the 0.7.0 RDF-star serialiser flows
through `rdf_construct_many` without regression.

### Exit criteria for Phase C

```
cargo test               # all green
cargo build --release && cargo test --release    # see CLAUDE.md
                                                 # "Footgun" note
```

Test count climbs by 7 (55 → 62 + 1 ignored).

---

## Phase D — docs

- **`README.md`** — extend the "SPARQL UPDATE" section's neighbour
  (one new subsection "Batched CONSTRUCT") with the example call
  shape + return-format note. Update the Roadmap checkbox list:
  `[x] rdf_construct_many — landed in 0.8.0`.

- **`CLAUDE.md`** — "SQL Function Reference" → "SPARQL Querying"
  block gains `rdf_construct_many` with the JSON-array return note.
  "Completing the implementation" → "RDF-star" stays at item 4;
  add the new gem wrapper / HTTP endpoint at items 6 / 7 (gem
  slipped from 0.8.0 to 0.9.0; HTTP from 0.9.0 to 0.10.0).

- **`CHANGELOG.md`** — add a 0.8.0 entry. Lead with "Batched
  CONSTRUCT — `rdf_construct_many(queries_json)` matches the
  `_many` convention". Cross-reference
  `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item #9.
  Spell out the JSON-array return shape and the per-query
  attribution rationale so future readers understand the
  non-`INTEGER` return.

- **`src/functions/sparql_query.rs`** doc comment at the top — note
  the new function alongside `sparql_query` / `sparql_ask` /
  `sparql_construct` / `sparql_update`.

### `CONSUMER_REQUIREMENT_RS.md` graduations

This release graduates item #9 from "Requested" to "SQL surfaces RS
consumes". Update both sides of the doc:

- In "SPARQL querying" table, add a row:
  `rdf_construct_many(queries_json TEXT) → TEXT (JSON array)` —
  call site `Semantica::Shacl::Rules.materialise!` (once RS
  PLAN_0.12.0 lands and routes through it). Pin the JSON-array
  return shape and the per-query attribution contract. Note the
  prefix scheme for parse / evaluation errors.
- In "Requested extensions" section, replace item #9's full block
  with a one-line graduation note pointing at the live row, in the
  same style as the other graduations.

The other four requested items (#6, #7, #8, #10) stay where they
are — none move in this release.

### `CONSUMER_REQUIREMENT_MM.md` touchup

MM doesn't bulk-CONSTRUCT today. Add one line under "Available
upstream but not exercised by MM" pointing at the new function.
Mirrors how the 0.6.0 `rdf_load_*_to_graph` scalars were added
there.

### Exit criteria for Phase D

Reading `CONSUMER_REQUIREMENT_RS.md` top-to-bottom no longer
mentions item #9 in the "Requested" section. Reading `CHANGELOG.md`
shows a 0.8.0 entry naming the function and the return-shape
decision.

---

## Phase E — tag 0.8.0

- Bump `Cargo.toml` and `VERSION` to `0.8.0`.
- `cargo test` and `cargo test --release` both green at the bumped
  version.
- `git tag v0.8.0` and push.
- Ping RS to open `rails-semantica`'s PLAN_0.12.0 phase that
  routes `Shacl::Rules.materialise!` through `rdf_construct_many`.

---

## Risks

- **The per-CONSTRUCT SPARQL parse cost stays N×.** The savings are
  on FFI crossings and SQLite SQL parse, not on Oxigraph's SPARQL
  parse. RS's PLAN_0.12.0 should set its performance expectations
  against this. If telemetry shows the SPARQL parse is the
  bottleneck (not the FFI), a prepared-query model is needed and
  belongs in a separate plan — much bigger scope than 0.8.0.

- **JSON-array return shape vs. SQLite TEXT.** The return value
  for a large batch (e.g., 50 rules × ~100 inferred triples each =
  5000 triples × ~80 bytes = 400 KB) sits in one SQLite TEXT cell.
  SQLite handles this fine — TEXT can be GB-scale — but Ruby
  parsers may pay a non-trivial cost on a single 400 KB JSON
  envelope. Pin this in the CHANGELOG so consumers know the
  ballpark; if it bites, a streaming variant
  (`rdf_construct_many_iter` returning rows) becomes a follow-up.

- **Per-query attribution doesn't help if downstream still needs
  the engine to insert.** RS's downstream pattern is "construct →
  annotate gem-side → insert with annotations". If a future RS
  PLAN decides annotations should live in a separate named graph
  emitted by the engine, the engine-side-attribution variant
  (`rdf_construct_many_with_provenance`) becomes the right shape.
  This plan deliberately leaves room for that variant by not
  occupying its name — see "Out of scope".

- **RDF-star CONSTRUCT outputs.** Already covered by Phase B's
  reuse of the 0.7.0 `term_to_ntriples` serialiser, and pinned by
  `test_rdf_construct_many_with_rdf_star` in Phase C. No new
  surface here; just don't break it.

- **Empty array.** Pinned by
  `test_rdf_construct_many_empty_array`. The natural `Vec`
  iteration handles zero elements without special-casing; the
  `serde_json::to_string(&Vec::<String>::new())` call returns
  `"[]"` cleanly. Mention because every prior `_many` release has
  had to handle the empty-input case explicitly somewhere.

---

## Out of scope for 0.8.0

- **`rdf_construct_many_with_provenance(pairs, target_graph, predicate)`** —
  engine-side annotation emission. Defer until RS asks (see
  "Provenance shape stays out of the engine" above).

- **`rdf_construct_many_to_graph(queries, target_graph)`** — engine
  inserts CONSTRUCT outputs into a target graph and returns a
  count. Would change the function from read-only to read+write
  and re-open the provenance attribution question (which triples
  came from which query?). RS's downstream pattern (construct →
  annotate → insert) doesn't need this; if a future consumer asks
  for the write path, plan it then.

- **Prepared-query / query-plan reuse.** A model where the engine
  parses + plans each CONSTRUCT once and stores the plan handle
  for reuse across iterations. Genuinely useful for fixpoint
  workloads (SHACL Rules, OWL 2 RL) but a much larger change in
  Oxigraph's surface and in the SQL-extension wire. Out of scope;
  belongs in its own plan if/when telemetry shows the SPARQL
  parser as the bottleneck.

- **Items #6, #7, #8, #10 from the RS "Requested extensions"
  section.** Each gets its own plan when telemetry asks. #8 (DRed
  dependency index) is gated on #6 + #9; with #9 landing here, #8
  becomes a single-purpose plan rather than a co-design with #6.

- **Native rule / validator passes.** Items #6 and #7 are
  substantial native-Rust efforts that warrant their own plans
  AFTER concrete telemetry from RS PLAN_0.9.0 / PLAN_0.10.0 shows
  the SPARQL-driven shape is the bottleneck. The substrate posture
  on items #6–#10 is "revisit on first concrete bottleneck signal,"
  per the RS-doc preamble; this plan respects that for the items it
  doesn't ship.

- **Persistent RocksDB backend.** Stays deferred per PLAN_0.7.0.
  No consumer pressure.

---

## Re-numbering downstream milestones

`PLAN_0.2.0.md`'s roadmap table currently lists (after PLAN_0.7.0's
renumber):

| Version | Topic |
|---|---|
| 0.7.0 | RDF-star / SPARQL-star round-trip |
| 0.8.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.9.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB backend |

After this plan, the trailing rows shift by one:

| Version | Topic |
|---|---|
| 0.7.0 | RDF-star / SPARQL-star round-trip |
| 0.8.0 | `rdf_construct_many` (this file) |
| 0.9.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.10.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB backend |

Update the table in `PLAN_0.2.0.md` as part of Phase D's doc pass.
Do **not** edit `PLAN_0.2.0.md`'s prose — the renumbering is the
only change there.
