# frozen_string_literal: true

require "json"

module SqliteSparql
  # Ergonomic wrapper around a loaded `SQLite3::Database`. Every method
  # corresponds to one of the engine's SQL surfaces; arguments are passed
  # through with no transformation other than JSON encoding for the
  # options-hash slots.
  #
  # Construct with a database that's already had the extension loaded,
  # or pass an unloaded one and the constructor will load it for you:
  #
  #   store = SqliteSparql::Store.new(SQLite3::Database.new(":memory:"))
  #   store.insert("<urn:a>", "<urn:b>", "<urn:c>")
  #   store.sparql("SELECT ?o WHERE { <urn:a> ?p ?o }")
  class Store
    attr_reader :db

    def initialize(db)
      SqliteSparql.load(db) unless self.class.loaded?(db)
      @db = db
    end

    # ── Triple management ────────────────────────────────────────────────

    def insert(s, p, o, graph: nil)
      if graph
        @db.get_first_value("SELECT rdf_insert(?, ?, ?, ?)", [s, p, o, graph]).to_i
      else
        @db.get_first_value("SELECT rdf_insert(?, ?, ?)", [s, p, o]).to_i
      end
    end

    def delete(s, p, o, graph: nil)
      if graph
        @db.get_first_value("SELECT rdf_delete(?, ?, ?, ?)", [s, p, o, graph]).to_i
      else
        @db.get_first_value("SELECT rdf_delete(?, ?, ?)", [s, p, o]).to_i
      end
    end

    def clear
      @db.get_first_value("SELECT rdf_clear()").to_i == 1
    end

    def count(graph: nil)
      if graph
        @db.get_first_value("SELECT rdf_count(?)", [graph]).to_i
      else
        @db.get_first_value("SELECT rdf_count()").to_i
      end
    end

    def count_all
      @db.get_first_value("SELECT rdf_count_all()").to_i
    end

    # ── Bulk insert / delete (rdf_insert_many / rdf_delete_many) ─────────

    # rows: Array of [s, p, o] or [s, p, o, graph] arrays.
    def insert_many(rows)
      @db.get_first_value("SELECT rdf_insert_many(?)", [JSON.dump(rows)]).to_i
    end

    def delete_many(rows)
      @db.get_first_value("SELECT rdf_delete_many(?)", [JSON.dump(rows)]).to_i
    end

    # ── SPARQL ───────────────────────────────────────────────────────────

    # SELECT — returns parsed JSON (Array<Hash> of binding sets).
    def sparql(query)
      json = @db.get_first_value("SELECT sparql_query(?)", [query])
      JSON.parse(json)
    end

    def ask(query)
      @db.get_first_value("SELECT sparql_ask(?)", [query]).to_i == 1
    end

    # CONSTRUCT — returns the N-Triples text directly.
    def construct(query)
      @db.get_first_value("SELECT sparql_construct(?)", [query])
    end

    # Batched CONSTRUCT (since 0.8.0). `queries` is an Array<String>;
    # returns Array<String> of per-query N-Triples blobs.
    def construct_many(queries)
      json = @db.get_first_value("SELECT rdf_construct_many(?)", [JSON.dump(queries)])
      JSON.parse(json)
    end

    # SPARQL UPDATE — returns the signed net store-size delta.
    def update(query)
      @db.get_first_value("SELECT sparql_update(?)", [query]).to_i
    end

    # ── Term utilities ───────────────────────────────────────────────────

    def term_type(term)
      @db.get_first_value("SELECT rdf_term_type(?)", [term])
    end

    def term_value(term)
      @db.get_first_value("SELECT rdf_term_value(?)", [term])
    end

    # ── Bulk load ────────────────────────────────────────────────────────

    def load_turtle(text, graph: nil)
      bulk_load("turtle", text, graph)
    end

    def load_ntriples(text, graph: nil)
      bulk_load("ntriples", text, graph)
    end

    def load_rdfxml(text, graph: nil)
      bulk_load("rdfxml", text, graph)
    end

    def dump_ntriples
      @db.get_first_value("SELECT rdf_dump_ntriples()")
    end

    # ── Reasoning & validation (0.9.0+) ──────────────────────────────────

    # Native OWL 2 RL fixpoint pass. `inferred:` is required (the engine
    # rejects NULL); `asserted:` defaults to the default graph.
    def materialise(inferred:, asserted: nil, options: {})
      @db.get_first_value(
        "SELECT rdf_owl_rl_materialise(?, ?, ?)",
        [asserted, inferred, JSON.dump(options)]
      ).to_i
    end

    # OWL 2 RL inconsistency detection (since 0.13.0). Returns the JSON
    # array parsed into Array<Hash{"rule"=>String, "s"=>String,
    # "p"=>String, "o"=>String}>. Empty array means consistent.
    def consistency_violations(inferred:, asserted: nil, options: {})
      json = @db.get_first_value(
        "SELECT rdf_owl_rl_consistent(?, ?, ?)",
        [asserted, inferred, JSON.dump(options)]
      )
      JSON.parse(json)
    end

    def consistent?(inferred:, asserted: nil, options: {})
      consistency_violations(inferred: inferred, asserted: asserted, options: options).empty?
    end

    # Native SHACL Core validator (since 0.11.0).
    def shacl_validate(shapes:, report:, data: nil, options: {})
      @db.get_first_value(
        "SELECT rdf_shacl_core_validate(?, ?, ?, ?)",
        [data, shapes, report, JSON.dump(options)]
      ).to_i
    end

    # Native DRed over-deletion (since 0.12.0). `retracted_premises` is an
    # Array of [s, p, o] or [s, p, o, graph] arrays.
    def dred_overdelete(inferred:, retracted_premises:)
      @db.get_first_value(
        "SELECT rdf_dred_overdelete(?, ?)",
        [inferred, JSON.dump(retracted_premises)]
      ).to_i
    end

    # ── Class helpers ────────────────────────────────────────────────────

    # Detect whether the extension is loaded on this connection. Used by
    # the constructor to avoid re-loading when the consumer has already
    # called `SqliteSparql.load(db)`.
    def self.loaded?(db)
      db.get_first_value("SELECT rdf_count()") && true
    rescue ::SQLite3::SQLException
      false
    end

    private

    def bulk_load(format, text, graph)
      if graph
        @db.get_first_value("SELECT rdf_load_#{format}_to_graph(?, ?)", [text, graph]).to_i
      else
        @db.get_first_value("SELECT rdf_load_#{format}(?)", [text]).to_i
      end
    end
  end
end
