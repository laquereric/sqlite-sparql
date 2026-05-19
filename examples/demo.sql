-- sqlite-sparql demo script
-- Run with: sqlite3 :memory: < examples/demo.sql
-- Or interactively:
--   sqlite3
--   .load ./target/release/libsqlite_sparql
--   (then paste statements below)

.load ./target/release/libsqlite_sparql

-- ── 1. Insert triples individually ──────────────────────────────────────────

SELECT '--- Inserting triples ---' AS step;

SELECT rdf_insert(
  'http://example.org/alice',
  'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
  'http://xmlns.com/foaf/0.1/Person'
) AS inserted;

SELECT rdf_insert(
  'http://example.org/alice',
  'http://xmlns.com/foaf/0.1/name',
  '"Alice"'
) AS inserted;

SELECT rdf_insert(
  'http://example.org/alice',
  'http://xmlns.com/foaf/0.1/knows',
  'http://example.org/bob'
) AS inserted;

SELECT rdf_insert(
  'http://example.org/bob',
  'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
  'http://xmlns.com/foaf/0.1/Person'
) AS inserted;

SELECT rdf_insert(
  'http://example.org/bob',
  'http://xmlns.com/foaf/0.1/name',
  '"Bob"'
) AS inserted;

SELECT rdf_count() AS triple_count;

-- ── 2. Bulk-load from Turtle ─────────────────────────────────────────────────

SELECT '--- Bulk loading Turtle ---' AS step;

SELECT rdf_load_turtle('
  @prefix foaf: <http://xmlns.com/foaf/0.1/> .
  @prefix ex:   <http://example.org/> .

  ex:carol a foaf:Person ;
           foaf:name "Carol" ;
           foaf:knows ex:alice .
') AS triples_loaded;

SELECT rdf_count() AS triple_count;

-- ── 3. SPARQL SELECT ─────────────────────────────────────────────────────────

SELECT '--- SPARQL SELECT: all people ---' AS step;

SELECT sparql_query(
  'SELECT ?person ?name
   WHERE {
     ?person a <http://xmlns.com/foaf/0.1/Person> .
     ?person <http://xmlns.com/foaf/0.1/name> ?name .
   }
   ORDER BY ?name'
) AS result_json;

-- ── 4. SPARQL ASK ────────────────────────────────────────────────────────────

SELECT '--- SPARQL ASK: does Alice know Bob? ---' AS step;

SELECT sparql_ask(
  'ASK {
     <http://example.org/alice>
       <http://xmlns.com/foaf/0.1/knows>
       <http://example.org/bob>
   }'
) AS alice_knows_bob;

-- ── 5. SPARQL CONSTRUCT ──────────────────────────────────────────────────────

SELECT '--- SPARQL CONSTRUCT: social graph ---' AS step;

SELECT sparql_construct(
  'CONSTRUCT { ?a <http://xmlns.com/foaf/0.1/knows> ?b }
   WHERE     { ?a <http://xmlns.com/foaf/0.1/knows> ?b }'
) AS social_graph_ntriples;

-- ── 6. Virtual table ─────────────────────────────────────────────────────────

SELECT '--- Virtual table: rdf_triples ---' AS step;

CREATE VIRTUAL TABLE triples USING rdf_triples();

SELECT subject, predicate, object FROM triples LIMIT 5;

-- Filter by predicate
SELECT '--- People (via virtual table) ---' AS step;
SELECT rdf_term_value(subject) AS person
FROM   triples
WHERE  predicate = '<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>'
  AND  object    = '<http://xmlns.com/foaf/0.1/Person>';

-- ── 7. Term helpers ──────────────────────────────────────────────────────────

SELECT '--- Term helpers ---' AS step;

SELECT rdf_term_type('<http://example.org/alice>') AS type;   -- iri
SELECT rdf_term_type('_:b0')                       AS type;   -- blank
SELECT rdf_term_type('"Alice"')                    AS type;   -- literal

SELECT rdf_term_value('<http://example.org/alice>') AS value; -- http://example.org/alice
SELECT rdf_term_value('"Alice"@en')                 AS value; -- Alice

-- ── 8. Dump and clear ────────────────────────────────────────────────────────

SELECT '--- Dump all triples as N-Triples ---' AS step;
SELECT rdf_dump_ntriples() AS ntriples_dump;

SELECT '--- Clear store ---' AS step;
SELECT rdf_clear() AS cleared;
SELECT rdf_count() AS triple_count_after_clear;
