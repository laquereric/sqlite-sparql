# sqlite-sparql

A [SQLite loadable extension](https://www.sqlite.org/loadext.html) that embeds
[Oxigraph](https://github.com/oxigraph/oxigraph) to provide **native RDF triple
storage and SPARQL querying** directly within SQLite — with no external server
required.

Built with [`sqlite-loadable-rs`](https://github.com/asg017/sqlite-loadable-rs),
the premier framework for writing SQLite extensions in Rust.

---

## Features

- **`rdf_insert(s, p, o)`** / **`rdf_insert(s, p, o, graph)`** — Insert
  RDF triples using N-Triples term syntax. The 4-arg form routes the
  triple into a named graph; `graph = NULL` is the default graph.
- **`rdf_delete(s, p, o)`** / **`rdf_delete(s, p, o, graph)`** — Delete
  triples; same `graph` semantics as `rdf_insert`.
- **`rdf_insert_many(json)`** / **`rdf_delete_many(json)`** — Batched
  write of a JSON array of triples. Each row is `[s, p, o]` or
  `[s, p, o, graph]`. Returns the count actually inserted / deleted
  (RDF set semantics — duplicates and no-ops don't count).
- **`rdf_clear()`** — Empty the in-memory store
- **`rdf_count()`** / **`rdf_count(graph)`** — Count triples in the
  default graph (zero-arg) or in a named graph (`NULL` = default).
- **`rdf_count_all()`** — Count triples across every graph
- **`rdf_load_turtle(text)`** / **`rdf_load_turtle_to_graph(text, graph)`** —
  Bulk-load from Turtle format. The 2-arg form routes every parsed triple
  into the named graph `graph` (`NULL` → default graph, identical to the
  1-arg form).
- **`rdf_load_ntriples(text)`** / **`rdf_load_ntriples_to_graph(text, graph)`** —
  Bulk-load from N-Triples format, same graph routing convention.
- **`rdf_load_rdfxml(text)`** / **`rdf_load_rdfxml_to_graph(text, graph)`** —
  Bulk-load from RDF/XML format, same graph routing convention.
- **`rdf_dump_ntriples()`** — Serialise all triples as N-Triples
- **`rdf_term_type(term)`** — Returns `"iri"`, `"blank"`, or `"literal"`
- **`rdf_term_value(term)`** — Extracts the plain string value from a term
- **`sparql_query(query)`** — Execute a SPARQL SELECT → JSON array. SPARQL
  1.1 `FROM <graph>`, `FROM NAMED <graph>`, and `GRAPH <graph> { … }`
  clauses route the query through Oxigraph unchanged.
- **`sparql_ask(query)`** — Execute a SPARQL ASK → `0` or `1`
- **`sparql_construct(query)`** — Execute a SPARQL CONSTRUCT → N-Triples text
- **`sparql_update(query)`** — Execute any SPARQL 1.1 UPDATE form
  (`INSERT DATA`, `DELETE DATA`, `INSERT/DELETE … WHERE`, `CLEAR`,
  `CREATE`, `DROP`). Returns the **signed net change** in store size:
  `+N` for inserts, `-N` for deletes, `inserts - deletes` for mixed
  modifies (so a balanced mixed UPDATE may return `0`).
- **`rdf_triples` virtual table** — Read/write SQL view of the triple
  store. Columns: `subject`, `predicate`, `object`, plus a HIDDEN `graph`
  column (default graph = `NULL`). `SELECT *` and the 3-column `INSERT
  VALUES` form keep the 0.2.0 shape; name the `graph` column explicitly
  to read or write named graphs.

---

## Quick Start

### Build

```bash
# macOS
cargo build --release
# Extension: target/release/libsqlite_sparql.dylib

# Linux
cargo build --release
# Extension: target/release/libsqlite_sparql.so
```

### SQLite CLI

```sql
-- Load the extension
.load ./target/release/libsqlite_sparql

-- Insert some triples (N-Triples term syntax)
SELECT rdf_insert(
  'http://example.org/alice',
  'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
  'http://xmlns.com/foaf/0.1/Person'
);
SELECT rdf_insert(
  'http://example.org/alice',
  'http://xmlns.com/foaf/0.1/name',
  '"Alice"'
);

-- Count triples
SELECT rdf_count();  -- 2

-- SPARQL SELECT → JSON
SELECT sparql_query(
  'SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }'
);
-- [{"name":"\"Alice\""}]

-- SPARQL ASK
SELECT sparql_ask('ASK { <http://example.org/alice> ?p ?o }');  -- 1

-- Virtual table
CREATE VIRTUAL TABLE triples USING rdf_triples();
SELECT * FROM triples;
```

### Named graphs

```sql
-- 4-arg form routes into a named graph
SELECT rdf_insert(
  'http://example.org/alice',
  'http://xmlns.com/foaf/0.1/name',
  '"Alice"',
  'urn:graph:bhphoto'
);

-- Count by graph
SELECT rdf_count();                       -- default graph only
SELECT rdf_count('urn:graph:bhphoto');    -- named graph
SELECT rdf_count_all();                   -- every graph

-- SPARQL routing via standard GRAPH / FROM clauses
SELECT sparql_query(
  'SELECT ?s WHERE { GRAPH <urn:graph:bhphoto> { ?s ?p ?o } }'
);

-- Virtual table: name the graph column to read or write it
INSERT INTO triples(subject, predicate, object, graph)
VALUES ('http://example.org/x', 'http://example.org/p', '"v"', 'urn:graph:bhphoto');

SELECT subject FROM triples WHERE graph = 'urn:graph:bhphoto';
```

### Batched writes

For thousands of triples in one shot, `rdf_insert_many` takes a single
JSON-array argument and loops on the Rust side via Oxigraph's bulk
loader — materially faster than N separate `rdf_insert` calls because
the FFI crossing and SQL parse happen once instead of N times.

```sql
SELECT rdf_insert_many('[
  ["http://example.org/alice", "http://xmlns.com/foaf/0.1/name", "\"Alice\""],
  ["http://example.org/bob",   "http://xmlns.com/foaf/0.1/name", "\"Bob\"",   "urn:graph:bhphoto"],
  ["http://example.org/carol", "http://xmlns.com/foaf/0.1/name", "\"Carol\"", null]
]');
-- → 3 (count of newly-inserted triples; duplicates and no-ops don't count)

SELECT rdf_delete_many('[
  ["http://example.org/alice", "http://xmlns.com/foaf/0.1/name", "\"Alice\""]
]');
-- → 1
```

A malformed row (wrong arity, non-string element, invalid IRI) aborts
the whole batch with a row-indexed error message; nothing is written.

### SPARQL UPDATE

For arbitrary SPARQL 1.1 UPDATE — anything beyond `INSERT DATA` /
`DELETE DATA` that the scalar surface and `rdf_insert_many` can
already express — use `sparql_update`:

```sql
SELECT sparql_update(
  'INSERT { ?p <http://example.org/derived_at> ?nowstr }
   WHERE  { ?p a <http://xmlns.com/foaf/0.1/Person>
            BIND(STR(NOW()) AS ?nowstr) }'
);
-- → +N  (one new triple per matching person)

SELECT sparql_update('CLEAR GRAPH <urn:graph:bhphoto>');
-- → -N  (count cleared)
```

Return value: signed net change in store size. Positive for
insert-only, negative for delete-only, `inserts - deletes` for mixed
operations (so a balanced mixed UPDATE can return `0` even though
both halves ran). Observe the store with `rdf_count` / `sparql_ask`
when you need to assert state rather than delta.

### Batched CONSTRUCT (since 0.8.0)

For fixpoint workloads (SHACL Rules, OWL 2 RL reasoning) that
issue many CONSTRUCTs per iteration, `rdf_construct_many` runs an
array of CONSTRUCT queries in one FFI crossing and returns a JSON
array of per-query N-Triples blobs:

```sql
SELECT rdf_construct_many(
  json('[
    "CONSTRUCT { ?p mm:tier mm:VIP }
       WHERE  { ?p mm:total_orders ?n . FILTER(?n > 100) }",
    "CONSTRUCT { ?p mm:availability \"in_stock\" }
       WHERE  { ?p mm:inventory ?n . FILTER(?n > 0) }"
  ]')
);
-- => '["<urn:p:1> <…> <…> .\\n…", "<urn:p:7> <…> <…> .\\n…"]'
```

Per-query attribution is preserved (the `i`-th element of the
returned array is the output of the `i`-th input query), so
consumers can attach `:derivedBy <rule_iri>` annotations rule by
rule before inserting. CONSTRUCT is read-only — the engine does
not insert results into the store; the caller decides where each
blob lands. Pre-flight: any parse error aborts the whole batch
with the prefix `SPARQL parse error (query index N):` before any
query evaluates.

### OWL 2 RL native reasoning (since 0.9.0; full derivation coverage since 0.10.0)

For fixpoint reasoning workloads (OWL 2 RL closures), `rdf_owl_rl_materialise`
runs a native Rust fixpoint loop in one FFI crossing in place of the
gem-side per-rule `sparql_update` round-trip:

```sql
SELECT rdf_owl_rl_materialise(
  NULL,                       -- asserted graph (NULL = default)
  'urn:g:catalogue:inferred', -- inferred graph (must be a named graph)
  json('{"max_iterations": 50, "provenance": true,
         "equality_saturation": true, "eq_reflexive": false}')
);
-- => INTEGER (signed net delta in store size)
```

**0.10.0 ships the full W3C OWL 2 RL/RDF derivation rule set — 60
rules across Scm / Cls / Cax / Prp / Eq / Dt tables.** The
*inconsistency*-detecting rules (~15 W3C rules that conclude
"false" rather than derive a triple) are not in this release; a
separate `rdf_owl_rl_consistent` surface is queued for a future
release. See `docs/plans/PLAN_0.10.0.md` § "Inconsistency rules —
deferred to a separate surface" for the rationale.

Two new options on top of 0.9.0's:

- `equality_saturation` (default `true`) — short-circuits `eq-rep-s` /
  `eq-rep-p` / `eq-rep-o` when `false`, for graphs with heavy
  `owl:sameAs` linkage that would otherwise blow up the closure.
- `eq_reflexive` (default `false`) — opt-in for `eq-ref` (reflexive
  `?term owl:sameAs ?term` for every term in every quad). Off by
  default because `eq-ref` + `provenance: true` doesn't converge —
  annotation triples contain new quoted-triple terms that the rule
  itself then derives reflexives for, ad infinitum within the
  iteration cap.

Two `Dt` rules (`dt-eq`, `dt-diff`) ship as functional no-ops
because Oxigraph 0.4's model rejects literals in subject position;
they revive once the model upgrades. The remaining 58 derivation
rules fire fully.

With `"provenance": true`, every derived triple is annotated with
two RDF-star quads (since 0.7.0):

```
<< <s> <p> <o> >> <http://www.w3.org/ns/prov#wasDerivedFrom>
    <urn:semantica:rule:scm-sco> .
<< <s> <p> <o> >> <http://www.w3.org/ns/prov#generatedAtTime>
    "2026-05-25T20:02:43Z"^^xsd:dateTime .
```

The predicate IRIs and rule-IRI prefix are operator-overridable via
`options.derived_by_iri` / `derived_at_iri` / `rule_iri_prefix`.
Defaults match the `vv-graph` `Vv::Graph::Reasoner` convention so
the engine + gem produce identical closures.

`inferred_iri = NULL` is rejected — derived triples mixing into the
default graph would erase the asserted-vs-derived distinction OWL
reasoning depends on. Pre-flight: if the fixpoint isn't reached
within `max_iterations` (default 50), error with the prefix
`rdf_owl_rl_materialise: fixpoint not reached after N iterations`
and leave the partially-derived state in the inferred graph for
inspection.

### SHACL Core validation (since 0.11.0)

For validation workloads, `rdf_shacl_core_validate` evaluates a
SHACL Core shapes graph against a data graph in one FFI crossing,
emitting a W3C-conformant `sh:ValidationReport` into a named
report graph:

```sql
SELECT rdf_shacl_core_validate(
  'urn:g:data',       -- data graph (NULL = default graph)
  'urn:g:shapes',     -- shapes graph (required)
  'urn:g:report',     -- report graph (required; cleared before write)
  '{}'                -- options JSON
);
-- => INTEGER (violation count; 0 = conforming)
```

0.11.0 ships the 12-constraint subset matching `vv-graph`'s
`Vv::Graph::Shacl::ConstraintLibrary`: `sh:minCount`,
`sh:maxCount`, `sh:datatype`, `sh:nodeKind`, `sh:class`,
`sh:pattern` (+ `sh:flags`), `sh:minLength`, `sh:maxLength`,
`sh:in`, `sh:hasValue`, `sh:minInclusive`, `sh:maxInclusive`.
The path evaluator handles predicate, inverse (`sh:inversePath`),
sequence (`( :p1 :p2 )`), alternative (`sh:alternativePath`),
zero-or-more / one-or-more / zero-or-one paths. Targets:
`sh:targetClass`, `sh:targetNode`, `sh:targetSubjectsOf`,
`sh:targetObjectsOf`.

The report graph is **cleared** before each call — re-validating
overwrites rather than accumulates. Documented loudly because it
deviates from the "engine emits, consumer decides where it lands"
posture: the report is the call's own output, so engine-managed
graph state is acceptable.

Options (all optional; defaults pin parity with `vv-graph`):

| Option | Default | Purpose |
|---|---|---|
| `max_violations` | `10000` | Safety guard; the call aborts with a fixed-prefix error once exceeded |
| `provenance` | `false` | Adds `:reportedBy` and `:reportedAt` triples on each `sh:ValidationResult` |
| `reported_by_iri` | `urn:semantica:shacl:reportedBy` | Predicate for the "reported by" provenance triple |
| `reported_at_iri` | `http://www.w3.org/ns/prov#generatedAtTime` | Predicate for the report timestamp |
| `shape_iri_prefix` | `urn:semantica:shape:` | Prefix synthesised for blank-node shape IRIs in `sh:sourceShape` |

`shapes_iri = NULL` and `report_iri = NULL` are both rejected with
fixed-prefix errors so consumers can pattern-match. `data_iri =
NULL` means the default graph (same convention as
`rdf_owl_rl_materialise`).

### OWL 2 RL inconsistency detection (since 0.13.0)

For consistency-checking workloads, `rdf_owl_rl_consistent` runs a
read-only pass over the 17 W3C OWL 2 RL/RDF *inconsistency* rules —
the "false"-deriving siblings of the 60 derivation rules in
`rdf_owl_rl_materialise`. Returns a JSON array of `{rule, s, p, o}`
witness records, or `"[]"` when the graphs are consistent:

```sql
-- Optional: materialise first so the inconsistency rules can find
-- indirect contradictions through the inferred closure.
SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}');

-- Then check for inconsistency.
SELECT rdf_owl_rl_consistent(
  NULL,               -- asserted graph (NULL = default graph)
  'urn:g:inferred',   -- inferred graph (required)
  '{}'
);
-- => "[]"   when consistent
-- => '[{"rule":"cax-dw","s":"<urn:alice>","p":"<…#type>","o":"<urn:Animal>"}, …]'

-- Drive directly from SQL via json_each:
SELECT json_extract(value, '$.rule') AS rule,
       json_extract(value, '$.s')    AS s,
       json_extract(value, '$.p')    AS p,
       json_extract(value, '$.o')    AS o
FROM   json_each(rdf_owl_rl_consistent(NULL, 'urn:g:inferred', '{}'));
```

Rule coverage (all 17 W3C inconsistency rules):

| Group | Rules |
|---|---|
| **Prp** (6) | `prp-irp`, `prp-asyp`, `prp-pdw`, `prp-adp`, `prp-npa1`, `prp-npa2` |
| **Cls** (5) | `cls-nothing2`, `cls-com`, `cls-maxc1`, `cls-maxqc1`, `cls-maxqc2` |
| **Cax** (2) | `cax-dw`, `cax-adc` |
| **Eq** (3) | `eq-diff1`, `eq-diff2`, `eq-diff3` |
| **Dt** (1) | `dt-not-type` (XSD integer family + booleans) |

Symmetric rules (`cax-dw`, `prp-asyp`, `cls-com`, `eq-diff*`,
`prp-pdw`, `prp-adp`, `cax-adc`) emit one record per semantic
violation — the witness commits to the lex-smaller participant
by N-Triples form. Output is globally sorted by `(rule, s, p,
o)` so two back-to-back calls on the same store produce
byte-identical JSON.

The function is read-only: never inserts into the store, never
touches the dependency index. Composing with materialise +
DRed is a natural pattern — materialise to saturate, check
consistency, and if violations appear, retract premises and
re-DRed.

Options (all optional):

| Option | Default | Purpose |
|---|---|---|
| `max_violations` | `10_000` | Safety cap; exceeding aborts with a fixed-prefix error (no silent truncate — matches `rdf_shacl_core_validate`) |

`inferred_iri = NULL` is rejected with a fixed-prefix error so
consumers can pattern-match. `asserted_iri = NULL` means the
default graph.

`dt-not-type` validates the XSD integer family and booleans in
0.13.0. Decimal, double, dateTime, anyURI, and the string
family skip validation — no false positives. Custom datatype
IRIs are opaque to OWL 2 RL and skip too.

### Incremental reasoning with DRed (since 0.12.0)

For incremental reasoning workloads, `rdf_dred_overdelete` is the
"delete-and-rederive" primitive: given a set of premises that the
consumer has retracted, it walks the native dependency index and
removes every inferred quad whose every derivation became invalid
— transitively, so a removed inferred quad cascades to anything it
itself supports.

```sql
-- Step 1: enable tracking on the materialise pass.
SELECT rdf_owl_rl_materialise(
  NULL,                         -- asserted graph (default)
  'urn:g:catalogue:inferred',   -- inferred graph
  '{"track_dependencies": true}'
);

-- Step 2 (consumer): remove the asserted-graph premise(s).
SELECT rdf_delete(
  'http://example.org/B',
  'http://www.w3.org/2000/01/rdf-schema#subClassOf',
  'http://example.org/C'
);

-- Step 3: over-delete in one FFI crossing.
SELECT rdf_dred_overdelete(
  'urn:g:catalogue:inferred',
  json('[["http://example.org/B",
          "http://www.w3.org/2000/01/rdf-schema#subClassOf",
          "http://example.org/C"]]')
);
-- => INTEGER (count of over-deleted inferred quads)

-- Step 4 (consumer): re-materialise to fill in anything still
-- derivable from the remaining facts. Repopulates the index.
SELECT rdf_owl_rl_materialise(NULL, 'urn:g:catalogue:inferred',
  '{"track_dependencies": true}');
```

`track_dependencies` defaults to `false` — the per-derivation
allocation cost is real, so the option is opt-in. The dependency
index records derivations from the five W3C OWL 2 RL "core
derivation" rules in 0.12.0 (`scm-sco`, `scm-spo`, `eq-trans`,
`cax-sco`, `prp-spo1`); other rules fire as usual but skip the
write-through. Expansion to the remaining 55 rules is mechanical
and waits on a consumer pull.

The index is in-memory and process-scoped. `rdf_clear()` clears
it in lockstep with the store; persistence across process
restarts ties to the deferred RocksDB backend.

### Bulk Load (Turtle)

```sql
SELECT rdf_load_turtle('
  @prefix foaf: <http://xmlns.com/foaf/0.1/> .
  @prefix ex:   <http://example.org/> .

  ex:bob   a foaf:Person ; foaf:name "Bob" .
  ex:carol a foaf:Person ; foaf:name "Carol" .
');
SELECT rdf_count();  -- 4
```

---

## Rails Integration (Rails 8+)

```yaml
# config/database.yml
default: &default
  adapter: sqlite3
  extensions:
    - "<%= Rails.root.join('vendor/sqlite/libsqlite_sparql') %>"
```

```ruby
# In a Rails model or service object
class KnowledgeGraph
  def self.insert(subject:, predicate:, object:)
    ActiveRecord::Base.connection.execute(
      "SELECT rdf_insert(?, ?, ?)", subject, predicate, object
    )
  end

  def self.query(sparql)
    json = ActiveRecord::Base.connection.select_value(
      "SELECT sparql_query(?)", sparql
    )
    JSON.parse(json)
  end
end
```

## Ruby (`sqlite-sparql` gem, since 0.14.0)

For Ruby applications outside Rails — or Rails apps that want the
ergonomic helpers without inheriting the `extensions:` plumbing —
the in-tree `ruby/` subdirectory ships a companion gem:

```ruby
require "sqlite3"
require "sqlite_sparql"

db = SQLite3::Database.new(":memory:")
SqliteSparql.load(db)

# Ergonomic wrapper covering every SQL surface — triples, SPARQL,
# materialise, consistent, SHACL, DRed — returning native Ruby types.
store = SqliteSparql::Store.new(db)
store.insert("<urn:alice>", "<urn:knows>", "<urn:bob>")
store.sparql("SELECT ?o WHERE { <urn:alice> ?p ?o }")
store.materialise(inferred: "urn:g:inferred",
                  options: { "track_dependencies" => true })
store.consistent?(inferred: "urn:g:inferred")
```

For Rails models, the optional-require AR concern:

```ruby
require "sqlite_sparql/has_rdf_triples"

class Knowledge < ApplicationRecord
  include SqliteSparql::HasRdfTriples

  def sync_to_rdf_store
    rdf_store.insert(subject_iri, predicate_iri, object_iri)
  end

  def remove_from_rdf_store
    rdf_store.delete(subject_iri, predicate_iri, object_iri)
  end
end

Knowledge.sparql("SELECT ?s WHERE { ?s a <urn:Person> }")
Knowledge.materialise(inferred: "urn:g:inferred")
```

Build and install from source (not yet on RubyGems — cross-platform
pre-built binaries land in a follow-on plan):

```bash
cd ruby
bundle install
rake native       # cargo build --release + vendor host binary
rake build        # produces sqlite-sparql-0.14.0.gem
gem install ./sqlite-sparql-0.14.0.gem
```

See `ruby/README.md` for the full gem documentation.

---

## N-Triples Term Syntax

All subject, predicate, and object arguments use N-Triples encoding:

| RDF Term | Syntax | Example |
|---|---|---|
| IRI | `<iri>` | `<http://example.org/alice>` |
| Blank node | `_:id` | `_:b0` |
| Plain literal | `"value"` | `"Hello"` |
| Language literal | `"value"@lang` | `"Bonjour"@fr` |
| Typed literal | `"value"^^<datatype>` | `"42"^^<http://www.w3.org/2001/XMLSchema#integer>` |
| Quoted triple (RDF-star, since 0.7.0) | `<< <s> <p> <o> >>` | `<< <http://e/bob> <http://e/name> "Bob" >>` |

> **Caveat for `rdf_insert`/`rdf_delete`/the vtab:** subject and object
> positions take a *bare* IRI (no angle brackets) — `'http://e/alice'`,
> not `'<http://e/alice>'`. Angle brackets only appear *inside* a
> quoted-triple term (`'<< <http://e/a> <http://e/p> "x" >>'`). The
> dump and SPARQL-result outputs use full N-Triples encoding (with
> brackets).

---

## RDF-star / SPARQL-star

Quoted-triple terms (the RDF-star
[CG report](https://w3c-cg.github.io/rdf-star/cg-spec/2021-12-17.html))
round-trip through every read and write path since 0.7.0. The
substrate is Oxigraph 0.4, which already accepts Turtle-star /
N-Triples-star input and evaluates SPARQL-star — the SQL surface
just stopped throwing the terms away.

```sql
-- Load a Turtle-star body with annotation shorthand
SELECT rdf_load_turtle('
  @prefix : <http://example.org/> .
  :bob :name "Bob" {| :statedBy :alice ; :confidence "0.9" |} .
');
-- → 3 (one asserted triple + two annotation triples)

-- Insert a quoted triple as subject
SELECT rdf_insert(
  '<< <http://e/bob> <http://e/name> "Bob" >>',
  'http://e/statedBy',
  'http://e/alice'
);

-- Query it with SPARQL-star
SELECT sparql_query('
  PREFIX : <http://example.org/>
  SELECT ?val ?stater WHERE {
    :bob :name ?val {| :statedBy ?stater |} .
  }
');

-- Destructure a quoted-triple term in plain SQL
SELECT rdf_triple_subject('<< <http://e/a> <http://e/p> "x" >>');
-- → '<http://e/a>'
SELECT rdf_term_type('<< <a> <b> <c> >>');
-- → 'triple'
```

Surface delta from 0.6.x:

- **All write paths** (`rdf_insert`, `rdf_delete`, `rdf_insert_many`,
  `rdf_delete_many`, `rdf_triples` vtab `INSERT`) accept `<< s p o >>`
  in subject and object position. Predicate position stays
  IRI-only — RDF doesn't extend star to predicates.
- **All read paths** (`rdf_dump_ntriples`, `sparql_construct`,
  `sparql_query` JSON bindings, `rdf_triples` vtab `SELECT`) emit
  `<< s p o >>` for quoted-triple terms.
- **SPARQL-star** flows straight through to Oxigraph — annotation
  shorthand `{| |}`, explicit `<<>>` patterns, and the
  `TRIPLE` / `SUBJECT` / `PREDICATE` / `OBJECT` / `isTRIPLE`
  built-ins all work without any SQL-side wrapping.
- New helper scalars (since 0.7.0):
  - `rdf_term_type(term)` returns `"triple"` for a quoted triple.
  - `rdf_triple_subject(term)` / `rdf_triple_predicate(term)` /
    `rdf_triple_object(term)` extract the parts of a quoted triple
    in plain SQL. (Inside SPARQL, use the `SUBJECT` / `PREDICATE` /
    `OBJECT` built-ins.)
  - `rdf_term_value(term)` on a quoted triple raises an error with
    the prefix `rdf_term_value: triple terms have no scalar value`
    — quoted triples have three parts, not one scalar value.

Nesting (`<< << s p o >> p o >>`) round-trips through every path.

For background on why this matters (statement-about-statement
provenance, the Conformer pattern), see
`docs/research/StarExts.md`.

---

## Architecture

```
SQLite connection
      │
      │  .load libsqlite_sparql
      ▼
┌─────────────────────────────────────┐
│         sqlite-sparql extension     │
│                                     │
│  SQL functions        Virtual table │
│  ─────────────        ──────────── │
│  rdf_insert()         rdf_triples  │
│  rdf_delete()                       │
│  sparql_query()                     │
│  sparql_ask()                       │
│  sparql_construct()                 │
│             │                       │
│             ▼                       │
│   ┌──────────────────┐              │
│   │  Process-wide    │              │
│   │  Oxigraph Store  │              │
│   │  (in-memory)     │              │
│   └──────────────────┘              │
└─────────────────────────────────────┘
```

There is **one Oxigraph store per process**. Every SQLite connection on
every thread sees the same triple graph. Oxigraph 0.4's in-memory store
is internally concurrent (every mutator takes `&self`); the extension
wraps it in `OnceLock` only for lazy initialisation.

### Limitations

- **No persistence.** The store is purely in-memory — process restart
  drops every triple. The persistent RocksDB backend lands in a later
  release; until then, populate the store from a source of truth at
  boot or first access.
- **Blank-node graphs are rejected.** Oxigraph supports them; we keep
  the boundary narrow. Use IRI-named graphs.
- **`LOAD <iri>` inside `sparql_update`** would make Oxigraph fetch
  the IRI over HTTP from inside the database. The default Oxigraph
  build has no HTTP support, so `LOAD` returns an evaluation error.
  If you build Oxigraph with HTTP enabled, sandbox the database
  process accordingly.

---

## Roadmap

- [x] Named graph support (4-arg `rdf_insert`/`rdf_delete`, hidden
      `graph` column on `rdf_triples`, SPARQL `GRAPH` / `FROM`
      routing) — landed in 0.3.0
- [x] Batched insert (`rdf_insert_many` / `rdf_delete_many`) — landed
      in 0.4.0
- [x] `sparql_update(query)` for SPARQL 1.1 Update — landed in 0.5.0
- [x] Graph-scoped bulk loading (`rdf_load_*_to_graph`) — landed in 0.6.0
- [x] RDF-star / SPARQL-star round-trip — landed in 0.7.0
- [x] Batched CONSTRUCT (`rdf_construct_many`) — landed in 0.8.0
- [x] Native OWL 2 RL fixpoint pass (15-rule subset) — landed in 0.9.0
- [x] Full OWL 2 RL derivation coverage (60 rules) — landed in 0.10.0
- [x] Native SHACL Core validator pass (12-constraint subset matching
      VG `ConstraintLibrary`, 7 path forms) — landed in 0.11.0
- [x] Native dependency index for DRed (5 core derivation rules,
      `rdf_dred_overdelete` + `track_dependencies` materialise option)
      — landed in 0.12.0
- [x] OWL 2 RL inconsistency detection (`rdf_owl_rl_consistent`,
      all 17 W3C inconsistency rules) — landed in 0.13.0
- [x] Ruby gem wrapper (`ruby/` subdirectory: loader, `Store`,
      `HasRdfTriples` AR concern, 24-test minitest suite) —
      landed in 0.14.0. Cross-platform pre-built binaries +
      RubyGems publication queued as a follow-on plan.
- [ ] SPARQL Protocol HTTP endpoint middleware for Rails
- [ ] Persistent store via Oxigraph's RocksDB backend — *deferred,
      no consumer pressure; revive on first ask*
- [ ] Differential dataflow at the store layer — *deferred (VG CR
      #10 explicitly out-of-reach for incremental engine work)*

---

## Development

See [CLAUDE.md](CLAUDE.md) for detailed guidance on completing the
implementation with Claude Code.

```bash
cargo build          # debug build
cargo build --release  # release build
cargo test           # run tests
cargo doc --open     # browse API docs
```

---

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT License](LICENSE-MIT) at your option.
