# frozen_string_literal: true

require_relative "test_helper"

class TestStore < Minitest::Test
  def setup
    @db = SQLite3::Database.new(":memory:")
    @store = SqliteSparql::Store.new(@db)
    @store.clear
  end

  def teardown
    @db.close if @db && !@db.closed?
  end

  def test_insert_and_count
    assert_equal 0, @store.count
    @store.insert("http://e/a", "http://e/p", "http://e/b")
    assert_equal 1, @store.count
  end

  def test_delete
    @store.insert("http://e/a", "http://e/p", "http://e/b")
    @store.delete("http://e/a", "http://e/p", "http://e/b")
    assert_equal 0, @store.count
  end

  def test_named_graph_insert_and_count
    @store.insert("http://e/a", "http://e/p", "http://e/b", graph: "urn:g:x")
    assert_equal 1, @store.count(graph: "urn:g:x")
    assert_equal 0, @store.count # default graph still empty
  end

  def test_sparql_select_returns_array_of_hashes
    @store.insert("http://e/alice", "http://e/name", '"Alice"')
    results = @store.sparql("SELECT ?o WHERE { <http://e/alice> ?p ?o }")
    assert_kind_of Array, results
    assert_equal 1, results.length
    assert_kind_of Hash, results.first
  end

  def test_sparql_ask_returns_boolean
    @store.insert("http://e/a", "http://e/p", "http://e/b")
    assert_equal true, @store.ask("ASK { <http://e/a> ?p ?o }")
    assert_equal false, @store.ask("ASK { <http://e/no-such> ?p ?o }")
  end

  def test_sparql_construct_returns_ntriples_text
    @store.insert("http://e/a", "http://e/p", "http://e/b")
    out = @store.construct("CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }")
    assert_kind_of String, out
    assert out.include?("<http://e/a>")
  end

  def test_load_turtle
    n = @store.load_turtle(<<~TTL)
      @prefix ex: <http://example.org/> .
      ex:alice ex:knows ex:bob .
      ex:bob   ex:knows ex:carol .
    TTL
    assert_equal 2, n
    assert_equal 2, @store.count
  end

  def test_insert_many_and_delete_many
    rows = [
      ["http://e/a", "http://e/p", "http://e/x"],
      ["http://e/a", "http://e/p", "http://e/y"],
      ["http://e/b", "http://e/p", "http://e/z"],
    ]
    assert_equal 3, @store.insert_many(rows)
    assert_equal 3, @store.count
    assert_equal 2, @store.delete_many(rows.first(2))
    assert_equal 1, @store.count
  end

  def test_materialise_and_consistent
    # T-Box + A-Box that triggers scm-sco and cax-sco.
    @store.insert(
      "http://e/Dog",
      "http://www.w3.org/2000/01/rdf-schema#subClassOf",
      "http://e/Animal"
    )
    @store.insert(
      "http://e/fido",
      "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
      "http://e/Dog"
    )
    delta = @store.materialise(inferred: "urn:g:inferred")
    assert_operator delta, :>=, 1
    assert @store.consistent?(inferred: "urn:g:inferred")
    assert_empty @store.consistency_violations(inferred: "urn:g:inferred")
  end

  def test_consistency_flags_cax_dw_violation
    @store.insert(
      "http://e/Animal",
      "http://www.w3.org/2002/07/owl#disjointWith",
      "http://e/Plant"
    )
    @store.insert(
      "http://e/alice",
      "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
      "http://e/Animal"
    )
    @store.insert(
      "http://e/alice",
      "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
      "http://e/Plant"
    )
    refute @store.consistent?(inferred: "urn:g:inferred")
    violations = @store.consistency_violations(inferred: "urn:g:inferred")
    assert_equal 1, violations.length
    assert_equal "cax-dw", violations.first["rule"]
  end

  def test_dred_overdelete_round_trip
    @store.insert(
      "http://e/A",
      "http://www.w3.org/2000/01/rdf-schema#subClassOf",
      "http://e/B"
    )
    @store.insert(
      "http://e/B",
      "http://www.w3.org/2000/01/rdf-schema#subClassOf",
      "http://e/C"
    )
    @store.materialise(inferred: "urn:g:inferred", options: { "track_dependencies" => true })
    removed = @store.dred_overdelete(
      inferred: "urn:g:inferred",
      retracted_premises: [
        ["http://e/B",
         "http://www.w3.org/2000/01/rdf-schema#subClassOf",
         "http://e/C"]
      ]
    )
    assert_equal 1, removed
  end

  def test_shacl_validate
    @store.load_turtle(<<~TTL, graph: "urn:g:shapes")
      @prefix sh: <http://www.w3.org/ns/shacl#> .
      @prefix ex: <http://example.org/> .
      ex:PersonShape a sh:NodeShape ;
        sh:targetClass ex:Person ;
        sh:property [ sh:path ex:name ; sh:minCount 1 ] .
    TTL
    @store.load_turtle(<<~TTL, graph: "urn:g:data")
      @prefix ex: <http://example.org/> .
      ex:alice a ex:Person ; ex:name "Alice" .
      ex:bob   a ex:Person .
    TTL
    count = @store.shacl_validate(
      data: "urn:g:data",
      shapes: "urn:g:shapes",
      report: "urn:g:report"
    )
    assert_equal 1, count
  end

  def test_term_helpers
    assert_equal "iri",   @store.term_type("<http://example.org/>")
    assert_equal "blank", @store.term_type("_:b0")
  end

  def test_clear_returns_true
    @store.insert("http://e/a", "http://e/p", "http://e/b")
    assert_equal true, @store.clear
    assert_equal 0, @store.count
  end
end
