# Changelog

## 0.14.0 — `sqlite-sparql-ruby` gem wrapper

Ships a companion Ruby gem under a new top-level `ruby/`
subdirectory. The engine and its first-party language wrapper
now version together — `lib/sqlite_sparql/version.rb` pins to
the same string as the engine `VERSION` file and the test
suite asserts equality (`test_version.rb`).

Driver: `CLAUDE.md` item #11 ("Rails Gem Wrapper") — the stub
that's been on the roadmap since 0.1.0. With 0.13.0's
`rdf_owl_rl_consistent` rounding out the reasoning surface, the
engine has stabilised enough for a language wrapper to make
sense. Forward-leaning ship per the project pattern.

### Surface

```ruby
require "sqlite3"
require "sqlite_sparql"

db = SQLite3::Database.new(":memory:")
SqliteSparql.load(db)

# Now every engine SQL function is callable from this connection:
db.execute("SELECT rdf_insert(?, ?, ?)",
           ["<urn:a>", "<urn:b>", "<urn:c>"])
db.get_first_value("SELECT rdf_count()")  # => 1
```

Plus the ergonomic `SqliteSparql::Store` wrapper that covers
every SQL surface — `#insert` / `#delete` / `#sparql` / `#ask` /
`#construct` / `#update` / `#load_turtle` / `#materialise` /
`#consistent?` / `#consistency_violations` / `#shacl_validate` /
`#dred_overdelete` — returning native Ruby types (Integer,
Array<Hash>, Boolean, String) instead of forcing JSON parses
at the call site.

Plus `SqliteSparql::HasRdfTriples` — an optional-require
ActiveRecord concern that wires class-level delegators
(`Model.sparql`, `Model.materialise`, `Model.consistent?`) plus
`after_create` / `after_destroy` lifecycle hooks that call the
model's own `sync_to_rdf_store` / `remove_from_rdf_store`
instance methods if defined.

### Loader internals

The Ruby `sqlite3` gem 2.x's `db.load_extension(path)` dropped
the explicit-entrypoint parameter and now relies on SQLite's
filename-based auto-derivation. SQLite computes the entrypoint
as `sqlite3_<basename>_init` where `<basename>` is the cdylib
filename minus `lib` prefix and file extension.

The engine's entrypoint is `sqlite3_sqlitesparql_init` (no
underscore between "sqlite" and "sparql"), so the vendored
cdylib is **renamed** at vendor-copy time:
`libsqlite_sparql.{dylib,so,dll}` → `libsqlitesparql.{ext}`.
`rake native` does this rename. For dev workflows pointing at
`target/release/libsqlite_sparql.{ext}` via
`ENV["SQLITE_SPARQL_CDYLIB"]`, the loader has a dev-rewrap
fallback that hardlinks/copies to a temp `libsqlitesparql.{ext}`
basename on first use — memoised per process.

### Directory layout

```
ruby/
├── Gemfile
├── Rakefile             # rake native | rake build | rake test
├── sqlite-sparql.gemspec
├── README.md
├── lib/
│   ├── sqlite_sparql.rb         # entrypoint + loader
│   └── sqlite_sparql/
│       ├── version.rb           # pinned to engine VERSION
│       ├── store.rb             # ergonomic wrapper
│       └── has_rdf_triples.rb   # AR concern (optional require)
├── vendor/                      # .gitignored; populated by rake native
│   └── <arch>-<os>/
│       └── libsqlitesparql.{dylib,so,dll}
└── test/                        # minitest; 24 runs, 44 assertions
    ├── test_helper.rb
    ├── test_loader.rb
    ├── test_store.rb
    ├── test_has_rdf_triples.rb
    └── test_version.rb
```

### Build prerequisites

The gem requires a local `cargo build --release` of the engine
to produce the cdylib that `rake native` then vendors. The
gemspec does not list Rust as a dependency — it's assumed the
engine repo has been built. Cross-platform fat-gem distribution
(mac-arm64 + mac-x86_64 + linux-x86_64 + linux-arm64 +
linux-musl + windows-x86_64) via GitHub Releases is the next
plan; until then, consumers either build from source or wait.

### Tests

`bundle exec rake test` runs 24 minitests with 44 assertions
covering:

- Loader (idempotency, path resolution, smoke round-trip)
- Store (insert/delete/count, named graphs, SPARQL select/ask/
  construct, batched insert/delete, Turtle loading, OWL 2 RL
  materialise, OWL 2 RL inconsistency, SHACL validate, DRed
  overdelete, term helpers)
- AR concern (lifecycle hooks fire, class delegators work for
  sparql/materialise/consistent)
- Version (gem VERSION matches engine VERSION)

### Not in 0.14.0

- **RubyGems publication.** Holds until cross-platform binary
  distribution lands — publishing a gem that requires a Rust
  toolchain on the consumer side is a non-starter.
- **Cross-platform binary distribution.** A
  `.github/workflows/release-binaries.yml` that builds for all
  major platforms on tag push and uploads to GitHub Releases.
  Tied to a downstream plan (PLAN_0.14.1 or PLAN_0.15.x).
- **Rails generator.** `bin/rails g sqlite_sparql:install` to
  drop a `config/initializers/sqlite_sparql.rb`. Future polish.
- **JRuby / TruffleRuby.** MRI-only in 0.14.0.
- **SPARQL HTTP endpoint middleware.** That's PLAN_0.15.0.

### vv-graph posture

`vv-graph` continues to load the extension via Rails 8's
`extensions:` config key today. The new gem doesn't replace
that — it complements it for non-Rails Ruby consumers who
don't have a `config/database.yml`. VG could potentially
switch to `SqliteSparql.load(db)` to gain the ergonomic
helpers, but there's no consumer ask; current path stays.

## 0.13.0 — Native OWL 2 RL inconsistency detection

`rdf_owl_rl_consistent(asserted_iri, inferred_iri, options_json) →
TEXT` lands as a new top-level scalar returning a JSON array of
`{rule, s, p, o}` violation records (or `"[]"` for consistent).
Ships the 17 W3C OWL 2 RL/RDF *inconsistency* rules that
PLAN_0.10.0 deliberately deferred — the "false"-deriving rules
that sit outside `rdf_owl_rl_materialise`'s monotonic-fixpoint
contract.

Driver: PLAN_0.10.0 §"Inconsistency rules — deferred to a separate
surface" explicitly nominated this slot. No `Vv::Graph::Reasoner.
consistent?` caller yet; the engine ships the surface so the gem
can flip on whenever it grows the check. Forward-leaning ship per
the same posture as 0.9.0 / 0.10.0 / 0.11.0 / 0.12.0.

### Rule coverage (all 17 rules ship)

| Group | Rules | Witness shape |
|---|---|---|
| **Prp** (6) | `prp-irp`, `prp-asyp`, `prp-pdw`, `prp-adp`, `prp-npa1`, `prp-npa2` | `(x, p, y)` triple that violates the property axiom |
| **Cls** (5) | `cls-nothing2`, `cls-com`, `cls-maxc1`, `cls-maxqc1`, `cls-maxqc2` | `(x, rdf:type, c)` or `(x, p, y)` |
| **Cax** (2) | `cax-dw`, `cax-adc` | `(x, rdf:type, c)` — the smaller-IRI side of the disjoint pair |
| **Eq** (3) | `eq-diff1`, `eq-diff2`, `eq-diff3` | `(x, owl:sameAs, y)` — the contradicted equality |
| **Dt** (1) | `dt-not-type` | `(s, p, "lex"^^<dt>)` — the malformed literal triple |

### Surface

```sql
SELECT rdf_owl_rl_consistent(
  NULL,                         -- asserted graph (NULL = default graph)
  'urn:g:catalogue:inferred',   -- inferred graph (required)
  json('{"max_violations": 10000}')
);
-- => TEXT (JSON array)
--
-- [] when consistent; otherwise:
-- [
--   {"rule":"cax-dw",  "s":"<urn:alice>", "p":"<…#type>", "o":"<urn:Animal>"},
--   {"rule":"prp-irp", "s":"<urn:bob>",   "p":"<urn:parentOf>", "o":"<urn:bob>"}
-- ]
```

### Per-derivation determinism

Symmetric rules (`cax-dw`, `prp-asyp`, `cls-com`, `eq-diff1`,
`prp-pdw`, `prp-adp`, `cax-adc`, `eq-diff2`, `eq-diff3`) emit
**one** record per semantic violation — the witness commits to
the lex-smaller participant by N-Triples form. Output is
globally sorted by `(rule, s, p, o)` so two back-to-back calls
on the same store produce byte-identical JSON.

### Witness format

`s` / `p` / `o` use N-Triples-style serialisation:
- IRI: `<http://example.org/alice>`
- Blank node: `_:b0`
- Literal: `"thirty"^^<http://www.w3.org/2001/XMLSchema#integer>`
- Quoted triple (RDF-star, since 0.7.0): `<< <s> <p> <o> >>`

This is the same format `rdf_term_value` / `rdf_triple_subject`
consume — round-trippable without an extra parse pass.

### Options (all optional)

| Option | Default | Purpose |
|---|---|---|
| `max_violations` | `10000` | Safety cap; exceeding aborts with a fixed-prefix error (no silent truncate — matches `rdf_shacl_core_validate`'s posture) |

### Error envelopes (fixed-prefix for consumer pattern-matching)

- `rdf_owl_rl_consistent: inferred_iri must be a named graph …`
- `rdf_owl_rl_consistent: options_json: <serde error>`
- `rdf_owl_rl_consistent: violation count exceeded max_violations (N)`
- `rdf_owl_rl_consistent: rule <id> error: <message>`

### Read-only

Inconsistency detection never inserts into the store and never
touches the dependency index (0.12.0). Pin: a dedicated
`test_rdf_owl_rl_consistent_read_only` integration test
asserts `rdf_count_all()` before == after.

### `dt-not-type` validation scope

0.13.0 validates the XSD **integer family** (`integer`, `int`,
`long`, `short`, `byte`, `nonNegativeInteger`,
`positiveInteger`, `nonPositiveInteger`, `negativeInteger`,
`unsigned{Long,Int,Short,Byte}`) and **booleans**. Decimal,
double / float, dateTime, anyURI, and the string family skip
validation — no false positives. Custom datatype IRIs are
opaque to OWL 2 RL and skip too. Documented for future
expansion.

### Implementation

- `src/functions/rdf_owl_rl/inconsistency.rs` — 17 `detect_*`
  functions + `INCONSISTENCY_RULES` dispatch table + 5 in-module
  smoke tests.
- `src/functions/rdf_owl_rl_consistent.rs` — the SQL scalar
  shell, options struct, max-violations guard.
- Helper visibility in `rules.rs` bumped to `pub(crate)` for
  reuse (`pairs_for_predicate`, `instances_of`, `all_quads`,
  `term_to_subj` / `term_to_named` / `subj_to_term`,
  `graphs_to_query`, `graph_to_ref`, `literal_int_value_eq`,
  `collect_cardinality_restrictions`,
  `type_pairs_index_by_{class,subject}` plus the
  well-known IRI constants).
- 21 new integration tests under `// ── 0.13.0 rdf_owl_rl_consistent ──`.

### Out of scope (revisit on consumer signal)

- `report_iri` option that writes violations into a graph
  instead of returning JSON. Would parallel
  `rdf_shacl_core_validate`'s report-graph mode.
- Cross-rule deduplication. Two rules can witness the same
  underlying contradiction (`cls-com` + `cax-dw` when complement
  is also disjoint). Each emits independently; consumers dedupe
  if they care.
- Repair suggestions (`sh:resultMessage` analogue).
- Extended datatype validation (decimal, float, dateTime, etc.).

## 0.12.0 — Native dependency index for DRed

`rdf_dred_overdelete(inferred_iri, retracted_premises_json) →
INTEGER` lands as a new top-level scalar, paired with a new
`track_dependencies` option on `rdf_owl_rl_materialise`. Together
they turn the consumer-side "delete-and-rederive" loop from an
O(retracted × inferred-with-overlap) SPARQL pattern match against
a dense `:derivedFrom` annotation graph into an O(log N)-per-
premise reverse-index lookup against a native side-table.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested
extensions" item **#8 — Native dependency index for DRed**.
`Vv::Graph::ChangeSet` / `Vv::Graph::Reasoner.dred!` (PLAN_0.11.0
Phase A on the gem side) can now route its over-deletion phase
through this surface instead of issuing per-premise SPARQL
round-trips.

### Surface

```sql
-- Step 1: populate the dependency index during materialise.
SELECT rdf_owl_rl_materialise(
  NULL,                         -- asserted graph (default)
  'urn:g:catalogue:inferred',   -- inferred graph
  json('{"track_dependencies": true}')
);

-- Step 2 (consumer): retract one or more asserted-graph premises
-- the usual way (rdf_delete / sparql_update / rdf_triples DML).

-- Step 3: over-delete the inferred dependents in one FFI crossing.
SELECT rdf_dred_overdelete(
  'urn:g:catalogue:inferred',
  json('[
    ["http://example.org/B",
     "http://www.w3.org/2000/01/rdf-schema#subClassOf",
     "http://example.org/C"]
  ]')
);
-- => INTEGER (count of over-deleted inferred quads)

-- Step 4 (consumer): re-materialise to pick up anything still
-- derivable from the remaining asserted facts. The index is
-- carried forward; subsequent overdeletes reuse it.
SELECT rdf_owl_rl_materialise(NULL, 'urn:g:catalogue:inferred',
  json('{"track_dependencies": true}'));
```

### Rule coverage (5 of 60 in 0.12.0)

The index records derivations from the five W3C OWL 2 RL "core
derivation" rules whose forward shape lends itself cleanly to
premise tracking — `scm-sco`, `scm-spo`, `eq-trans` (the three
`transitive_closure` shapes), `cax-sco`, and `prp-spo1`. The
remaining 55 rules from the 0.10.0 set still fire under
`track_dependencies: true`; their derivations just don't write
through to the dependency index, and `rdf_dred_overdelete` will
miss those quads when their premises are retracted. Expansion
is mechanical (each rule mirrors its premise-collecting helper
to retain source `Quad`s); the remaining 55 wait on a consumer
pull. Forward-leaning ship per the same posture as 0.9.0 / 0.10.0
/ 0.11.0.

### Per-derivation tracking, not per-quad union

The original PLAN_0.12.0 sketch proposed a single union set per
inferred quad (`HashMap<Quad, HashSet<Quad>>`). 0.12.0 ships the
stricter per-derivation list (`HashMap<Quad, Vec<HashSet<Quad>>>`)
because the multi-derivation cascade rule — "remove only when
*every* derivation has been broken" — cannot be decided from the
union without re-proving each candidate against the current
store. The per-derivation list lets the cascade decide locally
in O(d) where d is the number of derivations attached to the
candidate. The new test `test_rdf_dred_overdelete_multi_derivation`
pins this: an inferred quad with two independent derivations
survives a partial retract.

### Cascade semantics

The cascade is transitive: an over-deleted inferred quad is
treated as a removed premise for downstream derivations. The
worklist iterates until no new quad is added — bounded by the
depth of the index. Retracted premises are seeded into the
`removed` set at the start; the function does **not** remove
the premises themselves (the consumer handles `rdf_delete` etc).
After the cascade, the over-deleted quads are removed from the
store via `Store::remove` and `DependencyIndex::forget` walks
back through their derivations to drop the reverse entries.

### Error envelopes (fixed-prefix for consumer pattern-matching)

- `"rdf_dred_overdelete: inferred_iri must be a named graph …"`
- `"rdf_dred_overdelete: inferred_iri is required (NULL not allowed)"`
- `"rdf_dred_overdelete: retracted_premises_json parse error: …"`
- `"rdf_dred_overdelete: retracted_premises_json must be a JSON array …"`
- `"rdf_dred_overdelete: row N must have 3 or 4 elements …"`
- `"rdf_dred_overdelete: no dependency index — re-run
  rdf_owl_rl_materialise with track_dependencies: true"` — surfaced
  only when the index is *entirely* empty and the consumer passed
  non-empty premises (i.e. distinct from "this premise has no
  dependents," which silently returns 0).

### Default opt-out

`track_dependencies` defaults to `false`. The tracking write-
through roughly doubles per-derivation allocation cost
(every tracked derivation carries a `Vec<Quad>` of premises),
and most workloads don't run a DRed cycle. Turn it on only when
the consumer's incremental-reasoning loop will follow up with
`rdf_dred_overdelete`. Switching mid-stream is fine: the next
materialise call adds new derivations to the index; the index
persists across calls within a process.

### Index lifetime

In-memory, process-scoped, in lockstep with the in-memory store.
`rdf_clear()` clears the index too (added wiring on `clear_store`
— see `src/store.rs`). Persistence across process restarts ties
to the deferred RocksDB backend; until then, every cold start
needs a fresh `rdf_owl_rl_materialise(... track_dependencies:
true)` to repopulate the index before `rdf_dred_overdelete` can
do anything useful.

### Implementation

- `src/dependency_index.rs` — `DependencyIndex` (forward + reverse
  maps), `cascade`, `forget`, `clear`, process-wide singleton via
  `OnceLock<Mutex<_>>`. 5 unit tests.
- `src/functions/rdf_owl_rl.rs` — adds the `track_dependencies`
  option; the fixpoint loop now also drains an index-record
  worklist after each iteration when tracking is on.
- `src/functions/rdf_owl_rl/rules.rs` — adds tracked variants
  for the five core derivation rules alongside their existing
  un-tracked siblings. Other 55 rules carry `apply_tracked: None`.
- `src/functions/rdf_dred.rs` — the new SQL scalar.
- 9 new integration tests under `// ── 0.12.0 rdf_dred_overdelete ──`.

### Out of scope (revisit on consumer signal)

- Tracking the remaining 55 OWL 2 RL rules from 0.10.0.
- `rdf_construct_many` → index write-through (would let SHACL
  Rules materialisations participate in DRed).
- Cross-process / cross-restart index persistence (ties to
  the deferred RocksDB plan).
- A combined `rdf_dred_step(inferred, retracted_premises) →
  INTEGER` that fuses overdelete + re-materialise — the
  primitives stay composable until a consumer asks for ergonomics.

## 0.11.0 — Native SHACL Core validator pass

`rdf_shacl_core_validate(data_iri, shapes_iri, report_iri,
options_json) → INTEGER` lands as a new top-level scalar.
Collapses what was previously N constraints × M focus-nodes worth
of `sparql_ask` / `sparql_query` round-trips in the consumer
(`Vv::Graph::Shacl.validate!`) into one FFI crossing: the engine
walks the data graph once per shape, evaluates the per-property
constraints natively, and writes a W3C-conformant
`sh:ValidationReport` into a named report graph.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested
extensions" item **#7 — Native SHACL Core validator pass**. VG's
`Vv::Graph::Shacl.validate!` (PLAN_0.10.0 Phase B shipped on the
gem side, commit `ed55ef4`) can now route through this surface
instead of issuing per-constraint SPARQL round-trips.

### Constraint coverage (12 components, parity with VG's `ConstraintLibrary`)

| Group | Constraints |
|---|---|
| **Cardinality** | `sh:minCount`, `sh:maxCount` |
| **Value type** | `sh:datatype`, `sh:nodeKind`, `sh:class` (with `rdfs:subClassOf*` walk) |
| **String** | `sh:pattern` (+ `sh:flags` — `i`/`s`/`m`/`x`), `sh:minLength`, `sh:maxLength` |
| **Value membership** | `sh:in`, `sh:hasValue` |
| **Range** | `sh:minInclusive`, `sh:maxInclusive` |

The remaining ~18 SHACL Core constraint components in VG's
`PHASE_B_PENDING` defer to a future release — same lockstep
posture as PLAN_0.10.0's rule-set / VG coverage relationship.

### Path evaluator (7 forms)

- **Predicate** — bare IRI in `sh:path`.
- **Inverse** — `[ sh:inversePath :p ]`.
- **Sequence** — `( :p1 :p2 … )` (RDF list of paths).
- **Alternative** — `[ sh:alternativePath ( :p1 :p2 … ) ]`.
- **Zero-or-more** — `[ sh:zeroOrMorePath :p ]` (reflexive
  transitive closure).
- **One-or-more** — `[ sh:oneOrMorePath :p ]` (transitive closure
  without reflexive seed).
- **Zero-or-one** — `[ sh:zeroOrOnePath :p ]`.

### Target resolution

`sh:targetClass` (walks `rdf:type` in the data graph),
`sh:targetNode` (focus IRI verbatim), `sh:targetSubjectsOf`
(every subject of a triple with the given predicate),
`sh:targetObjectsOf` (every object). Shapes are also picked up
via explicit `rdf:type sh:NodeShape` even when no target is
declared.

### Report contract

The `report_iri` named graph is **cleared** before each call —
re-validating overwrites rather than accumulates. The emitted
graph contains a single `sh:ValidationReport` header node with
`sh:conforms <true|false>^^xsd:boolean` and one `sh:result`
edge per violation. Each `sh:ValidationResult` carries
`sh:focusNode`, `sh:resultPath`, `sh:value` (when applicable),
`sh:sourceShape`, `sh:sourceConstraintComponent`,
`sh:resultSeverity` (always `sh:Violation` in 0.11.0), and
`sh:resultMessage`.

Blank-node shapes get a synthesised stable IRI in `sh:sourceShape`
(`shape_iri_prefix + <bnode-id>`) so consumers can pattern-match
without grokking blank-node identity.

### Options

| Option | Default | Purpose |
|---|---|---|
| `max_violations` | `10000` | Hard upper bound; call aborts with fixed-prefix error once exceeded |
| `provenance` | `false` | Adds `:reportedBy` and `:reportedAt` triples on each `sh:ValidationResult` |
| `reported_by_iri` | `urn:semantica:shacl:reportedBy` | Operator-overridable provenance predicate |
| `reported_at_iri` | `http://www.w3.org/ns/prov#generatedAtTime` | Operator-overridable timestamp predicate |
| `shape_iri_prefix` | `urn:semantica:shape:` | Blank-node-shape IRI prefix |

### Error envelopes (fixed-prefix for consumer pattern-matching)

- `rdf_shacl_core_validate: shapes_iri must be a named graph (NULL is not allowed)`
- `rdf_shacl_core_validate: report_iri must be a named graph (NULL is not allowed for the report slot)`
- `rdf_shacl_core_validate: violation count exceeded max_violations (N)`
- `rdf_shacl_core_validate: sh:path must be an IRI or blank-node structure, …`
- `rdf_shacl_core_validate: property shape <…> has no sh:path`
- `rdf_shacl_core_validate: sh:path list has a cycle`
- `rdf_shacl_core_validate: options_json: <serde error>`

### Out of scope

- **SHACL-SPARQL constraints** (`sh:sparql`) — arbitrary embedded
  SPARQL; falls back to consumer-side `sparql_query` round-trips
  anyway.
- **SHACL Rules** (`sh:rule`) — `Vv::Graph::Shacl::Rules.materialise!`
  already routes through 0.8.0's `rdf_construct_many`.
- **SHACL Advanced** (`sh:function`, `sh:expression`) — out of the
  SHACL Core profile.
- **The remaining ~18 SHACL Core constraint components** — same
  lockstep-with-VG posture as PLAN_0.10.0's rule-set.

12 integration tests added (`test_rdf_shacl_core_validate_*`) +
14 constraint unit tests + 11 path evaluator unit tests. Test
count climbs by 37; full suite is green in both debug and
release.

## 0.10.0 — Full OWL 2 RL derivation coverage (~45 additional rules)

`rdf_owl_rl_materialise`'s rule library grows from 15 to 60 rules,
covering the W3C OWL 2 RL/RDF derivation rules across all five
tables (Scm, Cls, Cax, Prp, Eq, Dt). Function signature, return
shape, atomicity contract, and error envelopes from 0.9.0 are
unchanged — this release is purely additive at the rule level.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested extensions"
item #6, second bullet ("The remaining ~55 rules … land in engine
0.10.0; Vv::Graph callers using ontologies that depend on
out-of-subset constructs stay on the per-rule `Sparql.execute`
path until then"). With this release, Vv::Graph's
`Vv::Graph::Reasoner::Rules::PHASE_B_PENDING` has a native engine
path; the gem can graduate its rule library on its own cadence.

New rules grouped by W3C table (45 total):

- **Scm — T-Box (16):** scm-cls, scm-op, scm-dp, scm-eqc2,
  scm-eqp2, scm-dom1, scm-dom2, scm-rng1, scm-rng2, scm-hv,
  scm-svf1, scm-svf2, scm-avf1, scm-avf2, scm-int, scm-uni.
- **Cls — class-expression A-Box (14):** cls-thing, cls-nothing1,
  cls-int1, cls-int2, cls-uni, cls-svf1, cls-svf2, cls-avf,
  cls-hv1, cls-hv2, cls-maxc2, cls-maxqc3, cls-maxqc4, cls-oo.
- **Cax — class-axiom A-Box (2):** cax-eqc1, cax-eqc2.
- **Prp — property reasoning (5):** prp-ifp, prp-spo2
  (property-chain composition via `owl:propertyChainAxiom`),
  prp-eqp1, prp-eqp2, prp-key (`owl:hasKey` → sameAs with
  cartesian-product key matching for multi-valued keys).
- **Eq — equality (4):** eq-ref, eq-rep-s, eq-rep-p, eq-rep-o.
- **Dt — datatype (4):** dt-type1 (closed 31-IRI W3C list of
  XSD + RDF datatypes get `rdf:type rdfs:Datatype`), dt-type2
  (same for consumer-defined datatypes that appear as literal
  datatypes in the store), dt-eq, dt-diff.

Two new options on `MaterialiseOptions`:

- `equality_saturation: bool` — gates `eq-rep-s` / `eq-rep-p` /
  `eq-rep-o`. **Default `true`** (W3C semantics). Set to `false`
  to short-circuit term-substitution when a graph with heavy
  `owl:sameAs` linkage would otherwise produce an O(N · K)
  closure. `eq-sym` and `eq-trans` continue to fire regardless.
- `eq_reflexive: bool` — gates `eq-ref`. **Default `false`** —
  this is a deviation from the original PLAN_0.10.0 design, which
  shipped `eq-ref` on by default. Phase D test runs showed
  `eq-ref` with `provenance: true` does not converge: every
  reflexive `?s owl:sameAs ?s` it derives gets two annotation
  triples whose subjects are quoted-triple terms new to the
  inferred graph, which `eq-ref` then derives reflexives for, and
  so on — the closure runs out the 50-iteration cap. The opt-in
  default keeps the engine bounded; consumers round-tripping
  against a W3C-strict reasoner that expects reflexive saturation
  enable it explicitly.

Known limitations:

- **`dt-eq` / `dt-diff` are no-ops in Oxigraph 0.4.** The W3C rule
  emits `?lit1 owl:sameAs ?lit2` / `?lit1 owl:differentFrom ?lit2`
  where both sides are literals. Oxigraph 0.4's `Subject` enum
  has no `Literal` variant, so a literal-subject triple is
  type-blocked at construction time. Both rule functions are
  wired into the dispatch but return empty derivations. Revive
  when Oxigraph upgrades the model (≥ 0.5).
- **Inconsistency rules deferred.** The ~15 W3C "false"-deriving
  rules (`prp-irp`, `prp-asyp`, `prp-pdw`, `prp-adp`, `prp-npa1/2`,
  `cls-com`, `cls-nothing2`, `cls-maxc1`, `cls-maxqc1/2`,
  `cax-dw`, `cax-adc`, `eq-diff1/2/3`, `dt-not-type`) are not in
  this release. They detect contradictions rather than derive
  triples, which doesn't fit `rdf_owl_rl_materialise`'s monotonic
  fixpoint contract. A separate `rdf_owl_rl_consistent` surface
  is queued for a future release returning a JSON array of
  violation records.

Implementation notes:

- New helper module `src/functions/rdf_owl_rl/rdf_lists.rs` —
  walks `rdf:first` / `rdf:rest` chains for the list-using rules
  (cls-int1/2, cls-uni, cls-oo, scm-int, scm-uni, prp-spo2,
  prp-key). Cycle-safe; rejects ambiguous / malformed lists as
  `None` so rule bodies fail gracefully.
- The 0.9.0 fixpoint loop, provenance emission, options blob,
  return-shape, error envelopes, and dedup-against-inferred check
  are all reused unchanged. Only `rules.rs` grew (and the
  dispatch table within it).
- `Cargo.toml` and `VERSION` bump to `0.10.0`. No new
  external dependencies.

Tests: 30 → 61 lib + 70 → 77 integration + 1 ignored, both debug
and release. Per-rule lib smoke tests cover each derivation rule
in isolation; 5 full-stack integration tests
(`*_intersection_round_trip`, `*_property_chain_uncle`,
`*_has_key_resolves_duplicates`,
`*_inverse_functional_property_collapses`, `*_dt_type1_emits_xsd_axioms`)
exercise the rule families through the SQL surface;
`*_equality_saturation_disabled` and
`*_equality_saturation_default_substitutes` pin the new option's
contract.

See `docs/plans/PLAN_0.10.0.md` for the full design (scope split
rationale, equality-saturation discussion, the realised `eq-ref`
non-convergence under provenance, the deferred-inconsistency
follow-on plan).

## 0.9.0 — Native OWL 2 RL rule pass (15-rule subset)

`rdf_owl_rl_materialise(asserted_iri TEXT, inferred_iri TEXT,
options_json TEXT) → INTEGER` runs a native Rust fixpoint loop over
Oxigraph's store, applying 15 W3C OWL 2 RL/RDF rules in one FFI
crossing in place of `vv-graph`'s per-rule `Sparql.execute`
round-trip. Skips the SPARQL parser per rule; ships parity with
`vv-graph`'s `Vv::Graph::Reasoner::Rules::OwlRl` so the engine +
gem produce identical closures (pinned by
`test_rdf_owl_rl_materialise_equivalence_with_vg`).

Driver: `CONSUMER_REQUIREMENT_VG.md` § "Requested extensions"
item #6. VG's `Vv::Graph::Reasoner.materialise!` (gem-side Phase B
already shipped) issues one `sparql_update` per rule per fixpoint
iteration. The native pass collapses N rules × M iterations of
SQL parse + SPARQL parse + evaluate to a single FFI crossing while
preserving the gem's `:derivedBy <rule_iri> ; :derivedAt …`
RDF-star provenance shape.

Surface:

- `rdf_owl_rl_materialise(asserted_iri, inferred_iri, options_json) → INTEGER`
  - `asserted_iri = NULL` → default graph; otherwise a named graph.
  - `inferred_iri = NULL` is **rejected** — derived triples mixing
    into the default graph would erase the asserted-vs-derived
    distinction OWL reasoning depends on.
  - `options_json` JSON object; all fields optional. Defaults:
    `{"max_iterations": 50, "provenance": false,
     "derived_by_iri": "http://www.w3.org/ns/prov#wasDerivedFrom",
     "derived_at_iri": "http://www.w3.org/ns/prov#generatedAtTime",
     "rule_iri_prefix": "urn:semantica:rule:"}`.
  - Return: signed net delta in store size — matches
    `sparql_update`'s convention.

Rule coverage (the 15 rules — W3C names verbatim):

| Bucket | Rules |
|---|---|
| T-Box transitive closure | `scm-sco`, `scm-spo`, `scm-eqc1`, `scm-eqp1` |
| A-Box propagation        | `cax-sco`, `prp-spo1` |
| Domain / range           | `prp-dom`, `prp-rng` |
| Property characteristics | `prp-trp`, `prp-symp`, `prp-inv1`, `prp-inv2`, `prp-fp` |
| sameAs closure           | `eq-sym`, `eq-trans` |

The remaining ~55 W3C OWL 2 RL rules are deferred to 0.10.0.
Operators using ontologies that depend on out-of-subset constructs
(`owl:intersectionOf`, `owl:unionOf`, `owl:hasKey`, etc.) should
stay on the per-rule `sparql_update` path until 0.10.0 ships.

With `"provenance": true`, every derived triple is annotated with
two RDF-star quads in the inferred graph (since 0.7.0):

```
<< <s> <p> <o> >> prov:wasDerivedFrom <urn:semantica:rule:scm-sco> .
<< <s> <p> <o> >> prov:generatedAtTime "2026-05-25T20:02:43Z"^^xsd:dateTime .
```

The predicate IRIs and rule-IRI prefix are operator-overridable;
defaults match `vv-graph`'s `Vv::Graph::Reasoner` convention.

Decisions worth flagging for consumers:

- **Provenance shape commits to defaults that match VG.** Deviation
  from PLAN_0.7.0/0.8.0's "engine stays domain-agnostic" posture —
  materialisation has nowhere to put provenance except on the
  triple it just derived (no consumer round-trip the way
  `rdf_construct_many` has). The override mechanism softens the
  coupling for callers using a different provenance vocabulary.
  See `docs/plans/PLAN_0.9.0.md` for the rationale.
- **Atomicity is partial-on-iteration.** If the fixpoint isn't
  reached within `max_iterations`, the partial closure stays in
  the inferred graph rather than rolling back. Matches
  `sparql_update`'s partial-on-evaluation contract from 0.5.0.
- **No `chrono` dependency.** A hand-rolled RFC3339 formatter
  (Hinnant's civil-from-days algorithm, ~20 lines) handles the
  one timestamp call site. Avoids ~150 KB of dylib growth from
  a single-use dep.

Error envelopes (fixed prefix for consumer pattern-matching):

- `rdf_owl_rl_materialise: inferred_iri must be a named graph
  (NULL is not allowed for the inferred slot)`
- `rdf_owl_rl_materialise: fixpoint not reached after N iterations`
- `rdf_owl_rl_materialise: rule <id> error at iteration N: …`
- `rdf_owl_rl_materialise: options_json: …`

Tests: 62 → 70 + 1 ignored. 8 new under
`// ── 0.9.0 rdf_owl_rl_materialise ──` in
`tests/integration_test.rs`. The equivalence test pins the engine's
closure against a hand-written expected fixture (the closure VG
would produce for the same input).

## 0.8.0 — Batched CONSTRUCT

`rdf_construct_many(queries_json TEXT) → TEXT` evaluates N CONSTRUCT
queries in one FFI crossing. The return is a JSON array of N
N-Triples blobs — one per input query — preserving per-query
attribution so consumers can annotate per-rule downstream before
inserting. Matches the `_many` convention from 0.4.0 (`rdf_insert_many`)
and 0.6.0 (`rdf_load_*_to_graph`).

Driver: `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item
#9 (Batched SHACL Rules execution), added in the post-v0.7.0 doc
update. RS's `Shacl::Rules.materialise!` issues one `sparql_update`
per rule per fixpoint iteration; ~50 rules per iteration paying the
SQL + FFI overhead 50× collapses to 1× with this scalar. (The
per-rule SPARQL parse cost still happens N× — Oxigraph parses each
query at evaluation time. The savings are on the SQL/FFI side, not
the SPARQL parser. A prepared-query model would be a separate, much
larger plan.)

Surface:

- `rdf_construct_many(queries_json TEXT) → TEXT` — `queries_json`
  is a JSON array of CONSTRUCT query strings. Returns a JSON array
  of the same length where the `i`-th element is the N-Triples
  output of the `i`-th query (an empty string when the query
  binds zero triples).

Decisions worth flagging for consumers:

- **JSON array of N-Triples blobs**, not a flat blob or an integer
  count. Flat would lose per-query attribution; integer would imply
  the engine inserts results (it doesn't — CONSTRUCT is read-only,
  and provenance shape is RS's call, not ours). See `docs/plans/PLAN_0.8.0.md`
  for the full rationale.
- **Provenance stays out of the engine.** Same posture as PLAN_0.7.0:
  the engine emits data, the consumer (RS) attaches `:derivedBy`
  / `:derivedAt` annotations downstream. The name
  `rdf_construct_many_with_provenance` is deliberately left
  unoccupied for a future engine-side annotation variant if RS asks.
- **All-or-nothing pre-flight parse.** Every query is parsed up
  front; if any fails the batch errors with the prefix
  `SPARQL parse error (query index N):` before any query evaluates.
  Matches `rdf_insert_many`'s atomicity contract.
- **Non-CONSTRUCT input is rejected** with the prefix
  `rdf_construct_many: query index N is not a CONSTRUCT`.
- **Non-array JSON input is rejected** with the prefix
  `rdf_construct_many: expected JSON array of query strings`.

No surface change to `sparql_construct` (1-arg). 1-element batches
are byte-identical to the 1-arg path — pinned by
`test_rdf_construct_many_parser_parity_with_single`.

RDF-star CONSTRUCT outputs (quoted-triple subjects from 0.7.0) flow
through unchanged — pinned by `test_rdf_construct_many_with_rdf_star`.

## 0.7.0 — RDF-star / SPARQL-star round-trip

Quoted-triple terms now survive the SQL boundary in both directions.
Before 0.7.0, the term serialiser in `src/functions/sparql_query.rs`
stubbed every `Term::Triple` / `Subject::Triple` to the literal string
`"<<rdf-star unsupported>>"`, and the term parser in `src/store.rs`
rejected any `<<…>>` input as a malformed IRI. The engine side
(Oxigraph 0.4) already parsed Turtle-star / N-Triples-star and
evaluated SPARQL-star — only the SQL boundary lost information.

Surface delta:

- **Write paths** — `rdf_insert(s, p, o[, g])`, `rdf_delete(s, p, o[, g])`,
  `rdf_insert_many`, `rdf_delete_many`, and the `rdf_triples` vtab all
  accept `<< <s> <p> <o> >>` in subject and object position. Predicate
  position stays IRI-only (RDF doesn't extend star to predicates).
- **Read paths** — `rdf_dump_ntriples`, `sparql_construct`, the JSON
  bindings from `sparql_query`, and `SELECT` over the `rdf_triples`
  vtab all emit `<< s p o >>` for quoted-triple terms. Nesting
  (`<< << s p o >> p o >>`) round-trips.
- **SPARQL-star** flows straight through to Oxigraph — annotation
  shorthand `{| |}`, explicit `<<>>` patterns, and the
  `TRIPLE` / `SUBJECT` / `PREDICATE` / `OBJECT` / `isTRIPLE` built-ins
  all work without SQL-side wrapping.
- **New scalars** (additive — every 0.6.x call works unchanged):
  - `rdf_triple_subject(term) → TEXT` — extract subject of a quoted triple.
  - `rdf_triple_predicate(term) → TEXT` — extract predicate.
  - `rdf_triple_object(term) → TEXT` — extract object.

Behaviour changes (call out for consumers):

- `rdf_term_type(term)` now returns `"triple"` for a `<<…>>` string
  (previously `"unknown"`).
- `rdf_term_value(term)` on a `<<…>>` string now raises a SQLite
  error with the fixed-prefix message
  `rdf_term_value: triple terms have no scalar value; use
  rdf_triple_subject / rdf_triple_predicate / rdf_triple_object: …`
  Previously raised `unrecognised term format: …`. Prefix-matching
  consumers (none known) must update.

Driver: the MM Conformer subagent in vv-memory's Silver tier — see
`docs/research/StarExts.md` §6. Neither `CONSUMER_REQUIREMENT_MM.md`
nor `CONSUMER_REQUIREMENT_RS.md` calls the new surface yet; both
list it in their "Available upstream but not exercised" sections so
the paper trail is in place when consumers adopt.

RocksDB persistence (penciled in for 0.7.0 by earlier roadmaps) is
deferred indefinitely — no consumer pressure. Revive on first ask.

## 0.6.0 — Graph-scoped bulk loading

Closes the last named-graph gap on the SQL surface. Until 0.6.0 the
three bulk loaders forced every parsed quad into the default graph,
which meant a consumer issuing `INSERT DATA { GRAPH <iri> { … } }`
through `rdf_load_ntriples` saw the `GRAPH` wrapper silently
discarded. Three new scalars route the parsed quads into a named graph
in one FFI call:

- `rdf_load_ntriples_to_graph(body TEXT, graph TEXT) → INTEGER`
- `rdf_load_turtle_to_graph(body TEXT, graph TEXT) → INTEGER`
- `rdf_load_rdfxml_to_graph(body TEXT, graph TEXT) → INTEGER`

`graph = NULL` means the default graph (identical to the 1-arg
loaders); `graph = '<iri>'`-style strings are rejected — pass the bare
IRI as the second argument, matching the 4-arg `rdf_insert(s, p, o,
graph)` convention from 0.3.0. Blank-node graph IRIs (`_:label`) are
rejected with the same `blank-node graphs are not supported` error
the 0.3.0 path raises, so consumer-side prefix-matching keeps working
unchanged.

The 1-arg loaders are byte-for-byte unchanged. The 2-arg form with
`NULL` produces the same store state as the 1-arg form — pinned by
`test_rdf_load_ntriples_to_graph_parser_parity`.

Driver: `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item #1.
With this in place, items #1–#4 of that file graduate from "Requested"
to "SQL surfaces RS consumes."

## 0.5.0 — SPARQL UPDATE

Exposes Oxigraph's `Store::update` as a new scalar:

- `sparql_update(query) → INTEGER` — runs any SPARQL 1.1 UPDATE form
  (`INSERT DATA`, `DELETE DATA`, `INSERT { … } WHERE { … }`,
  `DELETE { … } WHERE { … }`, mixed modifies, `CLEAR`, `CREATE`,
  `DROP`, `LOAD`).

### Return value — important

Oxigraph 0.4's `Store::update` doesn't expose a first-class
affected-row count. `sparql_update` returns the **signed net change**
in store size, computed via `len()` before and after the call:

| UPDATE shape                            | Return value             |
|-----------------------------------------|--------------------------|
| `INSERT DATA { … }`                     | `+N` (newly inserted, post-dedup) |
| `DELETE DATA { … }`                     | `-N` (removed)           |
| `INSERT { … } WHERE { … }`              | `+N`                     |
| `DELETE { … } WHERE { … }`              | `-N`                     |
| mixed `DELETE/INSERT { … } WHERE { … }` | `inserts - deletes` (may be `0`) |
| `CLEAR DEFAULT` / `CLEAR ALL` / `CLEAR GRAPH <g>` | `-N`           |

A balanced mixed UPDATE returns `0` even though both halves ran.
When you need to assert *state*, use `rdf_count` / `sparql_ask` /
`sparql_query` instead of relying on the delta.

### Error classification

Errors are split into `ParseError` (Oxigraph's `EvaluationError::Parsing`
variant — bad SPARQL syntax) and `EvalError` (everything else —
graph-already-exists, unbound service, etc.). The resulting SQLite
error message is prefixed `SPARQL parse error: …` or
`SPARQL evaluation error: …` so downstream consumers can
pattern-match.

### Network safety

SPARQL 1.1 `LOAD <iri>` would make Oxigraph fetch the IRI over HTTP
from inside the database. The default Oxigraph build has no HTTP
support, so `LOAD` returns an evaluation error today. If you ever
build Oxigraph with HTTP enabled, sandbox the host process
accordingly — this is a deliberate non-mitigation in 0.5.0.

### Tests

Ten new integration tests (37 + 1 ignored, up from 27 + 1):
`test_sparql_update_insert_data`, `test_sparql_update_delete_data`,
`test_sparql_update_dedup_on_insert_data`,
`test_sparql_update_where_insert`,
`test_sparql_update_modify_mixed` (asserts store state, not delta,
for mixed ops), `test_sparql_update_named_graph`,
`test_sparql_update_clear_default`, `test_sparql_update_clear_all`,
`test_sparql_update_parse_error_surfaces`,
`test_sparql_update_evaluation_error_surfaces`.

## 0.4.0 — batched insert / delete

Adds `rdf_insert_many(json)` and `rdf_delete_many(json)` for writing
many triples in a single FFI crossing, collapsing the SQL-parse +
function-dispatch overhead of N separate `rdf_insert` calls down to
one.

### New SQL surface

- `rdf_insert_many(json) → INTEGER` — single JSON-array argument.
  Each row is `[s, p, o]` (default graph) or `[s, p, o, graph]`
  (named graph; `null` means default). Uses Oxigraph's `bulk_loader`
  internally. Returns the count of *newly* inserted quads; duplicates
  collapse under RDF set semantics and don't count.
- `rdf_delete_many(json) → INTEGER` — mirror. Per-row removal; no-ops
  (rows not present in the store) don't count toward the return value.

### Behaviour

- Empty array `'[]'` returns `0`, no error.
- Malformed input — non-array JSON, row of wrong arity, non-string
  element, invalid IRI, blank-node graph — aborts the *whole* batch
  before any write touches the store. Error messages include the
  failing row index (e.g. `row 7: subject: …`).
- Term encoding matches the single-row `rdf_insert(s, p, o)` parser
  exactly. Pinned by `test_insert_many_parser_parity_with_single`.

### Internal

- `store::{build_quad, parse_named_or_blank, parse_term,
  parse_graph_name}` are now `pub(crate)` so the bulk module reuses
  the single-row parser. This keeps the two write paths bit-identical
  in their handling of the term grammar (the risk the plan called
  out).

### Tests

Seven new tests (27 + 1 ignored perf-smoke, up from 20):
`test_insert_many_3_arg_rows`,
`test_insert_many_mixed_arities`,
`test_insert_many_dedup_return_value`,
`test_insert_many_malformed_aborts_batch`,
`test_insert_many_empty_array`,
`test_insert_many_parser_parity_with_single`,
`test_delete_many_partial`,
`test_insert_many_perf_smoke` (release-only, `#[ignore]` — run with
`cargo test --release -- --ignored insert_many_perf_smoke`; 1000-row
batch under 100 ms).

## 0.3.0 — named graphs

Adds named-graph support across the full SQL surface. All existing
zero- and three-argument signatures keep their 0.2.0 behaviour;
named-graph variants ride alongside.

### New SQL surface

- `rdf_insert(s, p, o, graph)` — 4-arg form routes into a named graph.
  `graph = NULL` is the default graph (same as the 3-arg form).
  Blank-node graphs (`_:…`) are rejected with a clear error.
- `rdf_delete(s, p, o, graph)` — mirror of insert.
- `rdf_count(graph)` — 1-arg form counts quads in a named graph;
  `NULL` is the default graph (same as `rdf_count()`).
- `rdf_count_all()` — counts across every graph, default included.
- `rdf_triples` virtual table now has a HIDDEN `graph` column:
  - `SELECT *` still returns three columns
  - `INSERT INTO triples VALUES (s, p, o)` still works (default graph)
  - `INSERT INTO triples(subject, predicate, object, graph) VALUES (…)`
    writes to a named graph
  - `WHERE graph = 'urn:g:…'` / `WHERE graph IS NULL` filter on graph

### SPARQL routing

SPARQL 1.1 `FROM <graph>`, `FROM NAMED <graph>`, and `GRAPH <graph> { … }`
clauses go straight through to Oxigraph — no extra plumbing needed.
The default dataset for an unqualified `?s ?p ?o` query remains the
default graph only; named-graph triples never leak in without an
explicit `FROM` or `GRAPH` clause (pinned by
`test_sparql_query_default_dataset_isolates`).

### Backward compatibility

Every 0.1.0 / 0.2.0 caller keeps working unchanged. The 3-arg forms,
zero-arg `rdf_count()`, and the 3-column `SELECT * FROM triples` /
`INSERT INTO triples VALUES (…)` shapes are unchanged in syntax and
semantics.

### Tests

Six new integration tests (20 total, up from 13):
`test_rdf_insert_4arg_named_graph`,
`test_rdf_delete_4arg_named_graph`,
`test_rdf_insert_4arg_rejects_blank_graph`,
`test_sparql_query_graph_clause`,
`test_sparql_query_default_dataset_isolates`,
`test_vtab_named_graph_round_trip`,
`test_vtab_default_graph_compat`.

## 0.2.0 — shared process-wide store

Replaces the per-thread Oxigraph store from 0.1.0 with a single
process-wide store wrapped in `OnceLock<Store>`. Every SQLite connection
on every thread now sees the same triple graph.

### Behaviour change

- A triple inserted on one SQLite connection is **visible** from every
  other connection in the same process, including connections on other
  threads. This is the headline fix for the "insert-on-thread-A-
  invisible-from-thread-B" footgun called out in
  `docs/reviews/REVIEW_0.1.0.md`.
- The SQL surface is unchanged. No function was renamed, added, or
  given a new signature in this release.
- `rdf_clear()` now empties the existing store in place (via
  `Store::clear`) rather than replacing it with a fresh instance. The
  observable behaviour is identical for callers (count → 0; subsequent
  inserts continue to work).
- Internal: `store::with_store_mut` was removed (it was always a
  misnomer — Oxigraph's `Store` mutates through `&self`). `with_store`
  takes its place at every call site. This is not a public API.

### Concurrency

Oxigraph 0.4's in-memory `Store` is internally concurrent — every
mutator takes `&self` and the storage layer uses `DashMap` plus
`RwLock` for synchronisation. The extension wraps the store in
`OnceLock` only for lazy initialisation; no additional `Mutex` or
`RwLock` is layered on top.

Downstream consumers like `rails-semantica` should be aware that
concurrent HTTP requests (Puma threads) can now interleave reads and
writes against the shared graph — which is the right correctness
story, but is a new concurrency surface compared to 0.1.0.

### Tests

- Dropped `test_thread_local_isolation` — it pinned the old, buggy
  invariant.
- Added `test_cross_thread_visibility` — proves the new invariant
  across threads.
- Added `test_shared_store_across_connections` — proves it across
  connections on the same thread.
- Added `serial_test` as a dev-dependency and marked every
  integration test `#[serial]`. The shared store means parallel tests
  would otherwise race; `cargo test` is now serialised at the
  integration-test layer only.

### Roadmap shift

PLAN_0.1.0 tentatively numbered "named graphs" as 0.2.0. That work
moves to **0.3.0** (`docs/plans/PLAN_0.3.0.md`); batched insert
(`rdf_insert_many`) was newly broken out as **0.4.0**
(`docs/plans/PLAN_0.4.0.md`). The MM consumer document
(`CONSUMER_REQUIREMENT_MM.md`) has been re-labelled to match.

## 0.1.0 — first green build

Initial release. SQLite loadable extension embedding the Oxigraph RDF/SPARQL
engine. The thread-local Oxigraph store is in-memory and resets when the
thread exits.

### SQL surface

- Scalar functions: `rdf_insert`, `rdf_delete`, `rdf_clear`, `rdf_count`,
  `rdf_load_turtle`, `rdf_load_ntriples`, `rdf_load_rdfxml`,
  `rdf_dump_ntriples`, `rdf_term_type`, `rdf_term_value`.
- SPARQL: `sparql_query` (SELECT → JSON), `sparql_ask` (ASK → 0/1),
  `sparql_construct` (CONSTRUCT → N-Triples).
- Virtual table: `rdf_triples` — read scans the default graph; INSERT
  writes through to the store. DELETE and UPDATE on the vtab are not
  supported in 0.1.x (use `rdf_delete(s,p,o)` or a SPARQL DELETE).

### Scope

- RDF 1.1 only — RDF-star quoted triples are rejected with a clear error.
- All triples live in the default graph; named graphs land in 0.2.0.
- In-memory store only; the persistent RocksDB backend lands in 0.4.0.

### Known limitations

- Thread-local store: Rails 8's SQLite connection pool reuses threads, so
  each pooled thread sees its own store. Acceptable for the in-memory
  build; revisit when the persistent backend lands.
