# PLAN 0.7.0 — RDF-star / SPARQL-star round-trip

> Close the four gaps that currently turn the extension's `Term::Triple`
> handling into a one-way street: teach the N-Triples term parser to
> read `<< s p o >>`, teach the term serialiser to write it back out,
> and teach `rdf_term_type` / `rdf_term_value` to classify and unpack
> it. Loading, storing, and SPARQL evaluation already work through
> Oxigraph 0.4 — only the SQL-surface boundary loses information today.

Driver: the **MM Conformer** subagent described in
`../research/StarExts.md` §6. Every Bronze→Silver extraction needs to
attach five fields (source episode, confidence, extracted-at,
extractor identity, supersedes-pointer) to a single triple. The two
honest representations are RDF-star annotation shorthand
(`{| :fromEpisode 42 ; :confidence 0.87 ; … |}`) and explicit
occurrence nodes (§3 of the primer); both demand that a quoted-triple
term survives a SQL round trip. Neither works today.

Neither `CONSUMER_REQUIREMENT_MM.md` nor `CONSUMER_REQUIREMENT_RS.md`
asks for this yet — they predate the Conformer plan. This release is
**forward-leaning**: it lands the substrate capability so MM's
PLAN_0.2.0 (vv-memory Silver Conformer) has a `sqlite-sparql` revision
it can pin against. The MM consumer doc grows a "Requested extensions"
section in Phase D listing what the Conformer plans to call; this plan
ships the surface to back it.

Depends on 0.2.0 (shared store — `Store` already stores `Term::Triple`
natively; no store-layer change needed) and 0.6.0 (parser-format
plumbing in `load_rdf`, reused unchanged: Oxigraph 0.4's `RdfFormat`
Turtle / N-Triples / TriG parsers accept RDF-star syntax by default,
no separate `TurtleStar` variant exists).

---

## Goal

`cargo test` passes new round-trip tests that:

1. Load a Turtle-star body containing `{| |}` annotations via
   `rdf_load_turtle`, count the resulting quads, and dump them back via
   `rdf_dump_ntriples` as syntactically-valid N-Triples-star
   (subject/object `<< … >>` forms, not the current
   `"<<rdf-star unsupported>>"` literal stub).
2. Insert a quoted triple as a subject through the 3-arg
   `rdf_insert('<<<s> <p> <o>>>', '<p2>', '<o2>')` path and read it back
   through the `rdf_triples` vtab.
3. Issue a SPARQL-star query
   (`SELECT ?val ?stater WHERE { :bob :name ?val {| :statedBy ?stater |} }`)
   and receive a JSON result whose triple-term bindings serialise as
   `<< … >>` rather than the stub string.
4. Round-trip the five SPARQL-star built-ins
   (`TRIPLE`, `SUBJECT`, `PREDICATE`, `OBJECT`, `isTRIPLE`) through
   `sparql_query`.

Item 1 unblocks the Conformer's "write annotation, dump for inspection"
loop. Items 2–4 unblock the MM Silver tier's read path.

---

## What works today vs. what doesn't

This split matters because it shapes the plan: the engine side is
already done by Oxigraph, so the work is concentrated at the
SQL-boundary code we own.

| Path | Status | Evidence |
|---|---|---|
| Turtle-star / N-Triples-star **parse** via `rdf_load_*` | ✅ works | `RdfParser::from_format(RdfFormat::Turtle)` in `src/functions/rdf_triple.rs:308` — Oxigraph 0.4's Turtle / N-Triples / TriG parsers accept `<<…>>` and `{\|…\|}` by default; no separate `…Star` format variant |
| Storing `Term::Triple` quads | ✅ works | `Store::insert(&quad)` accepts the quad shape regardless of triple-term subjects/objects |
| SPARQL-star **evaluation** via `sparql_query` / `sparql_ask` / `sparql_construct` / `sparql_update` | ✅ works | `Store::query` and `Store::update` parse and evaluate the full SPARQL-star grammar |
| Serialising `Term::Triple` results | ❌ **stub** | `src/functions/sparql_query.rs:209-211` — `Term::Triple(_) => "\"<<rdf-star unsupported>>\""` |
| Serialising `Subject::Triple` subjects | ❌ **stub** | `src/functions/sparql_query.rs:217-218` — same stub for subjects |
| `rdf_dump_ntriples` output | ❌ **lossy** | Calls the two functions above; every quoted triple in the store becomes the literal stub string |
| `sparql_construct` output | ❌ **lossy** | Same call sites |
| `sparql_query` JSON output | ❌ **lossy** | `term_to_json` → `term_to_ntriples` → stub |
| `rdf_insert(s, p, o)` accepting `<<…>>` as subject or object | ❌ **rejected** | `src/store.rs:159-169` `parse_named_or_blank` and `src/store.rs:177-191` `parse_term` fall through to `NamedNode::new` for anything that isn't `_:` / `"` — a `<<…>>` string is rejected as a malformed IRI |
| `rdf_triples` vtab `INSERT VALUES` accepting `<<…>>` | ❌ **rejected** | Routes through `insert_triple_in_graph` → `build_quad` → same parsers |
| `rdf_term_type('<<…>>')` returning `"triple"` | ❌ **wrong** | `src/functions/rdf_triple.rs:243-251` only knows `<` / `_:` / `"`; returns `"unknown"` |
| `rdf_term_value('<<…>>')` | ❌ **error** | `src/functions/rdf_triple.rs:286-291` raises `InvalidArgument` |

Five files touch the boundary; one of them
(`src/functions/sparql_query.rs:209`) carries an explicit pre-existing
TODO comment — `// RDF-star quoted triples are out of scope for 0.1.x.`
This plan retires that comment.

---

## Why an N-Triples-star encoding at the SQL boundary, not a JSON struct

The boundary is TEXT-typed in SQLite. Two encodings are plausible for a
quoted triple at that boundary:

- **N-Triples-star** (`<< <s> <p> <o> >>`) — the canonical RDF 1.1 star
  serialisation. Nestable. Round-trips through every star-aware parser.
  Already the syntax `rdf_load_ntriples` reads in the parse direction.
- **A SQLite JSON structure** (`{"s":"<…>","p":"<…>","o":"…"}` or a
  3-tuple array) — easy to consume from `json_each`, but invents a
  second wire format that disagrees with what `rdf_load_*` already
  accepts.

N-Triples-star wins for symmetry: every place the extension currently
takes an N-Triples term as TEXT (`rdf_insert`, the vtab, the dumper, the
JSON output of `sparql_query`) keeps using N-Triples syntax, with one
new lexical form added. No new wire format, no second encoding for
callers to maintain. The 0.6.0 convention — "the SQL surface speaks
N-Triples; the engine speaks whatever the parser accepts" — extends
cleanly into star.

The JSON output of `sparql_query` continues to embed terms as opaque
strings. Callers who need the inner-triple parts use the SPARQL-star
built-ins (`SUBJECT(?t)`, `PREDICATE(?t)`, `OBJECT(?t)`) inside the
query and project the parts as separate bindings — exactly the pattern
the spec recommends and the pattern the Conformer will use anyway.

---

## Why not "JSON of the triple" as `rdf_term_value`'s return?

`rdf_term_value` returns TEXT. For an IRI, blank node, or literal it
returns the unquoted lexical value. For a triple term, the "value" has
three parts, which can't honestly collapse to a single TEXT. Three
candidates:

1. Return the N-Triples-star encoding unchanged (i.e., `rdf_term_value`
   on a quoted triple returns the same string the caller passed in).
   Honest but useless — caller already has it.
2. Return a JSON object `{"s":"…","p":"…","o":"…"}`. Compact, but
   introduces a second encoding (see above) and asymmetrically — only
   for triple terms.
3. Raise an error and point callers at `SUBJECT(?t)`, `PREDICATE(?t)`,
   `OBJECT(?t)` (extractable inside SPARQL) or at three new scalars:
   `rdf_triple_subject(t)`, `rdf_triple_predicate(t)`,
   `rdf_triple_object(t)`. Honest, symmetric with how callers already
   pick a term apart from outside SPARQL today.

Pick **#3 + the three new scalars**. The boundary stays "one
N-Triples-encoded TEXT in, one term out" everywhere. `rdf_term_type`
on a quoted triple returns the new literal `"triple"` so callers can
branch before calling `rdf_term_value`.

---

## Provenance shape — annotation vs. occurrence — is a CONSUMER question

`StarExts.md` §3 and §6 frame the choice: does the Conformer attach
provenance directly to the triple (`{| :fromEpisode 42 ; :confidence 0.87 |}`)
or mint an explicit occurrence node and attach provenance to that?
Both shapes round-trip through the SQL surface this plan ships. The
choice belongs in MM's vv-memory PLAN, not here. Out of scope.

This plan's contract: whichever shape the Conformer picks, the bytes
survive `rdf_insert` → `Store::insert` → `Store::query` →
`sparql_query` / `rdf_dump_ntriples` without information loss.

---

## Phase A — pin the baseline (diagnostic, no code change)

Before changing anything, prove the four "works today" rows in the
table above actually hold against this build. Add **temporary**
diagnostic-only tests (delete them at end of Phase A) under
`tests/star_probe.rs`:

1. `probe_turtle_star_load` — `rdf_load_turtle` on a body with
   `{| |}` annotation. Expect a positive load count. Pin the count
   (annotation expands to N triples in the parser; the exact number is
   the pin).
2. `probe_sparql_star_query` — three sub-probes:
   (a) annotation-shorthand WHERE clause;
   (b) explicit `<<>>` pattern;
   (c) `TRIPLE(...)` built-in via `BIND`.
   Expect non-error returns; the JSON values for triple-term bindings
   will be the stub string `"<<rdf-star unsupported>>"` — pin that as
   the *current* observable so the Phase B diff is visible.
3. `probe_dump_roundtrip` — load star Turtle, dump via
   `rdf_dump_ntriples`, assert the output contains the stub literal.
   This is the negative pin.
4. `probe_rdf_insert_quoted_subject` — assert `rdf_insert` with a
   `<<...>>` subject string raises an error today. Negative pin.

### Exit criteria for Phase A

```
cargo test star_probe -- --nocapture
```

All four probes match their pinned (current) behaviour. The four rows
in the "works today" table are confirmed; the four "broken today" rows
are confirmed. The probe file is **deleted** before merging Phase B —
its tests would invert when Phase B lands, and the Phase C tests
replace them.

If a probe disagrees with the table above, stop and revise the table
before continuing. The plan's design rests on the parser-side rows
being accurate.

---

## Phase B — teach the term serialiser to emit N-Triples-star

Touch one file: `src/functions/sparql_query.rs`.

Replace the two stub arms:

```rust
// before — sparql_query.rs:194
pub fn term_to_ntriples(term: &Term) -> String {
    match term {
        Term::NamedNode(n) => format!("<{}>", n.as_str()),
        Term::BlankNode(b) => format!("_:{}", b.as_str()),
        Term::Literal(l) => { … },
        Term::Triple(_) => "\"<<rdf-star unsupported>>\"".to_string(),
    }
}
```

```rust
// after
pub fn term_to_ntriples(term: &Term) -> String {
    match term {
        Term::NamedNode(n) => format!("<{}>", n.as_str()),
        Term::BlankNode(b) => format!("_:{}", b.as_str()),
        Term::Literal(l) => { … },                         // unchanged
        Term::Triple(t) => format!(
            "<< {} <{}> {} >>",
            term_to_ntriples_subject(&t.subject),
            t.predicate.as_str(),
            term_to_ntriples(&t.object),
        ),
    }
}
```

Symmetric change in `term_to_ntriples_subject` for `Subject::Triple(t)`.
Both calls are recursive — nested star (`<< << s p o >> p o >>`)
round-trips for free because the recursion bottoms out at the
non-triple arms.

Whitespace: the canonical N-Triples-star form uses a single space
inside the brackets (`<< s p o >>`, not `<<s p o>>`). Match the
canonical form so external star-aware parsers (Jena, RDFLib) can
re-ingest the output without surprise.

### Affected call sites (no code change at the call sites)

- `rdf_dump_ntriples_fn` — output now includes `<<…>>` forms instead
  of the stub literal. Round-trips through `rdf_load_ntriples` after
  Phase C, but not before, because the loader path's term parser
  doesn't accept `<<` as a *direct* input via `rdf_insert` — it accepts
  it only as part of an N-Triples-star body fed to `rdf_load_ntriples`,
  which already works (Phase A pin row 1). The single-row path is
  Phase C.
- `sparql_construct_fn` — same.
- `sparql_query_fn` → `term_to_json` → `term_to_ntriples` — JSON
  bindings now contain the N-Triples-star form as a string. The
  outer-JSON shape (an array of objects with string-typed values)
  doesn't change; only the encoding inside the string improves.

### Exit criteria for Phase B

`cargo build` clean. `cargo test` still green (no test changes yet;
Phase A's probes are gone). A manual `cargo test star_probe` run will
fail because the probes are deleted — that's expected.

---

## Phase C — teach the term parser to read N-Triples-star

Touch `src/store.rs`. Extend `parse_term` and `parse_named_or_blank`
with a `<<` prefix arm that builds a `Triple` term recursively.

Two implementation paths; pick one in the plan, defer between them
until the first build attempt reveals which is less friction.

### Option C-1: handwritten recursive descent

Add a `parse_quoted_triple(s: &str) -> Result<Triple>` helper that:

1. Strips the leading `<<` and trailing `>>` (and a single space on
   each side if present — the canonical form has them).
2. Tokenises the three inner positions. The wrinkle is nested
   `<<…>>`: a simple split-on-whitespace fails on
   `<< <s> <p> << <s2> <p2> <o2> >> >>` because the inner brackets
   contain spaces.
3. Recurses: subject through `parse_named_or_blank`, predicate as
   `NamedNode::new`, object through `parse_term`.

Tokeniser sketch:

```rust
fn split_three(inner: &str) -> Result<[&str; 3]> {
    // Walk the string tracking bracket depth (`<<` opens, `>>` closes)
    // and quote state. Split at each unnested whitespace run, returning
    // exactly three slices. Error on anything else.
}
```

Pros: zero new dependency, predictable error messages, plays well with
the existing `parse_*` style. Cons: a hand-rolled tokeniser to debug.

### Option C-2: reuse Oxigraph's N-Triples-star line parser

Synthesise a one-line N-Triples-star body
(`{term} <urn:tag:sentinel/p> <urn:tag:sentinel/o> .\n` if `term` is
in subject position, or
`<urn:tag:sentinel/s> <urn:tag:sentinel/p> {term} .\n` for object
position), feed it through `RdfParser::from_format(RdfFormat::NTriples)`,
extract the parsed term, discard the rest.

Pros: zero hand-rolled grammar; nesting handled by the canonical
parser; future-proof against any RDF-1.2 grammar extension Oxigraph
absorbs. Cons: a `.unwrap()` on "exactly one quad came out"; sentinel
IRIs are an ugly seam; allocates a String per call.

### Recommendation

Default to **C-2**. The Conformer's per-triple insert path is not on a
hot loop (the bulk path is `rdf_insert_many` / `rdf_load_*_to_graph`,
neither of which routes through the single-term parser), so the
allocation cost is irrelevant. If a future profiling pass shows
single-row insert dominated by sentinel-string allocation, swap to
C-1; the public behaviour is identical, only the implementation
shifts.

### Affected call sites

- `rdf_insert(s, p, o[, graph])` — `s` and `o` may now be
  N-Triples-star quoted triples. `p` may not (predicates in RDF can
  only be IRIs; the SPARQL-star spec does not extend this).
- `rdf_delete(s, p, o[, graph])` — symmetric.
- `rdf_insert_many` / `rdf_delete_many` — already route through the
  same parsers, so they gain the capability automatically. No surface
  change.
- The `rdf_triples` vtab — same; the `subject` and `object` columns
  accept quoted triples after this change.

### Exit criteria for Phase C

`cargo build` clean. A standalone manual smoke test
(`SELECT rdf_insert('<<<http://e/a> <http://e/p> "x">>', 'http://e/p2', 'http://e/o2');`
followed by `SELECT rdf_dump_ntriples();`) round-trips lossless.

---

## Phase D — teach `rdf_term_type` and add `rdf_triple_{subject,predicate,object}`

Touch `src/functions/rdf_triple.rs`.

### `rdf_term_type` extension

Add a `<<` prefix branch returning `"triple"`. The function stays
deterministic + UTF-8 (`FunctionFlags::UTF8 | FunctionFlags::DETERMINISTIC`).

```rust
let kind = if term.starts_with("<<") {
    "triple"
} else if term.starts_with('<') {
    "iri"
} else if term.starts_with("_:") {
    "blank"
} else if term.starts_with('"') {
    "literal"
} else {
    "unknown"
};
```

Order matters: the `<<` check must precede the `<` check.

### `rdf_term_value` — explicit refusal for triple terms

Add a `<<` arm that returns a SQLite error with a fixed-prefix message:
`"rdf_term_value: triple terms have no scalar value; use rdf_triple_subject / rdf_triple_predicate / rdf_triple_object"`.
The prefix is fixed so consuming gems can prefix-match for refusal
envelopes without parsing the suffix.

### Three new scalars

```sql
rdf_triple_subject(term TEXT)   → TEXT  -- N-Triples-encoded subject
rdf_triple_predicate(term TEXT) → TEXT  -- N-Triples-encoded predicate (always an IRI)
rdf_triple_object(term TEXT)    → TEXT  -- N-Triples-encoded object
```

All three:

- Parse `term` via the Phase C parser path (reused — these are
  one-line wrappers around `parse_term` + `Triple` field access).
- Return the inner N-Triples encoding so the result can feed back into
  any other extension scalar.
- Refuse non-triple inputs with a fixed-prefix error
  (`"rdf_triple_subject: term is not a quoted triple: …"` etc.).

Register all three with `FunctionFlags::UTF8 | FunctionFlags::DETERMINISTIC`
(same as `rdf_term_type` / `rdf_term_value`).

### Exit criteria for Phase D

`cargo build` clean. `SELECT rdf_term_type('<<<a> <b> <c>>>')` returns
`"triple"`. `SELECT rdf_triple_predicate('<<<a> <b> <c>>>')` returns
`<b>`. `cargo test` still green (no new tests yet — Phase E adds them).

---

## Phase E — integration tests

Add to `tests/integration_test.rs`. Group under a `// ── RDF-star ──`
banner near the bottom.

### `test_rdf_star_load_turtle_with_annotation`

1. `rdf_clear()`.
2. Load a Turtle-star body with one annotation block (the example from
   `StarExts.md` §2). The body expands to 1 asserted triple + 2
   annotation triples = 3 quads.
3. Assert `rdf_count() = 3`.

### `test_rdf_star_dump_roundtrip`

1. `rdf_clear()`. Load the same body.
2. `SELECT rdf_dump_ntriples()`.
3. Assert the output contains `<<` and `>>` (negative-pin against the
   pre-0.7.0 stub literal).
4. Assert the output is byte-identical (modulo line order — sort first)
   to a hand-written expected fixture.
5. Round-trip: `rdf_clear()`, then `rdf_load_ntriples(<the dump>)`,
   then assert `rdf_count_all()` is unchanged.

### `test_rdf_star_insert_quoted_subject_via_rdf_insert`

1. `rdf_clear()`.
2. `SELECT rdf_insert('<< <http://e/a> <http://e/p> "x" >>', 'http://e/q', 'http://e/b')`.
3. Assert `rdf_count() = 1`.
4. Assert `rdf_dump_ntriples()` contains the inserted quoted-triple
   subject.

### `test_rdf_star_insert_quoted_object_via_rdf_insert`

Symmetric — quoted triple in object position.

### `test_rdf_star_vtab_insert_and_select`

1. `rdf_clear()`.
2. `INSERT INTO triples VALUES ('<<…>>', '<p>', '"y"')`.
3. `SELECT subject FROM triples` round-trips the quoted-triple form.

### `test_rdf_star_sparql_query_annotation_shorthand`

The §2 worked example from `StarExts.md`:

```sparql
SELECT ?val ?stater WHERE {
  :bob :name ?val {| :statedBy ?stater |} .
}
```

Assert the JSON envelope has one row with the expected `?val` and
`?stater` bindings (both bare IRIs / literals — no triple-term
bindings in this query, so no encoding change is exercised).

### `test_rdf_star_sparql_query_triple_term_binding`

```sparql
SELECT ?t ?stater WHERE { ?t :statedBy ?stater . }
```

`?t` binds to a triple term. Assert the JSON value for `?t` is the
expected N-Triples-star encoding (the new Phase B output). This is the
SPARQL-star-side round-trip pin.

### `test_rdf_star_sparql_construct`

CONSTRUCT a graph that copies annotations into a new predicate. Assert
the returned N-Triples blob contains valid `<<…>>` forms and parses
back through `rdf_load_ntriples` to the expected quad count.

### `test_rdf_star_builtin_TRIPLE`

```sparql
SELECT (TRIPLE(<http://e/s>, <http://e/p>, <http://e/o>) AS ?t) WHERE {}
```

Assert `?t` binds to `<< <http://e/s> <http://e/p> <http://e/o> >>`.

### `test_rdf_star_builtin_SUBJECT_PREDICATE_OBJECT_isTRIPLE`

Single query exercising all four destructor built-ins on a single
bound triple term. Pin the four bindings.

### `test_rdf_term_type_triple`

`SELECT rdf_term_type('<<<a> <b> <c>>>')` returns `"triple"`.

### `test_rdf_triple_subject_predicate_object_scalars`

`SELECT rdf_triple_subject('<< <a> <b> <c> >>')` returns `<a>`, etc.

### `test_rdf_term_value_refuses_triple`

`SELECT rdf_term_value('<<<a> <b> <c>>>')` raises an error whose
message starts with the fixed prefix. Pins the refusal envelope shape.

### `test_rdf_star_nested_triple_roundtrip`

`<< << <a> <b> <c> >> <d> <e> >>` round-trips through `rdf_insert` and
`rdf_dump_ntriples`. Pins the recursive parser/serialiser pair.

### Exit criteria for Phase E

```
cargo test               # all green
cargo test --release     # same
```

Test count climbs by 14.

---

## Phase F — docs

- `README.md` — new top-level section "RDF-star / SPARQL-star" between
  "Virtual Table" and "Rails Integration". One paragraph framing
  (point at `docs/research/StarExts.md`), then the surface delta:
  - `rdf_insert` / `rdf_delete` / `rdf_insert_many` / `rdf_delete_many`
    / `rdf_triples` vtab now accept `<<…>>` quoted-triple terms in
    subject and object position.
  - `rdf_dump_ntriples` / `sparql_construct` / `sparql_query` JSON
    emit `<<…>>` forms for triple-term bindings.
  - `rdf_term_type` returns `"triple"` for quoted-triple terms.
  - `rdf_triple_subject(t)` / `_predicate(t)` / `_object(t)` extract
    the parts of a quoted triple.
  - SPARQL-star query syntax (annotation shorthand, `<<…>>` patterns,
    `TRIPLE` / `SUBJECT` / `PREDICATE` / `OBJECT` / `isTRIPLE`) flows
    straight through to Oxigraph; no SQL wrapper required.

- `CLAUDE.md` —
  - "SQL Function Reference" table gains rows for the three new
    `rdf_triple_*` scalars.
  - "Term Utilities" subsection notes the `"triple"` return value
    from `rdf_term_type` and the refusal contract from
    `rdf_term_value`.
  - "Completing the implementation" — strike item #2's SPARQL-star
    bullet (now done) if present; otherwise leave alone. Verify
    against the file before editing.

- `CHANGELOG.md` — new 0.7.0 entry. Lead with "RDF-star /
  SPARQL-star round-trip — quoted-triple terms now survive the SQL
  boundary in both directions". Cross-reference
  `docs/research/StarExts.md`. List the new scalars, the changed
  semantics on existing scalars (`rdf_term_type` / `rdf_term_value`),
  and a one-line note that no existing test changes — the surface is
  purely additive plus one negative-stub removal.

- `src/functions/rdf_triple.rs` doc-comment table at the top — add
  the three new `rdf_triple_*` scalars and the new
  `"triple"` return value of `rdf_term_type`.

- `src/functions/sparql_query.rs:209` — delete the
  `// RDF-star quoted triples are out of scope for 0.1.x.` comment.
  Replace it with one line: `// N-Triples-star encoding: <<s p o>>`.

### `CONSUMER_REQUIREMENT_MM.md` graduation

This release lands surface MM does not call yet, so the graduation is
to the "Available upstream but not exercised by MM" section, not to
the "SQL surfaces MM consumes" section. Add rows for:

- "RDF-star / SPARQL-star round-trip — quoted-triple terms accepted
  in subject/object position by every write path; emitted by every
  read path. See `docs/research/StarExts.md`."
- The three new `rdf_triple_*` scalars.

When MM's vv-memory PLAN adopts the Conformer, move the rows up to
"SQL surfaces MM consumes" and add a "Requested" → "LANDED" history
block in the same style as the 0.3.0 / 0.4.0 / 0.6.0 graduations.

### `CONSUMER_REQUIREMENT_RS.md` touchup

No RS-side ask today (RS does not use RDF-star). Add a single line to
its equivalent "Available upstream but not exercised" section pointing
at the new capability and at `docs/research/StarExts.md`.

### Exit criteria for Phase F

`README.md` documents the new surface. `CHANGELOG.md` has a 0.7.0
entry. `CLAUDE.md` table is current.

---

## Phase G — tag 0.7.0

- Bump `Cargo.toml` and `VERSION` to `0.7.0`.
- `cargo test` and `cargo test --release` both green at the bumped
  version.
- `git tag v0.7.0` and push.
- Ping MM's vv-memory Conformer plan (open in
  `CONSUMER_REQUIREMENT_MM.md`'s referenced MM repo plan dir) that
  the substrate side is ready; the Conformer's PLAN can now pin a
  concrete `sqlite-sparql` rev rather than gating on "engine support
  TBD".

---

## Risks

- **Oxigraph 0.4 parser-format coverage of star.** This plan assumes
  Oxigraph 0.4's `RdfFormat::Turtle` / `RdfFormat::NTriples` /
  `RdfFormat::TriG` parsers accept RDF-star syntax by default. If
  Phase A's `probe_turtle_star_load` fails (no quads loaded, or a
  parse error), the plan's foundation breaks: either the parser needs
  a per-instance `with_quoted_triples()`-style toggle, or Oxigraph
  0.4 splits star handling into separate format variants. In either
  case, Phase A surfaces the gap before any user-visible change ships.
  The remediation is a single-line change to `RdfParser::from_format`
  call sites in `src/functions/rdf_triple.rs:308`. Surface the gap;
  don't paper over it.

- **`Term::Triple` storage at the engine layer.** Phase A also
  exercises the assertion that `Store::insert(&quad)` accepts a quad
  whose subject or object is a `Term::Triple`. If it doesn't (e.g.
  returns a "term type not supported" `StoreError`), the entire plan
  is moot — the substrate can't hold the data the SQL surface would
  parse. Same response: surface the gap, file an Oxigraph upstream
  issue if confirmed, do not ship 0.7.0 until the gap closes.

- **Whitespace canonicalisation for `<<…>>`.** Phase A pins behaviour
  the Phase E test fixtures depend on. If Oxigraph's own
  N-Triples-star serialiser uses a different spacing convention than
  the `<< s p o >>` form this plan picks (e.g. `<<s p o>>` with no
  inner spaces, or a leading `\n`), the round-trip
  `rdf_dump_ntriples → rdf_load_ntriples` still works (Oxigraph
  re-ingests both) but the byte-identical fixtures in
  `test_rdf_star_dump_roundtrip` need rewriting against the chosen
  form. Pick our own canonical form; document it in the source
  comment we replace the stub TODO with.

- **`rdf_term_value` behaviour change.** Today
  `rdf_term_value('<<…>>')` raises `InvalidArgument` with the generic
  message `"unrecognised term format: …"`. After this plan it raises
  a fixed-prefix message. Any caller prefix-matching on the old
  string breaks. No known caller does (RS and MM both treat
  `rdf_term_value` as opaque per their CONSUMER docs), but the
  CHANGELOG must call the message change out under "behaviour
  changes".

- **Parser drift between the loader path and the single-term path.**
  Phase C option C-2 reuses the same Oxigraph N-Triples-star parser
  the loader uses, so the two paths can't drift. Phase C option C-1
  re-implements the grammar, opening a drift surface. The
  `test_insert_many_parser_parity_with_single` pattern from 0.4.0 is
  the precedent for guarding this; add an equivalent
  `test_insert_parser_parity_for_star` to Phase E if C-1 is picked.
  Under C-2 the test is redundant.

- **Recursion depth.** Nested `<<…>>` recurses through
  `term_to_ntriples` and through the chosen Phase C parser. RDF-star
  doesn't cap nesting depth; in practice no real data nests more than
  two levels (statement-about-statement is the dominant pattern).
  Stack-blowing on adversarial input is a theoretical risk that
  doesn't merit a recursion guard at this version. Note it; revisit
  if the Conformer ever emits pathologically nested terms.

---

## Out of scope for 0.7.0

- **A SPARQL-star surface dedicated function.** Today `sparql_query`,
  `sparql_ask`, `sparql_construct`, `sparql_update` all pass the
  query string straight to Oxigraph, which already accepts
  SPARQL-star. No new SQL-side function is needed; the only thing
  that changes is the JSON encoding of triple-term bindings, covered
  in Phase B.

- **An `rdf_triple_build(s, p, o)` scalar.** A SQL-side constructor
  for quoted triples would let callers build a `<<…>>` string by
  function call rather than string concatenation. Symmetric with
  `rdf_triple_{subject,predicate,object}`. Skip — every caller
  already builds N-Triples strings via concatenation today (the
  `rdf_insert` arg position), and the SPARQL-side `TRIPLE(s, p, o)`
  built-in covers the query path. Add when a consumer asks.

- **Reification interop.** `StarExts.md` §4 describes the "unstar
  mapping" that rewrites RDF-star into pure RDF + reification. Useful
  for export to non-star-aware stores. No consumer asks today; defer.

- **`rdf_dump_ntriples_star_only()` / filtering by term shape.** A
  variant that emits only the asserted triples (no annotation triples)
  or only the annotation triples. Useful for the Conformer's
  "asserted truth" view vs. "provenance" view. No consumer asks today
  and the same effect is reachable via `sparql_construct` with a
  shape-filtering `FILTER`. Defer.

- **Persistent RocksDB backend.** Deferred indefinitely. No consumer
  asks for persistence — MM runs against an in-memory store rebuilt
  from the episode log on boot, RS exercises the in-memory shape only,
  and neither CONSUMER doc carries a persistence requirement. The
  Conformer's need for star round-trip is real and proximate; the
  operator's need for persistence is hypothetical. Pull it from the
  scheduled roadmap; revive when a consumer files an ask.

---

## Re-numbering downstream milestones

`PLAN_0.2.0.md`'s roadmap table currently lists (after PLAN_0.6.0's
renumber):

| Version | Topic |
|---|---|
| 0.7.0 | Persistent RocksDB backend |
| 0.8.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.9.0 | SPARQL HTTP endpoint |

After this plan, RocksDB drops off the scheduled roadmap entirely
(see "Out of scope" — no consumer pressure) and the remaining items
shift up:

| Version | Topic |
|---|---|
| 0.7.0 | RDF-star / SPARQL-star round-trip (this file) |
| 0.8.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.9.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB backend — revive on first consumer ask |

Update the table in `PLAN_0.2.0.md` as part of Phase F's doc pass. The
RocksDB row moves below the scheduled rows under a "Deferred" header
rather than carrying a version number. Do **not** edit
`PLAN_0.2.0.md`'s prose — the renumbering and the deferred-row split
are the only changes there.
