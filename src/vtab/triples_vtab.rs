/// `rdf_triples` — a read/write virtual table over the Oxigraph triple store.
///
/// This virtual table exposes the contents of the thread-local Oxigraph store
/// as a regular SQLite table with three columns:
///
/// | Column    | Type | Description                              |
/// |-----------|------|------------------------------------------|
/// | subject   | TEXT | N-Triples encoded subject term           |
/// | predicate | TEXT | N-Triples encoded predicate (IRI)        |
/// | object    | TEXT | N-Triples encoded object term            |
///
/// ## DDL
/// ```sql
/// CREATE VIRTUAL TABLE triples USING rdf_triples();
/// ```
///
/// ## DML
/// ```sql
/// -- Insert a triple
/// INSERT INTO triples VALUES (
///   'http://example.org/alice',
///   'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
///   'http://xmlns.com/foaf/0.1/Person'
/// );
///
/// -- Query triples
/// SELECT * FROM triples;
///
/// -- Delete a triple
/// DELETE FROM triples
///  WHERE subject   = 'http://example.org/alice'
///    AND predicate = 'http://www.w3.org/1999/02/22-rdf-syntax-ns#type'
///    AND object    = 'http://xmlns.com/foaf/0.1/Person';
/// ```
use sqlite_loadable::{
    prelude::*,
    table::{IndexInfo, UpdateOperation, VTab, VTabArguments, VTabCursor, VTabWriteable},
    BestIndexError, Error, Result,
};

use crate::functions::sparql_query::{term_to_ntriples, term_to_ntriples_subject};
use crate::store::{insert_triple, with_store};
use oxigraph::model::{GraphNameRef, Term};
use std::marker::PhantomData;
use std::mem;
use std::os::raw::c_int;

// ── Virtual table definition ──────────────────────────────────────────────────

static CREATE_SQL: &str =
    "CREATE TABLE x(subject TEXT, predicate TEXT, object TEXT)";

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
        use sqlite_loadable::api;

        match operation {
            UpdateOperation::Insert { values, rowid: _ } => {
                let s = api::value_text(&values[0])?;
                let p = api::value_text(&values[1])?;
                let o = api::value_text(&values[2])?;
                insert_triple(s, p, o).map_err(Error::from)?;
                Ok(())
            }
            // 0.1.0: rowid-driven DELETE/UPDATE on rdf_triples is not supported
            // because the cursor's rowid is a position in a per-cursor materialised
            // scan, with no stable mapping back to a triple. Users should use
            // rdf_delete(s, p, o) or a SPARQL DELETE query instead.
            UpdateOperation::Delete(_) => Err(Error::new_message(
                "DELETE on rdf_triples is not supported in 0.1.x — use rdf_delete(s,p,o) or SPARQL DELETE",
            )),
            UpdateOperation::Update { .. } => Err(Error::new_message(
                "UPDATE on rdf_triples is not supported in 0.1.x — use rdf_delete + rdf_insert",
            )),
        }
    }
}

// ── Cursor ────────────────────────────────────────────────────────────────────

#[repr(C)]
pub struct RdfTriplesCursor<'vtab> {
    /// SQLite requires the `sqlite3_vtab_cursor` base struct as the first
    /// field of every concrete cursor type.
    base: sqlite3_vtab_cursor,
    rows: Vec<(String, String, String)>,
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
        // Load all quads from the default graph into our row buffer.
        self.rows = with_store(|store| {
            let mut rows = Vec::new();
            for quad in store.quads_for_pattern(
                None,
                None,
                None,
                Some(GraphNameRef::DefaultGraph),
            ) {
                if let Ok(quad) = quad {
                    let s = term_to_ntriples_subject(&quad.subject);
                    let p = format!("<{}>", quad.predicate.as_str());
                    let o = term_to_ntriples(&Term::from(quad.object));
                    rows.push((s, p, o));
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
