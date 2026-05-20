//! # sqlite-sparql
//!
//! A SQLite loadable extension that embeds [Oxigraph](https://github.com/oxigraph/oxigraph)
//! to provide native RDF triple storage and SPARQL querying directly within SQLite.
//!
//! ## SQL Functions provided
//!
//! ### Triple Management
//! | Function | Signature | Description |
//! |---|---|---|
//! | `rdf_insert` | `(subject TEXT, predicate TEXT, object TEXT) → INTEGER` | Insert one triple |
//! | `rdf_delete` | `(subject TEXT, predicate TEXT, object TEXT) → INTEGER` | Delete one triple |
//! | `rdf_clear` | `() → INTEGER` | Reset the entire store |
//! | `rdf_count` | `() → INTEGER` | Count triples in the store |
//! | `rdf_load_turtle` | `(turtle TEXT) → INTEGER` | Bulk-load from Turtle format |
//! | `rdf_load_ntriples` | `(ntriples TEXT) → INTEGER` | Bulk-load from N-Triples format |
//! | `rdf_load_rdfxml` | `(rdfxml TEXT) → INTEGER` | Bulk-load from RDF/XML format |
//! | `rdf_dump_ntriples` | `() → TEXT` | Dump all triples as N-Triples |
//!
//! ### Term Utilities
//! | Function | Signature | Description |
//! |---|---|---|
//! | `rdf_term_type` | `(term TEXT) → TEXT` | Returns "iri", "blank", or "literal" |
//! | `rdf_term_value` | `(term TEXT) → TEXT` | Extracts the plain string value |
//!
//! ### SPARQL Querying
//! | Function | Signature | Description |
//! |---|---|---|
//! | `sparql_query` | `(query TEXT) → TEXT (JSON)` | Execute a SELECT query |
//! | `sparql_ask` | `(query TEXT) → INTEGER (0/1)` | Execute an ASK query |
//! | `sparql_construct` | `(query TEXT) → TEXT (N-Triples)` | Execute a CONSTRUCT query |
//!
//! ### Virtual Table
//! | Module | DDL | Description |
//! |---|---|---|
//! | `rdf_triples` | `CREATE VIRTUAL TABLE t USING rdf_triples()` | Read/write view of the triple store |
//!
//! ## Quick Start (SQL)
//!
//! ```sql
//! -- Load the extension
//! .load ./target/release/libsqlite_sparql
//!
//! -- Insert triples
//! SELECT rdf_insert(
//!   'http://example.org/alice',
//!   'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
//!   'http://xmlns.com/foaf/0.1/Person'
//! );
//!
//! -- Query via SPARQL SELECT
//! SELECT sparql_query('SELECT ?s WHERE { ?s a <http://xmlns.com/foaf/0.1/Person> }');
//!
//! -- Use the virtual table
//! CREATE VIRTUAL TABLE triples USING rdf_triples();
//! SELECT * FROM triples;
//! ```

use sqlite_loadable::prelude::*;

pub mod error;
pub mod functions;
pub mod store;
pub mod vtab;

/// Extension entry point — called by SQLite when the extension is loaded.
///
/// Registers all SQL functions and virtual table modules provided by
/// `sqlite-sparql`.
#[sqlite_entrypoint]
pub fn sqlite3_sqlitesparql_init(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    // Register RDF triple management scalar functions
    functions::rdf_triple::register(db)?;

    // Register batched insert / delete (rdf_insert_many, rdf_delete_many)
    functions::rdf_bulk::register(db)?;

    // Register SPARQL query scalar functions
    functions::sparql_query::register(db)?;

    // Register the rdf_triples virtual table module
    vtab::triples_vtab::register(db)?;

    Ok(())
}
