# frozen_string_literal: true

require "active_support/concern"
require_relative "store"

module SqliteSparql
  # ActiveRecord concern for Rails models that mirror RDF triples in
  # the sqlite-sparql store.
  #
  # The concern provides:
  #
  # - A class-level `rdf_store` accessor backed by the model's own AR
  #   connection — the extension is loaded into that connection lazily
  #   on first use.
  # - Pass-through class methods (`sparql`, `ask`, `construct`,
  #   `materialise`, `consistent?`, `consistency_violations`,
  #   `shacl_validate`, `dred_overdelete`) that delegate to `rdf_store`.
  # - `after_create` and `after_destroy` hooks that call the model's
  #   own `sync_to_rdf_store` / `remove_from_rdf_store` instance
  #   methods *if defined*. The default is a no-op so consumers can
  #   include the concern without immediately defining the mapping.
  #
  # Example:
  #
  #   class Knowledge < ApplicationRecord
  #     include SqliteSparql::HasRdfTriples
  #
  #     def sync_to_rdf_store
  #       rdf_store.insert(subject_iri, predicate_iri, object_iri)
  #     end
  #
  #     def remove_from_rdf_store
  #       rdf_store.delete(subject_iri, predicate_iri, object_iri)
  #     end
  #   end
  #
  #   Knowledge.sparql("SELECT ?s WHERE { ?s a <http://example.org/Person> }")
  #   Knowledge.materialise(inferred: "urn:g:inferred")
  module HasRdfTriples
    extend ActiveSupport::Concern

    class_methods do
      # Returns the SqliteSparql::Store wrapping this model's AR
      # connection. The store is memoised per AR class. If the AR
      # connection pool issues a fresh raw connection, the store is
      # re-built on next access.
      def rdf_store
        raw = connection.raw_connection
        if @_rdf_store_raw.nil? || !@_rdf_store_raw.equal?(raw)
          @_rdf_store = SqliteSparql::Store.new(raw)
          @_rdf_store_raw = raw
        end
        @_rdf_store
      end

      def sparql(query)
        rdf_store.sparql(query)
      end

      def ask(query)
        rdf_store.ask(query)
      end

      def construct(query)
        rdf_store.construct(query)
      end

      def materialise(**kwargs)
        rdf_store.materialise(**kwargs)
      end

      def consistent?(**kwargs)
        rdf_store.consistent?(**kwargs)
      end

      def consistency_violations(**kwargs)
        rdf_store.consistency_violations(**kwargs)
      end

      def shacl_validate(**kwargs)
        rdf_store.shacl_validate(**kwargs)
      end

      def dred_overdelete(**kwargs)
        rdf_store.dred_overdelete(**kwargs)
      end
    end

    included do
      after_create  :_sqlite_sparql_sync
      after_destroy :_sqlite_sparql_retract
    end

    # Instance accessor for the class-level store.
    def rdf_store
      self.class.rdf_store
    end

    private

    def _sqlite_sparql_sync
      sync_to_rdf_store if respond_to?(:sync_to_rdf_store, true)
    end

    def _sqlite_sparql_retract
      remove_from_rdf_store if respond_to?(:remove_from_rdf_store, true)
    end
  end
end
