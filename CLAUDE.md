# CLAUDE.md — sqlite-sparql

This file guides Claude Code when working in this repository.

## Project Summary

`sqlite-sparql` is a **Rust loadable SQLite extension** that embeds the
[Oxigraph](https://github.com/oxigraph/oxigraph) RDF/SPARQL engine directly
inside SQLite. Once loaded, any SQLite connection gains native SQL functions
for inserting, querying, and serialising RDF triples, plus a read/write
virtual table (`rdf_triples`) backed by the Oxigraph in-memory store.

The primary consumer is a **Ruby on Rails application** using SQLite as its
database, where the extension is loaded via Rails 8's `extensions:` key in
`config/database.yml`.

---

## Repository Layout

```
sqlite-sparql/
├── Cargo.toml                  # Crate manifest — cdylib + rlib
├── CLAUDE.md                   # This file
├── README.md                   # User-facing documentation
├── src/
│   ├── lib.rs                  # Extension entry point (#[sqlite_entrypoint])
│   ├── error.rs                # SparqlError enum + conversions
│   ├── store.rs                # Thread-local Oxigraph Store wrapper
│   ├── functions/
│   │   ├── mod.rs
│   │   ├── rdf_triple.rs       # rdf_insert/delete/clear/count/load/dump
│   │   ├── rdf_bulk.rs         # rdf_insert_many / rdf_delete_many (0.4.0)
│   │   ├── sparql_query.rs     # sparql_query/ask/construct/update + construct_many
│   │   ├── rdf_owl_rl.rs       # rdf_owl_rl_materialise — fixpoint loop + provenance (0.9.0)
│   │   └── rdf_owl_rl/
│   │       ├── rdf_lists.rs    # rdf:first / rdf:rest list walker (0.10.0)
│   │       └── rules.rs        # 60-rule OWL 2 RL library (15 in 0.9.0, +45 in 0.10.0)
│   └── vtab/
│       ├── mod.rs
│       └── triples_vtab.rs     # rdf_triples virtual table (read/write)
├── tests/
│   └── integration_test.rs     # rusqlite-based integration tests
└── examples/
    └── demo.sql                # SQL demo script for the SQLite CLI
```

---

## Mac Development Setup

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Install SQLite (for the CLI demo)
brew install sqlite

# 3. Build the extension (debug)
cargo build

# 4. Build the extension (release — smaller, faster)
cargo build --release

# 5. Run the test suite
cargo test

# 6. Try the SQL demo
sqlite3 :memory: < examples/demo.sql
# Or interactively:
sqlite3
sqlite> .load ./target/release/libsqlite_sparql
sqlite> SELECT rdf_insert('http://example.org/alice','http://www.w3.org/1999/02/22-rdf-syntax-ns#type','http://xmlns.com/foaf/0.1/Person');
sqlite> SELECT sparql_query('SELECT ?s WHERE { ?s a <http://xmlns.com/foaf/0.1/Person> }');
```

> **macOS note:** The compiled extension will be at
> `target/release/libsqlite_sparql.dylib`.  
> On Linux it will be `target/release/libsqlite_sparql.so`.

---

## Key Design Decisions

| Decision | Rationale |
|---|---|
| **Thread-local Oxigraph store** | Matches SQLite's one-connection-per-thread model; avoids cross-thread data races |
| **N-Triples encoding for term strings** | Standard, unambiguous, works as plain SQLite TEXT |
| **JSON output for SPARQL SELECT** | Easy to consume in Ruby via `JSON.parse`; works with SQLite's `json_each()` |
| **`cdylib` + `rlib` crate types** | `cdylib` for the loadable `.so/.dylib`; `rlib` enables Rust unit tests |
| **Oxigraph 0.4.x** | Stable API; 0.5.x changed the Store API significantly — upgrade carefully |

---

## Completing the Implementation

The following items are **not yet implemented** and should be finished next:

### 1. Fix API Incompatibilities
The `sqlite-loadable` crate (v0.0.5) has a slightly unstable API. Check these
areas when the first `cargo build` runs:

- `VTab::update()` signature — the `args` slice layout may differ from what
  `triples_vtab.rs` assumes. Consult
  `sqlite_loadable::table::UpdateOperation` if it exists.
- `define_virtual_table_writeable` — verify the function name in the installed
  version with `cargo doc --open`.
- `api::value_type` / `api::ValueType` — may be named differently; check
  `sqlite_loadable::api` docs.

### 2. Named Graph Support — DONE in 0.3.0
4-arg `rdf_insert`/`rdf_delete`, 1-arg `rdf_count`, `rdf_count_all`,
HIDDEN `graph` column on the `rdf_triples` vtab, and SPARQL routing
via standard `GRAPH`/`FROM`/`FROM NAMED` clauses. See
`docs/plans/PLAN_0.3.0.md`. Blank-node graphs are deliberately rejected
to keep the boundary narrow.

### 3. SPARQL UPDATE — DONE in 0.5.0
`sparql_update(query) → INTEGER` exposes `Store::update`. Returns the
signed net change in store size (Oxigraph 0.4 doesn't surface a
first-class affected-row count; the delta is the honest summary for
single-direction updates and `inserts - deletes` for mixed modifies).
See `docs/plans/PLAN_0.5.0.md`.

### 4. RDF-star / SPARQL-star — DONE in 0.7.0
Quoted-triple terms (`<< s p o >>`) round-trip through every read and
write path. The parser side (Turtle-star / N-Triples-star) and the
SPARQL-star evaluator were already provided by Oxigraph 0.4; 0.7.0
extends the SQL boundary to encode/decode the terms instead of
stubbing them. New scalars: `rdf_triple_subject`, `rdf_triple_predicate`,
`rdf_triple_object`. `rdf_term_type` returns `"triple"`. See
`docs/plans/PLAN_0.7.0.md` and `docs/research/StarExts.md`.

### 5. Batched CONSTRUCT — DONE in 0.8.0
`rdf_construct_many(queries_json TEXT) → TEXT` evaluates an array
of CONSTRUCT queries in one FFI crossing and returns a JSON array
of per-query N-Triples blobs. Per-query attribution preserved so
consumers can annotate per-rule before inserting. CONSTRUCT stays
read-only — the engine does not write results into the store.
Driver: RS PLAN_0.12.0's Shacl Rules materialise loop. See
`docs/plans/PLAN_0.8.0.md`.

### 6. Native OWL 2 RL reasoning — DONE (15-rule subset in 0.9.0; full
###    derivation coverage in 0.10.0)
`rdf_owl_rl_materialise(asserted, inferred, options_json) → INTEGER`
runs a native Rust fixpoint loop over Oxigraph's store. 0.10.0 ships
all 60 W3C OWL 2 RL/RDF *derivation* rules across Scm / Cls / Cax /
Prp / Eq / Dt (matching `vv-graph`'s expanded
`Vv::Graph::Reasoner::Rules::OwlRl` if/when the gem graduates its
`PHASE_B_PENDING` list). With `provenance: true`, emits
`<< s p o >> prov:wasDerivedFrom <rule_iri>` +
`prov:generatedAtTime "..."^^xsd:dateTime` RDF-star annotations
(predicates operator-overridable). New 0.10.0 options:
`equality_saturation` (default `true`) gates `eq-rep-s/p/o`;
`eq_reflexive` (default **`false`** — opt-in) gates `eq-ref` which
doesn't converge under `provenance: true`. `dt-eq` / `dt-diff` are
no-ops in Oxigraph 0.4 (literal subjects not representable).
Driver: VG CR #6. See `docs/plans/PLAN_0.9.0.md`,
`docs/plans/PLAN_0.10.0.md`, and `src/functions/rdf_owl_rl/`.

### 7. OWL 2 RL inconsistency detection — DONE in 0.13.0
`rdf_owl_rl_consistent(asserted_iri, inferred_iri, options_json) → TEXT`
returns a JSON array of `{rule, s, p, o}` violation records (or `"[]"`
for consistent). Ships all 17 W3C OWL 2 RL/RDF inconsistency rules
(Prp×6, Cls×5, Cax×2, Eq×3, Dt×1) outside the monotonic-fixpoint
contract of `rdf_owl_rl_materialise`. Read-only — never inserts into
the store, never touches the dependency index. Symmetric rules emit
one record per semantic violation with deterministic lex-smaller
witness; output is globally sorted by `(rule, s, p, o)`. `dt-not-type`
validates the XSD integer family + booleans (other datatypes skip;
no false positives). No consumer signal from Vv::Graph today
(`Reasoner.consistent?` not implemented), but the engine ships the
surface so the gem can flip on whenever it grows the check. Driver:
PLAN_0.10.0 §"Inconsistency rules — deferred to a separate surface."
See `docs/plans/PLAN_0.13.0.md`, `src/functions/rdf_owl_rl/
inconsistency.rs`, and `src/functions/rdf_owl_rl_consistent.rs`.

### 8. Native SHACL Core validator pass — DONE in 0.11.0
`rdf_shacl_core_validate(data_iri, shapes_iri, report_iri,
options_json) → INTEGER` walks the data graph once per shape and
emits a W3C-conformant `sh:ValidationReport` into the report graph
in one FFI crossing. Ships the 12-constraint subset matching VG's
`Vv::Graph::Shacl::ConstraintLibrary` (sh:minCount/maxCount/
datatype/nodeKind/class/pattern/minLength/maxLength/in/hasValue/
minInclusive/maxInclusive), plus a path evaluator covering
predicate / inverse / sequence / alternative / zero-or-more /
one-or-more / zero-or-one. Report graph is cleared before each
call. Driver: VG CR #7. See `docs/plans/PLAN_0.11.0.md` and
`src/functions/rdf_shacl_core/`.

### 9. Native dependency index for DRed — DONE in 0.12.0
`rdf_dred_overdelete(inferred_iri, retracted_premises_json) → INTEGER`
consumes a side-table mapping inferred quads to their per-derivation
premise sets, populated as a write-through during
`rdf_owl_rl_materialise` when called with the new
`{"track_dependencies": true}` option. Replaces the consumer-side
O(retracted × inferred) SPARQL pattern match against a
`:derivedFrom` annotation graph with an O(log N)-per-premise reverse-
index lookup and a transitive cascade. 0.12.0 tracks the five W3C
"core derivation" rules (`scm-sco`, `scm-spo`, `eq-trans`, `cax-sco`,
`prp-spo1`); the remaining 55 rules still fire but skip the
write-through. Driver: VG CR #8. See `docs/plans/PLAN_0.12.0.md`,
`src/dependency_index.rs`, and `src/functions/rdf_dred.rs`.

### 10. Persistent Store (RocksDB backend) — DEFERRED
No consumer asks for persistence. If it lands, replace the in-memory
`Store::new()` in `store.rs` with `Store::open(path)` (Oxigraph's
RocksDB-backed persistent store) and expose the path via a
`rdf_open(path TEXT)` SQL function or an extension argument. Revive
on first consumer ask.

### 11. Rails Gem Wrapper (`sqlite-sparql-ruby`)
Create a companion Ruby gem that:
- Ships the pre-compiled `.dylib`/`.so` for common platforms
- Exposes a `SqliteSparql.load(db)` method (mirroring `sqlite-vec`'s pattern)
- Provides an ActiveRecord concern `HasRdfTriples` for model-level helpers

### 12. SPARQL Endpoint Middleware
Add a Rack/Rails middleware that exposes a `/sparql` HTTP endpoint accepting
SPARQL queries over the wire (SPARQL Protocol 1.1).

### 13. Differential dataflow at the store layer — DEFERRED
VG CR #10. Explicitly flagged "genuinely out-of-reach for incremental
engine work" in the VG CR. Revive only if MM signals a workload that
can't be served by items #6 + #9 combined.

---

## SQL Function Reference

### Triple Management

```sql
SELECT rdf_insert(subject, predicate, object);   -- returns 1
SELECT rdf_delete(subject, predicate, object);   -- returns 1
SELECT rdf_clear();                              -- returns 1
SELECT rdf_count();                              -- returns INTEGER
SELECT rdf_load_turtle(turtle_text);             -- returns count loaded
SELECT rdf_load_turtle_to_graph(turtle_text, graph_iri);    -- 0.6.0; NULL graph = default
SELECT rdf_load_ntriples(ntriples_text);         -- returns count loaded
SELECT rdf_load_ntriples_to_graph(ntriples_text, graph_iri); -- 0.6.0; NULL graph = default
SELECT rdf_load_rdfxml(rdfxml_text);             -- returns count loaded
SELECT rdf_load_rdfxml_to_graph(rdfxml_text, graph_iri);    -- 0.6.0; NULL graph = default
SELECT rdf_dump_ntriples();                      -- returns TEXT
```

### Term Utilities

```sql
SELECT rdf_term_type('<http://example.org/>');   -- 'iri'
SELECT rdf_term_type('_:b0');                    -- 'blank'
SELECT rdf_term_type('"hello"');                 -- 'literal'
SELECT rdf_term_type('<< <a> <b> <c> >>');       -- 'triple'  (0.7.0)
SELECT rdf_term_value('<http://example.org/>');  -- 'http://example.org/'
SELECT rdf_term_value('"hello"@en');             -- 'hello'
-- rdf_term_value on a triple term raises a fixed-prefix error
-- ('rdf_term_value: triple terms have no scalar value; …') so
-- callers can prefix-match for refusal envelopes.

-- 0.7.0: destructure a quoted-triple term in plain SQL
SELECT rdf_triple_subject('<< <http://e/a> <http://e/p> "x" >>');   -- '<http://e/a>'
SELECT rdf_triple_predicate('<< <http://e/a> <http://e/p> "x" >>'); -- '<http://e/p>'
SELECT rdf_triple_object('<< <http://e/a> <http://e/p> "x" >>');    -- '"x"'
-- Inside SPARQL, prefer the SUBJECT / PREDICATE / OBJECT built-ins.
```

### SPARQL Querying

```sql
-- SELECT → JSON array of objects
SELECT sparql_query('SELECT ?s ?p ?o WHERE { ?s ?p ?o }');

-- ASK → 0 or 1
SELECT sparql_ask('ASK { <http://example.org/alice> ?p ?o }');

-- CONSTRUCT → N-Triples text
SELECT sparql_construct('CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }');

-- 0.8.0: batched CONSTRUCT → JSON array of per-query N-Triples blobs
SELECT rdf_construct_many(json('[
  "CONSTRUCT { ?s <http://e/q1> ?o } WHERE { ?s ?p ?o }",
  "CONSTRUCT { ?s <http://e/q2> ?o } WHERE { ?s ?p ?o }"
]'));
-- => '["<…> <q1> <…> .\n…", "<…> <q2> <…> .\n…"]'
-- Per-query attribution preserved (i-th element is the i-th query's output).
-- Pre-flight: any parse error aborts the batch with
-- "SPARQL parse error (query index N): …" before any query evaluates.
-- Non-CONSTRUCT queries error with "rdf_construct_many: query index N is not a CONSTRUCT".
```

### Reasoning (since 0.9.0; full derivation coverage since 0.10.0)

```sql
-- Native OWL 2 RL fixpoint pass. 0.10.0 ships the full W3C derivation
-- rule set (60 rules across Scm / Cls / Cax / Prp / Eq / Dt). The
-- inconsistency-detecting rules are deferred to a future
-- rdf_owl_rl_consistent surface (no consumer signal today).
-- asserted_iri = NULL means the default graph; inferred_iri must be a
-- named graph (NULL is rejected so derived triples can't pollute default).
SELECT rdf_owl_rl_materialise(
  NULL,                         -- asserted graph
  'urn:g:catalogue:inferred',   -- inferred graph
  json('{"max_iterations": 50, "provenance": true,
         "equality_saturation": true, "eq_reflexive": false}')
);
-- => INTEGER (signed net delta in store size)
--
-- With provenance: true, every derived triple gets two RDF-star
-- annotations (since 0.7.0):
--   << <s> <p> <o> >> prov:wasDerivedFrom <urn:semantica:rule:scm-sco>
--   << <s> <p> <o> >> prov:generatedAtTime "2026-05-25T20:02:43Z"^^xsd:dateTime
--
-- Options (all optional, defaults match vv-graph's Reasoner convention):
--   max_iterations      : int   (default 50)
--   provenance          : bool  (default false)
--   derived_by_iri      : str   (default "http://www.w3.org/ns/prov#wasDerivedFrom")
--   derived_at_iri      : str   (default "http://www.w3.org/ns/prov#generatedAtTime")
--   rule_iri_prefix     : str   (default "urn:semantica:rule:")
--   equality_saturation : bool  (default true)  — 0.10.0; gates eq-rep-s/p/o
--   eq_reflexive        : bool  (default false) — 0.10.0; gates eq-ref
--                                                 (off by default — non-convergent
--                                                  under provenance: true)
--
-- Error envelopes (fixed-prefix for consumer pattern-matching):
--   "rdf_owl_rl_materialise: inferred_iri must be a named graph …"
--   "rdf_owl_rl_materialise: fixpoint not reached after N iterations"
--   "rdf_owl_rl_materialise: rule <id> error at iteration N: …"
--
-- 0.10.0 limitations:
--   dt-eq / dt-diff currently emit nothing (Oxigraph 0.4's Subject
--     enum has no Literal variant; the W3C rule emits literal-subject
--     sameAs / differentFrom triples which can't be constructed).
```

### Validation (since 0.11.0)

```sql
-- Native SHACL Core validator pass. 0.11.0 ships the 12-constraint
-- subset matching vv-graph's Vv::Graph::Shacl::ConstraintLibrary, plus
-- a path evaluator covering predicate / inverse / sequence / alternative
-- / zero-or-more / one-or-more / zero-or-one. Targets: targetClass,
-- targetNode, targetSubjectsOf, targetObjectsOf.
SELECT rdf_shacl_core_validate(
  'urn:g:data',     -- data graph (NULL = default graph)
  'urn:g:shapes',   -- shapes graph (required)
  'urn:g:report',   -- report graph (required; cleared before write)
  json('{"max_violations": 10000, "provenance": false}')
);
-- => INTEGER (violation count; 0 = conforming)
--
-- Constraint coverage (12, matches VG ConstraintLibrary):
--   sh:minCount, sh:maxCount, sh:datatype, sh:nodeKind, sh:class,
--   sh:pattern (+ sh:flags i/s/m/x), sh:minLength, sh:maxLength,
--   sh:in, sh:hasValue, sh:minInclusive, sh:maxInclusive.
--
-- Report graph schema: W3C-conformant sh:ValidationReport with one
-- sh:ValidationResult per violation, carrying sh:focusNode,
-- sh:resultPath, sh:value (when applicable), sh:sourceShape,
-- sh:sourceConstraintComponent, sh:resultSeverity (always
-- sh:Violation in 0.11.0), sh:resultMessage.
--
-- Options (all optional; defaults match vv-graph's Shacl convention):
--   max_violations   : int   (default 10_000)
--   provenance       : bool  (default false) — adds :reportedBy / :reportedAt
--   reported_by_iri  : str   (default "urn:semantica:shacl:reportedBy")
--   reported_at_iri  : str   (default "http://www.w3.org/ns/prov#generatedAtTime")
--   shape_iri_prefix : str   (default "urn:semantica:shape:")
--                            — prefix for blank-node shape IRIs in sh:sourceShape
--
-- Error envelopes (fixed-prefix for consumer pattern-matching):
--   "rdf_shacl_core_validate: shapes_iri must be a named graph …"
--   "rdf_shacl_core_validate: report_iri must be a named graph …"
--   "rdf_shacl_core_validate: violation count exceeded max_violations (N)"
--   "rdf_shacl_core_validate: sh:path must be an IRI or blank-node structure, …"
--   "rdf_shacl_core_validate: property shape <…> has no sh:path"
--
-- Out of scope for 0.11.0:
--   - SHACL-SPARQL constraints (sh:sparql) — different evaluation model.
--   - SHACL Rules (sh:rule) — routes through 0.8.0 rdf_construct_many.
--   - The remaining ~18 SHACL Core constraints in VG's PHASE_B_PENDING.
--   - SHACL Advanced (sh:function, sh:expression).
```

### OWL 2 RL inconsistency detection (since 0.13.0)

```sql
-- Read-only pass over the 17 W3C OWL 2 RL/RDF inconsistency rules.
-- Returns a JSON array of {rule, s, p, o} witness records, or "[]"
-- when the graphs are consistent.
SELECT rdf_owl_rl_consistent(
  NULL,                         -- asserted graph (NULL = default graph)
  'urn:g:catalogue:inferred',   -- inferred graph (required)
  json('{"max_violations": 10000}')
);
-- => "[]"  when consistent, else e.g.:
-- '[{"rule":"cax-dw","s":"<urn:alice>","p":"<…#type>","o":"<urn:Animal>"},
--   {"rule":"prp-irp","s":"<urn:bob>","p":"<urn:parentOf>","o":"<urn:bob>"}]'

-- Consume from SQL via json_each:
SELECT json_extract(value, '$.rule') AS rule,
       json_extract(value, '$.s')    AS s,
       json_extract(value, '$.p')    AS p,
       json_extract(value, '$.o')    AS o
FROM   json_each(rdf_owl_rl_consistent(NULL, 'urn:g:inferred', '{}'));
```

Rule coverage (all 17 W3C OWL 2 RL inconsistency rules):

- **Prp** (6): `prp-irp`, `prp-asyp`, `prp-pdw`, `prp-adp`,
  `prp-npa1`, `prp-npa2`.
- **Cls** (5): `cls-nothing2`, `cls-com`, `cls-maxc1`,
  `cls-maxqc1`, `cls-maxqc2`.
- **Cax** (2): `cax-dw`, `cax-adc`.
- **Eq** (3): `eq-diff1`, `eq-diff2`, `eq-diff3`.
- **Dt** (1): `dt-not-type` (XSD integer family + booleans;
  other datatypes skip).

Symmetric rules (`cax-dw`, `prp-asyp`, `cls-com`, `eq-diff1`,
`prp-pdw`, `prp-adp`, `cax-adc`, `eq-diff2`, `eq-diff3`) emit
one record per semantic violation with lex-smaller witness.
Output is globally sorted — byte-identical across runs.

Options (all optional):
- `max_violations: usize` (default `10_000`) — exceeding aborts
  with a fixed-prefix error (no silent truncate, matches SHACL).

Error envelopes (fixed-prefix for consumer pattern-matching):
- `rdf_owl_rl_consistent: inferred_iri must be a named graph …`
- `rdf_owl_rl_consistent: options_json: <serde error>`
- `rdf_owl_rl_consistent: violation count exceeded max_violations (N)`
- `rdf_owl_rl_consistent: rule <id> error: <message>`

Read-only: never inserts into the store, never touches the
dependency index. Compose with `rdf_owl_rl_materialise` by
calling materialise first then consistent — saturated inferences
help the inconsistency rules find indirect contradictions.

### Incremental reasoning with DRed (since 0.12.0)

```sql
-- Step 1: populate the dependency index during materialise.
-- `track_dependencies` defaults to false (extra allocation cost);
-- turn it on only when a DRed cycle follows.
SELECT rdf_owl_rl_materialise(
  NULL,                         -- asserted graph (default)
  'urn:g:catalogue:inferred',   -- inferred graph
  json('{"track_dependencies": true}')
);

-- Step 2 (consumer): retract one or more asserted-graph premises
-- the usual way (rdf_delete, sparql_update, rdf_triples DML).

-- Step 3: over-delete every inferred quad whose every derivation
-- became invalid after step 2. Cascades transitively.
SELECT rdf_dred_overdelete(
  'urn:g:catalogue:inferred',
  json('[
    ["http://example.org/B",
     "http://www.w3.org/2000/01/rdf-schema#subClassOf",
     "http://example.org/C"]
  ]')
);
-- => INTEGER (over-deleted count; 0 = nothing depended on those premises)

-- Step 4 (consumer): re-materialise to pick up anything still
-- derivable from the remaining facts. Repopulates the index for
-- the next DRed cycle.
SELECT rdf_owl_rl_materialise(NULL, 'urn:g:catalogue:inferred',
  json('{"track_dependencies": true}'));
```

Tracked rules in 0.12.0: `scm-sco`, `scm-spo`, `eq-trans`,
`cax-sco`, `prp-spo1` — the five W3C "core derivation" shapes.
Other 55 rules fire as usual but don't write through; expansion
mechanical, waits on a consumer pull.

Per-derivation tracking (not the union sketched in PLAN_0.12.0):
an inferred quad with multiple independent derivations survives
partial retracts as long as one derivation's premise set stays
intact.

Error envelopes (fixed-prefix for consumer pattern-matching):
- `"rdf_dred_overdelete: inferred_iri must be a named graph …"`
- `"rdf_dred_overdelete: inferred_iri is required (NULL not allowed)"`
- `"rdf_dred_overdelete: retracted_premises_json parse error: …"`
- `"rdf_dred_overdelete: retracted_premises_json must be a JSON array …"`
- `"rdf_dred_overdelete: row N must have 3 or 4 elements …"`
- `"rdf_dred_overdelete: no dependency index — re-run
  rdf_owl_rl_materialise with track_dependencies: true"`

`rdf_clear()` clears the dependency index in lockstep with the
store. The index is in-memory and process-scoped — every cold
start needs a fresh tracking materialise.

### Virtual Table

```sql
CREATE VIRTUAL TABLE triples USING rdf_triples();

-- Read
SELECT * FROM triples;
SELECT subject FROM triples WHERE predicate = '<http://xmlns.com/foaf/0.1/name>';

-- Write
INSERT INTO triples VALUES (subject_text, predicate_text, object_text);
DELETE FROM triples WHERE subject = '...' AND predicate = '...' AND object = '...';
```

---

## Rails Integration

```yaml
# config/database.yml  (Rails 8+)
default: &default
  adapter: sqlite3
  extensions:
    - "<%= Rails.root.join('vendor/sqlite/libsqlite_sparql') %>"
```

```ruby
# app/models/concerns/has_rdf_triples.rb
module HasRdfTriples
  extend ActiveSupport::Concern

  included do
    after_create  :sync_to_rdf_store
    after_destroy :remove_from_rdf_store
  end

  def sparql(query)
    result = self.class.connection.select_value(
      "SELECT sparql_query(?)", nil, [[nil, query]]
    )
    JSON.parse(result)
  end

  private

  def sync_to_rdf_store
    # Override in model to define which triples to assert
  end

  def remove_from_rdf_store
    # Override in model to retract triples on destroy
  end
end
```

---

## Testing

```bash
# Unit + integration tests
cargo test

# With output
cargo test -- --nocapture

# Single test
cargo test test_sparql_select -- --nocapture
```

> **Footgun — release-mode tests don't rebuild the cdylib.** Integration
> tests load the compiled `.dylib` via the `SQLITE_SPARQL_CDYLIB` env
> var that `build.rs` sets to `target/{debug,release}/libsqlite_sparql.dylib`.
> The integration test crate depends on the *path*, not on the cdylib
> as a build artifact — so `cargo test --release` will happily reuse a
> stale release dylib from a previous build and produce ghost failures
> (e.g. tests asserting new behaviour against the old binary). Always
> run `cargo build --release` first, or invalidate `target/release/`,
> before `cargo test --release`.

---

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `sqlite-loadable` | 0.0.5 | SQLite extension framework |
| `oxigraph` | 0.4 | Embedded RDF/SPARQL engine |
| `serde` / `serde_json` | 1 | JSON serialisation of SPARQL results |
| `thiserror` | 1 | Ergonomic error types |
| `rusqlite` (dev) | 0.32 | Integration test harness |
| `tempfile` (dev) | 3 | Temporary files in tests |
