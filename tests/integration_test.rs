/// Integration tests for sqlite-sparql.
///
/// These tests load the compiled extension into an in-process SQLite connection
/// via rusqlite and exercise every public SQL function.
///
/// Run with:
///   cargo test
///
/// To see SQL output:
///   cargo test -- --nocapture
#[cfg(test)]
mod tests {
    use rusqlite::{Connection, Result};

    /// Helper: open an in-memory SQLite connection and load the extension.
    ///
    /// The extension is built as a cdylib; the path is resolved relative to
    /// the Cargo workspace root.
    fn open_with_extension() -> Result<Connection> {
        let conn = Connection::open_in_memory()?;
        // The path to our cdylib is exported by build.rs.
        let lib_path = env!("SQLITE_SPARQL_CDYLIB");
        unsafe {
            let guard = rusqlite::LoadExtensionGuard::new(&conn)?;
            conn.load_extension(lib_path, Some("sqlite3_sqlitesparql_init"))?;
            drop(guard);
        }
        Ok(conn)
    }

    // ── rdf_insert / rdf_count ────────────────────────────────────────────────

    #[test]
    fn test_rdf_insert_and_count() -> Result<()> {
        let conn = open_with_extension()?;

        // Insert two triples
        conn.execute_batch(
            "SELECT rdf_insert(
               'http://example.org/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
               'http://xmlns.com/foaf/0.1/Person'
             );
             SELECT rdf_insert(
               'http://example.org/alice',
               'http://xmlns.com/foaf/0.1/name',
               '\"Alice\"'
             );",
        )?;

        let count: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(count, 2, "Expected 2 triples after two inserts");
        Ok(())
    }

    // ── rdf_delete ────────────────────────────────────────────────────────────

    #[test]
    fn test_rdf_delete() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "SELECT rdf_insert(
               'http://example.org/bob',
               'http://xmlns.com/foaf/0.1/name',
               '\"Bob\"'
             );",
        )?;

        let before: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(before, 1);

        conn.execute_batch(
            "SELECT rdf_delete(
               'http://example.org/bob',
               'http://xmlns.com/foaf/0.1/name',
               '\"Bob\"'
             );",
        )?;

        let after: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(after, 0, "Triple should be deleted");
        Ok(())
    }

    // ── rdf_clear ─────────────────────────────────────────────────────────────

    #[test]
    fn test_rdf_clear() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "SELECT rdf_insert('http://a.org/s','http://a.org/p','http://a.org/o');
             SELECT rdf_insert('http://b.org/s','http://b.org/p','http://b.org/o');",
        )?;

        conn.execute_batch("SELECT rdf_clear();")?;
        let count: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(count, 0, "Store should be empty after rdf_clear()");
        Ok(())
    }

    // ── rdf_load_turtle ───────────────────────────────────────────────────────

    #[test]
    fn test_rdf_load_turtle() -> Result<()> {
        let conn = open_with_extension()?;

        let turtle = r#"
            @prefix foaf: <http://xmlns.com/foaf/0.1/> .
            @prefix ex:   <http://example.org/> .

            ex:carol a foaf:Person ;
                     foaf:name "Carol" .
        "#;

        let loaded: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            rusqlite::params![turtle],
            |r| r.get(0),
        )?;
        assert!(loaded >= 2, "Expected at least 2 triples loaded from Turtle");
        Ok(())
    }

    // ── rdf_load_ntriples ─────────────────────────────────────────────────────

    #[test]
    fn test_rdf_load_ntriples() -> Result<()> {
        let conn = open_with_extension()?;

        let nt = "<http://example.org/dave> <http://xmlns.com/foaf/0.1/name> \"Dave\" .\n";

        let loaded: i64 = conn.query_row(
            "SELECT rdf_load_ntriples(?)",
            rusqlite::params![nt],
            |r| r.get(0),
        )?;
        assert_eq!(loaded, 1);
        Ok(())
    }

    // ── rdf_dump_ntriples ─────────────────────────────────────────────────────

    #[test]
    fn test_rdf_dump_ntriples() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "SELECT rdf_insert(
               'http://example.org/eve',
               'http://xmlns.com/foaf/0.1/name',
               '\"Eve\"'
             );",
        )?;

        let dump: String = conn.query_row("SELECT rdf_dump_ntriples()", [], |r| r.get(0))?;
        assert!(dump.contains("http://example.org/eve"), "Dump should contain the subject IRI");
        assert!(dump.contains("Eve"), "Dump should contain the literal value");
        Ok(())
    }

    // ── rdf_term_type / rdf_term_value ────────────────────────────────────────

    #[test]
    fn test_term_helpers() -> Result<()> {
        let conn = open_with_extension()?;

        let iri_type: String = conn.query_row(
            "SELECT rdf_term_type('<http://example.org/foo>')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(iri_type, "iri");

        let blank_type: String =
            conn.query_row("SELECT rdf_term_type('_:b0')", [], |r| r.get(0))?;
        assert_eq!(blank_type, "blank");

        let lit_type: String = conn.query_row(
            "SELECT rdf_term_type('\"hello\"')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(lit_type, "literal");

        let iri_val: String = conn.query_row(
            "SELECT rdf_term_value('<http://example.org/foo>')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(iri_val, "http://example.org/foo");

        let lit_val: String = conn.query_row(
            "SELECT rdf_term_value('\"hello\"@en')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(lit_val, "hello");

        Ok(())
    }

    // ── sparql_query (SELECT) ─────────────────────────────────────────────────

    #[test]
    fn test_sparql_select() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "SELECT rdf_insert(
               'http://example.org/frank',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
               'http://xmlns.com/foaf/0.1/Person'
             );",
        )?;

        let json: String = conn.query_row(
            "SELECT sparql_query('SELECT ?s WHERE { ?s a <http://xmlns.com/foaf/0.1/Person> }')",
            [],
            |r| r.get(0),
        )?;

        assert!(json.contains("frank"), "Result JSON should contain the subject");
        Ok(())
    }

    // ── sparql_ask ────────────────────────────────────────────────────────────

    #[test]
    fn test_sparql_ask() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "SELECT rdf_insert(
               'http://example.org/grace',
               'http://xmlns.com/foaf/0.1/name',
               '\"Grace\"'
             );",
        )?;

        let yes: i64 = conn.query_row(
            "SELECT sparql_ask('ASK { <http://example.org/grace> <http://xmlns.com/foaf/0.1/name> ?n }')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(yes, 1);

        let no: i64 = conn.query_row(
            "SELECT sparql_ask('ASK { <http://example.org/nobody> ?p ?o }')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(no, 0);

        Ok(())
    }

    // ── sparql_construct ──────────────────────────────────────────────────────

    #[test]
    fn test_sparql_construct() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "SELECT rdf_insert(
               'http://example.org/henry',
               'http://xmlns.com/foaf/0.1/name',
               '\"Henry\"'
             );",
        )?;

        let nt: String = conn.query_row(
            "SELECT sparql_construct('CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }')",
            [],
            |r| r.get(0),
        )?;

        assert!(nt.contains("henry"), "Constructed N-Triples should contain the subject");
        Ok(())
    }

    // ── rdf_triples virtual table ─────────────────────────────────────────────

    #[test]
    fn test_virtual_table() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE triples USING rdf_triples();
             INSERT INTO triples VALUES (
               'http://example.org/iris',
               'http://xmlns.com/foaf/0.1/name',
               '\"Iris\"'
             );",
        )?;

        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM triples", [], |r| r.get(0))?;
        assert_eq!(count, 1);

        let subj: String = conn.query_row(
            "SELECT subject FROM triples LIMIT 1",
            [],
            |r| r.get(0),
        )?;
        assert!(subj.contains("iris"));

        Ok(())
    }

    // ── thread-local isolation ────────────────────────────────────────────────
    //
    // store.rs holds one Oxigraph store per thread. Two SQLite connections
    // opened on two different threads must therefore see two independent
    // triple stores. This locks in the design choice so a future refactor
    // doesn't silently turn the store into a process-global singleton.

    #[test]
    fn test_thread_local_isolation() -> Result<()> {
        use std::sync::mpsc;
        use std::thread;

        let (tx_a, rx) = mpsc::channel::<i64>();
        let tx_b = tx_a.clone();

        let ta = thread::spawn(move || {
            let conn = open_with_extension().expect("thread A open");
            conn.execute_batch(
                "SELECT rdf_insert('http://t.a/s','http://t.a/p','http://t.a/o');",
            )
            .expect("thread A insert");
            let n: i64 = conn
                .query_row("SELECT rdf_count()", [], |r| r.get(0))
                .expect("thread A count");
            tx_a.send(n).unwrap();
        });

        let tb = thread::spawn(move || {
            let conn = open_with_extension().expect("thread B open");
            // Thread B does no inserts — should see an empty store.
            let n: i64 = conn
                .query_row("SELECT rdf_count()", [], |r| r.get(0))
                .expect("thread B count");
            tx_b.send(n).unwrap();
        });

        ta.join().unwrap();
        tb.join().unwrap();

        let mut counts: Vec<i64> = vec![rx.recv().unwrap(), rx.recv().unwrap()];
        counts.sort_unstable();
        assert_eq!(
            counts,
            vec![0, 1],
            "Each thread must see its own store: one with 1 triple, one with 0"
        );
        Ok(())
    }
}
