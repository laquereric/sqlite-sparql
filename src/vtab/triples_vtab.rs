/// `rdf_triples` — a read/write virtual table over the Oxigraph triple store.
///
/// Logical columns (from 0.3.0):
///
/// | Column    | Type | Visible in `SELECT *`? | Description                              |
/// |-----------|------|------------------------|------------------------------------------|
/// | subject   | TEXT | yes                    | N-Triples encoded subject term           |
/// | predicate | TEXT | yes                    | N-Triples encoded predicate IRI          |
/// | object    | TEXT | yes                    | N-Triples encoded object term            |
/// | graph     | TEXT | no (HIDDEN)            | Graph IRI; `NULL` for the default graph  |
///
/// `graph` is declared HIDDEN so that
/// `INSERT INTO triples VALUES (s, p, o)` and `SELECT * FROM triples`
/// keep the 0.1.0 / 0.2.0 shape. Query it by name (`WHERE graph = '…'`)
/// and provide it on writes by naming all four columns
/// (`INSERT INTO triples(subject, predicate, object, graph) VALUES (…)`).
///
/// ## DDL
/// ```sql
/// CREATE VIRTUAL TABLE triples USING rdf_triples();
/// ```
///
/// ## DML
/// ```sql
/// -- 3-col INSERT — lands in the default graph (compat path)
/// INSERT INTO triples VALUES (
///   'http://example.org/alice',
///   'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
///   'http://xmlns.com/foaf/0.1/Person'
/// );
///
/// -- Named graph INSERT — must name all four columns
/// INSERT INTO triples(subject, predicate, object, graph) VALUES (
///   'http://example.org/alice',
///   'http://xmlns.com/foaf/0.1/name',
///   '"Alice"',
///   'urn:g:bhphoto'
/// );
///
/// -- Read the graph column explicitly
/// SELECT subject, graph FROM triples WHERE graph = 'urn:g:bhphoto';
/// ```
use sqlite_loadable::{
    prelude::*,
    table::{IndexInfo, UpdateOperation, VTab, VTabArguments, VTabCursor, VTabWriteable},
    BestIndexError, Error, Result,
};

use crate::functions::sparql_query::{term_to_ntriples, term_to_ntriples_subject};
use crate::store::{insert_triple_in_graph, with_store};
use oxigraph::model::{GraphName, Term};
use std::marker::PhantomData;
use std::mem;
use std::os::raw::c_int;

// ── Virtual table definition ──────────────────────────────────────────────────

static CREATE_SQL: &str =
    "CREATE TABLE x(subject TEXT, predicate TEXT, object TEXT, graph TEXT HIDDEN)";

#[repr(C)]
pub struct RdfTriplesTable {
    /// SQLite requires the `sqlite3_vtab` base struct as the first field
    /// of every concrete vtab type. See <https://www.sqlite.org/vtab.html>.
    base: sqlite3_vtab,
}

impl<'vtab> VTab<'vtab> for RdfTriplesTable {
    type Aux = ();
    type Cursor = RdfTriplesCursor<'vtab>;

    fn connect(
        _db: *mut sqlite3,
        _aux: Option<&()>,
        _args: VTabArguments,
    ) -> Result<(String, RdfTriplesTable)> {
        let vtab = RdfTriplesTable {
            base: unsafe { mem::zeroed() },
        };
        Ok((CREATE_SQL.to_string(), vtab))
    }

    fn best_index(&self, mut info: IndexInfo) -> core::result::Result<(), BestIndexError> {
        // Accept any combination of equality constraints on subject/predicate/object.
        // We scan the full store and filter in Rust; SQLite will not do secondary filtering.
        info.set_estimated_cost(1000.0);
        info.set_estimated_rows(1000);
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<RdfTriplesCursor<'vtab>> {
        Ok(RdfTriplesCursor::new())
    }
}

impl<'vtab> VTabWriteable<'vtab> for RdfTriplesTable {
    fn update(
        &'vtab mut self,
        operation: UpdateOperation,
        _p_rowid: *mut i64,
    ) -> Result<()> {
        use sqlite_loadable::api::{self, ValueType};

        match operation {
            UpdateOperation::Insert { values, rowid: _ } => {
                let s = api::value_text(&values[0])?;
                let p = api::value_text(&values[1])?;
                let o = api::value_text(&values[2])?;
                // `graph` is HIDDEN; on a 3-column INSERT VALUES (...) call
                // SQLite passes NULL for the 4th. Treat NULL as default graph.
                let graph_opt: Option<&str> = match values.get(3) {
                    Some(v) if api::value_type(v) == ValueType::Null => None,
                    Some(v) => Some(api::value_text(v)?),
                    None => None,
                };
                insert_triple_in_graph(s, p, o, graph_opt).map_err(Error::from)?;
                Ok(())
            }
            // 0.1.0: rowid-driven DELETE/UPDATE on rdf_triples is not supported
            // because the cursor's rowid is a position in a per-cursor materialised
            // scan, with no stable mapping back to a triple. Users should use
            // rdf_delete(s, p, o[, graph]) or a SPARQL DELETE query instead.
            UpdateOperation::Delete(_) => Err(Error::new_message(
                "DELETE on rdf_triples is not supported — use rdf_delete(s,p,o[,graph]) or SPARQL DELETE",
            )),
            UpdateOperation::Update { .. } => Err(Error::new_message(
                "UPDATE on rdf_triples is not supported — use rdf_delete + rdf_insert",
            )),
        }
    }
}

// ── Cursor ────────────────────────────────────────────────────────────────────

/// Materialised row from the store. `graph` is `None` for the default graph,
/// `Some(iri)` for a named graph.
type Row = (String, String, String, Option<String>);

#[repr(C)]
pub struct RdfTriplesCursor<'vtab> {
    /// SQLite requires the `sqlite3_vtab_cursor` base struct as the first
    /// field of every concrete cursor type.
    base: sqlite3_vtab_cursor,
    rows: Vec<Row>,
    pos: usize,
    _phantom: PhantomData<&'vtab RdfTriplesTable>,
}

impl<'vtab> RdfTriplesCursor<'vtab> {
    fn new() -> Self {
        RdfTriplesCursor {
            base: unsafe { mem::zeroed() },
            rows: Vec::new(),
            pos: 0,
            _phantom: PhantomData,
        }
    }
}

impl<'vtab> VTabCursor for RdfTriplesCursor<'vtab> {
    fn filter(
        &mut self,
        _idx_num: c_int,
        _idx_str: Option<&str>,
        _values: &[*mut sqlite3_value],
    ) -> Result<()> {
        // Scan every graph (default + named). Passing `None` for the graph
        // filter means "all graphs" in Oxigraph's API.
        self.rows = with_store(|store| {
            let mut rows = Vec::new();
            for quad in store.quads_for_pattern(None, None, None, None) {
                if let Ok(quad) = quad {
                    let s = term_to_ntriples_subject(&quad.subject);
                    let p = format!("<{}>", quad.predicate.as_str());
                    let o = term_to_ntriples(&Term::from(quad.object));
                    let g = match &quad.graph_name {
                        GraphName::DefaultGraph => None,
                        GraphName::NamedNode(n) => Some(n.as_str().to_string()),
                        // Blank-node graphs aren't a write path we accept, but
                        // an Oxigraph-internal one might still appear. Encode
                        // it as N-Triples so reads remain lossless.
                        GraphName::BlankNode(b) => Some(format!("_:{}", b.as_str())),
                    };
                    rows.push((s, p, o, g));
                }
            }
            rows
        });
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.pos += 1;
        Ok(())
    }

    fn eof(&self) -> bool {
        self.pos >= self.rows.len()
    }

    fn column(&self, context: *mut sqlite3_context, col: c_int) -> Result<()> {
        use sqlite_loadable::api;
        if let Some(row) = self.rows.get(self.pos) {
            match col {
                0 => api::result_text(context, &row.0)?,
                1 => api::result_text(context, &row.1)?,
                2 => api::result_text(context, &row.2)?,
                3 => match &row.3 {
                    None => api::result_null(context),
                    Some(g) => api::result_text(context, g)?,
                },
                _ => api::result_null(context),
            }
        }
        Ok(())
    }

    fn rowid(&self) -> Result<i64> {
        Ok(self.pos as i64)
    }
}

/// Register the `rdf_triples` virtual table module.
pub fn register(db: *mut sqlite3) -> Result<()> {
    sqlite_loadable::table::define_virtual_table_writeable::<RdfTriplesTable>(
        db,
        "rdf_triples",
        None,
    )
}
