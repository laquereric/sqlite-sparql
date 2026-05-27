# frozen_string_literal: true

require_relative "test_helper"
require "active_record"
require "sqlite_sparql/has_rdf_triples"

class TestHasRdfTriples < Minitest::Test
  class FakeKnowledge < ActiveRecord::Base
    self.table_name = "knowledge"
    include SqliteSparql::HasRdfTriples

    def sync_to_rdf_store
      rdf_store.insert(subject_iri, predicate_iri, object_iri)
    end

    def remove_from_rdf_store
      rdf_store.delete(subject_iri, predicate_iri, object_iri)
    end
  end

  def setup
    # One AR connection backed by a fresh in-memory SQLite database.
    ActiveRecord::Base.establish_connection(adapter: "sqlite3", database: ":memory:")
    ActiveRecord::Schema.define do
      create_table :knowledge, force: true do |t|
        t.string :subject_iri,   null: false
        t.string :predicate_iri, null: false
        t.string :object_iri,    null: false
      end
    end
    FakeKnowledge.instance_variable_set(:@_rdf_store, nil)
    FakeKnowledge.instance_variable_set(:@_rdf_store_raw, nil)
    FakeKnowledge.rdf_store.clear
  end

  def teardown
    ActiveRecord::Base.connection.disconnect! if ActiveRecord::Base.connected?
  end

  def test_create_syncs_to_store
    assert_equal 0, FakeKnowledge.rdf_store.count
    FakeKnowledge.create!(
      subject_iri:   "http://e/alice",
      predicate_iri: "http://e/knows",
      object_iri:    "http://e/bob",
    )
    assert_equal 1, FakeKnowledge.rdf_store.count
  end

  def test_destroy_retracts_from_store
    record = FakeKnowledge.create!(
      subject_iri:   "http://e/alice",
      predicate_iri: "http://e/knows",
      object_iri:    "http://e/bob",
    )
    assert_equal 1, FakeKnowledge.rdf_store.count
    record.destroy!
    assert_equal 0, FakeKnowledge.rdf_store.count
  end

  def test_class_sparql_delegator
    FakeKnowledge.create!(
      subject_iri:   "http://e/alice",
      predicate_iri: "http://e/knows",
      object_iri:    "http://e/bob",
    )
    results = FakeKnowledge.sparql("SELECT ?o WHERE { <http://e/alice> ?p ?o }")
    assert_kind_of Array, results
    assert_equal 1, results.length
  end

  def test_class_materialise_delegator_returns_integer
    FakeKnowledge.create!(
      subject_iri: "http://e/Dog",
      predicate_iri: "http://www.w3.org/2000/01/rdf-schema#subClassOf",
      object_iri: "http://e/Animal",
    )
    delta = FakeKnowledge.materialise(inferred: "urn:g:inferred")
    assert_kind_of Integer, delta
  end

  def test_class_consistent_delegator
    FakeKnowledge.create!(
      subject_iri: "http://e/alice",
      predicate_iri: "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
      object_iri: "http://e/Person",
    )
    assert FakeKnowledge.consistent?(inferred: "urn:g:inferred")
  end
end
