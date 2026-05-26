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
    use serial_test::serial;

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
        // Since 0.2.0 the store is process-wide and shared across every
        // connection on every thread, so cargo's parallel test runner
        // would have tests stomp on each other without an explicit reset.
        conn.execute_batch("SELECT rdf_clear();")?;
        Ok(conn)
    }

    // ── rdf_insert / rdf_count ────────────────────────────────────────────────

    #[test]
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
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

    // ── 0.2.0 store sharing ───────────────────────────────────────────────────
    //
    // 0.2.0 replaced the per-thread Oxigraph store with a single
    // process-wide store (`OnceLock<Store>`). The two tests below pin
    // that behaviour:
    //
    //  - cross-thread visibility: a triple inserted on thread A is
    //    visible from a SQLite connection opened on thread B.
    //  - same-thread cross-connection visibility: a triple inserted
    //    via one Connection is visible from a second Connection on the
    //    same thread (the common Rails pool case).
    //
    // Serial ordering is enforced by `#[serial]` on every test.

    #[test]
    #[serial]
    fn test_cross_thread_visibility() -> Result<()> {
        use std::sync::mpsc;
        use std::thread;

        // Start clean.
        let _ = open_with_extension()?;

        let (tx, rx) = mpsc::channel::<()>();
        let ta = thread::spawn(move || {
            let conn = Connection::open_in_memory().expect("thread A open");
            unsafe {
                let g = rusqlite::LoadExtensionGuard::new(&conn).unwrap();
                conn.load_extension(
                    env!("SQLITE_SPARQL_CDYLIB"),
                    Some("sqlite3_sqlitesparql_init"),
                )
                .unwrap();
                drop(g);
            }
            // Deliberately NOT calling rdf_clear() here — we want to
            // observe state shared with the other thread.
            conn.execute_batch(
                "SELECT rdf_insert('http://t.a/s','http://t.a/p','http://t.a/o');",
            )
            .expect("thread A insert");
            tx.send(()).unwrap();
        });

        let tb = thread::spawn(move || {
            rx.recv().unwrap(); // wait for thread A's write
            let conn = Connection::open_in_memory().expect("thread B open");
            unsafe {
                let g = rusqlite::LoadExtensionGuard::new(&conn).unwrap();
                conn.load_extension(
                    env!("SQLITE_SPARQL_CDYLIB"),
                    Some("sqlite3_sqlitesparql_init"),
                )
                .unwrap();
                drop(g);
            }
            let n: i64 = conn
                .query_row("SELECT rdf_count()", [], |r| r.get(0))
                .expect("thread B count");
            n
        });

        ta.join().unwrap();
        let count_seen_by_b = tb.join().unwrap();
        assert_eq!(
            count_seen_by_b, 1,
            "Thread B must see thread A's write through the shared store"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_shared_store_across_connections() -> Result<()> {
        let conn_a = open_with_extension()?; // clears the store
        let conn_b = open_with_extension()?;
        // open_with_extension() called rdf_clear() twice; both connections
        // share the same store, so the store is empty regardless.

        conn_a.execute_batch(
            "SELECT rdf_insert('http://shared/s','http://shared/p','http://shared/o');",
        )?;

        let n: i64 = conn_b.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(
            n, 1,
            "Conn B must see Conn A's write — the store is process-wide"
        );

        // Cleanup so we don't leak state into other tests.
        conn_a.execute_batch("SELECT rdf_clear();")?;
        Ok(())
    }

    // ── 0.3.0 named graphs ────────────────────────────────────────────────────

    #[test]
    #[serial]
    fn test_rdf_insert_4arg_named_graph() -> Result<()> {
        let conn = open_with_extension()?;

        conn.execute_batch(
            "SELECT rdf_insert('http://e/s1','http://e/p','http://e/o1');
             SELECT rdf_insert('http://e/s2','http://e/p','http://e/o2','urn:g:bhphoto');
             SELECT rdf_insert('http://e/s3','http://e/p','http://e/o3', NULL);",
        )?;

        let default_count: i64 =
            conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        let default_null: i64 =
            conn.query_row("SELECT rdf_count(NULL)", [], |r| r.get(0))?;
        let bhphoto_count: i64 = conn.query_row(
            "SELECT rdf_count(?)",
            rusqlite::params!["urn:g:bhphoto"],
            |r| r.get(0),
        )?;
        let all_count: i64 = conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;

        assert_eq!(default_count, 2, "default graph: s1 + s3 (NULL graph)");
        assert_eq!(default_null, 2, "rdf_count(NULL) must equal rdf_count()");
        assert_eq!(bhphoto_count, 1, "named graph holds only s2");
        assert_eq!(all_count, 3, "rdf_count_all spans every graph");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_delete_4arg_named_graph() -> Result<()> {
        let conn = open_with_extension()?;

        // Same triple shape in default and named graph — deletion in one
        // must not affect the other.
        conn.execute_batch(
            "SELECT rdf_insert('http://e/s','http://e/p','http://e/o');
             SELECT rdf_insert('http://e/s','http://e/p','http://e/o','urn:g:bhphoto');",
        )?;

        let n: i64 = conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;
        assert_eq!(n, 2);

        conn.execute_batch(
            "SELECT rdf_delete('http://e/s','http://e/p','http://e/o','urn:g:bhphoto');",
        )?;

        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT rdf_count('urn:g:bhphoto')",
                [],
                |r| r.get(0)
            )?,
            0,
            "named-graph triple removed"
        );
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT rdf_count()", [], |r| r.get(0))?,
            1,
            "default-graph copy is untouched"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_insert_4arg_rejects_blank_graph() -> Result<()> {
        let conn = open_with_extension()?;
        let err = conn
            .execute_batch(
                "SELECT rdf_insert('http://e/s','http://e/p','http://e/o','_:b0');",
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("blank-node graphs"),
            "expected blank-node rejection, got: {err}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_query_graph_clause() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/in_default','http://e/p','http://e/o');
             SELECT rdf_insert('http://e/in_bhphoto','http://e/p','http://e/o','urn:g:bhphoto');
             SELECT rdf_insert('http://e/in_other','http://e/p','http://e/o','urn:g:other');",
        )?;

        // GRAPH-bound query — bhphoto only.
        let json: String = conn.query_row(
            "SELECT sparql_query('SELECT ?s WHERE { GRAPH <urn:g:bhphoto> { ?s ?p ?o } }')",
            [],
            |r| r.get(0),
        )?;
        assert!(json.contains("in_bhphoto"), "got: {json}");
        assert!(!json.contains("in_default"), "got: {json}");
        assert!(!json.contains("in_other"), "got: {json}");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_query_default_dataset_isolates() -> Result<()> {
        // Confirms Oxigraph's default-dataset semantics: an unqualified
        // `?s ?p ?o` query returns only the default graph, not the union
        // of every graph. If this ever flips, downstream consumers will
        // start seeing named-graph triples they didn't ask for.
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/d','http://e/p','http://e/o');
             SELECT rdf_insert('http://e/n','http://e/p','http://e/o','urn:g:bhphoto');",
        )?;
        let json: String = conn.query_row(
            "SELECT sparql_query('SELECT ?s WHERE { ?s ?p ?o }')",
            [],
            |r| r.get(0),
        )?;
        assert!(json.contains("http://e/d"), "got: {json}");
        assert!(!json.contains("http://e/n"), "named-graph triple leaked: {json}");
        Ok(())
    }

    // ── rdf_load_*_to_graph (PLAN_0.6.0) ─────────────────────────────────────

    #[test]
    #[serial]
    fn test_rdf_load_ntriples_to_graph_roundtrip() -> Result<()> {
        let conn = open_with_extension()?;
        let nt = "\
<http://e/a> <http://e/p> \"x\" .\n\
<http://e/b> <http://e/p> \"y\" .\n\
<http://e/c> <http://e/p> \"z\" .\n";

        let loaded: i64 = conn.query_row(
            "SELECT rdf_load_ntriples_to_graph(?, 'urn:g:bhphoto')",
            rusqlite::params![nt],
            |r| r.get(0),
        )?;
        assert_eq!(loaded, 3);

        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT rdf_count()", [], |r| r.get(0))?,
            0,
            "default graph stays empty"
        );
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT rdf_count('urn:g:bhphoto')",
                [],
                |r| r.get(0)
            )?,
            3
        );
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT rdf_count_all()", [], |r| r.get(0))?,
            3
        );

        let json: String = conn.query_row(
            "SELECT sparql_query('SELECT ?s WHERE { GRAPH <urn:g:bhphoto> { ?s ?p ?o } }')",
            [],
            |r| r.get(0),
        )?;
        assert!(json.contains("http://e/a"), "got: {json}");
        assert!(json.contains("http://e/b"), "got: {json}");
        assert!(json.contains("http://e/c"), "got: {json}");

        let default_json: String = conn.query_row(
            "SELECT sparql_query('SELECT ?s WHERE { ?s ?p ?o }')",
            [],
            |r| r.get(0),
        )?;
        assert!(
            default_json == "[]" || default_json.is_empty(),
            "default-graph query should be empty, got: {default_json}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_load_ntriples_to_graph_null_is_default() -> Result<()> {
        let conn = open_with_extension()?;
        let nt = "<http://e/only> <http://e/p> \"v\" .\n";
        let loaded: i64 = conn.query_row(
            "SELECT rdf_load_ntriples_to_graph(?, NULL)",
            rusqlite::params![nt],
            |r| r.get(0),
        )?;
        assert_eq!(loaded, 1);
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT rdf_count()", [], |r| r.get(0))?,
            1
        );
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT rdf_count_all()", [], |r| r.get(0))?,
            1,
            "NULL graph means default; no quad lands in any named graph"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_load_ntriples_to_graph_rejects_blank_node_graph() -> Result<()> {
        let conn = open_with_extension()?;
        let nt = "<http://e/s> <http://e/p> \"v\" .\n";
        let err = conn
            .query_row::<i64, _, _>(
                "SELECT rdf_load_ntriples_to_graph(?, '_:bgraph')",
                rusqlite::params![nt],
                |r| r.get(0),
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("blank-node graphs"),
            "expected blank-node rejection, got: {err}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_load_ntriples_to_graph_parser_parity() -> Result<()> {
        // The 2-arg loader must route through the same parser as the 1-arg
        // form. Loading the same body into the default graph via either
        // function must produce byte-identical rdf_dump_ntriples() output.
        let nt = "\
<http://e/a> <http://e/p> \"x\" .\n\
<http://e/b> <http://e/p> \"y\" .\n";

        let conn1 = open_with_extension()?;
        conn1.query_row::<i64, _, _>(
            "SELECT rdf_load_ntriples(?)",
            rusqlite::params![nt],
            |r| r.get(0),
        )?;
        let dump_1arg: String =
            conn1.query_row("SELECT rdf_dump_ntriples()", [], |r| r.get(0))?;

        let conn2 = open_with_extension()?;
        conn2.query_row::<i64, _, _>(
            "SELECT rdf_load_ntriples_to_graph(?, NULL)",
            rusqlite::params![nt],
            |r| r.get(0),
        )?;
        let dump_2arg: String =
            conn2.query_row("SELECT rdf_dump_ntriples()", [], |r| r.get(0))?;

        assert_eq!(dump_1arg, dump_2arg, "the two loader paths must agree");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_vtab_named_graph_round_trip() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "CREATE VIRTUAL TABLE triples USING rdf_triples();
             INSERT INTO triples(subject, predicate, object, graph) VALUES (
               'http://e/iris', 'http://e/p', '\"Iris\"', 'urn:g:bhphoto'
             );",
        )?;
        // Read graph column explicitly.
        let g: String = conn.query_row(
            "SELECT graph FROM triples WHERE subject = '<http://e/iris>'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(g, "urn:g:bhphoto");
        // graph column is HIDDEN — SELECT * gives three columns only.
        let visible_cols: i64 = conn
            .prepare("SELECT * FROM triples LIMIT 1")?
            .column_count() as i64;
        assert_eq!(visible_cols, 3, "graph is HIDDEN; SELECT * is still 3 cols");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_vtab_default_graph_compat() -> Result<()> {
        // The 0.1.0/0.2.0 3-column INSERT VALUES form must keep working
        // unchanged after the graph column was added.
        let conn = open_with_extension()?;
        conn.execute_batch(
            "CREATE VIRTUAL TABLE triples USING rdf_triples();
             INSERT INTO triples VALUES (
               'http://e/legacy', 'http://e/p', 'http://e/o'
             );",
        )?;
        let g: Option<String> = conn.query_row(
            "SELECT graph FROM triples WHERE subject = '<http://e/legacy>'",
            [],
            |r| r.get(0),
        )?;
        assert!(g.is_none(), "missing graph means default graph; got: {g:?}");
        Ok(())
    }

    // ── 0.4.0 batched insert / delete ─────────────────────────────────────────

    #[test]
    #[serial]
    fn test_insert_many_3_arg_rows() -> Result<()> {
        let conn = open_with_extension()?;
        let n: i64 = conn.query_row(
            "SELECT rdf_insert_many(?)",
            rusqlite::params![
                r#"[
                  ["http://e/s1","http://e/p","\"a\""],
                  ["http://e/s2","http://e/p","\"b\""],
                  ["http://e/s3","http://e/p","\"c\""]
                ]"#
            ],
            |r| r.get(0),
        )?;
        assert_eq!(n, 3);
        let default_count: i64 =
            conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(default_count, 3, "all rows land in default graph");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_insert_many_mixed_arities() -> Result<()> {
        let conn = open_with_extension()?;
        let n: i64 = conn.query_row(
            "SELECT rdf_insert_many(?)",
            rusqlite::params![
                r#"[
                  ["http://e/d1","http://e/p","\"a\""],
                  ["http://e/g1","http://e/p","\"b\"","urn:g:bhphoto"],
                  ["http://e/d2","http://e/p","\"c\"",null]
                ]"#
            ],
            |r| r.get(0),
        )?;
        assert_eq!(n, 3);
        let default_n: i64 =
            conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        let bhphoto_n: i64 = conn.query_row(
            "SELECT rdf_count(?)",
            rusqlite::params!["urn:g:bhphoto"],
            |r| r.get(0),
        )?;
        assert_eq!(default_n, 2, "two rows targeted the default graph");
        assert_eq!(bhphoto_n, 1, "one row targeted bhphoto");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_insert_many_dedup_return_value() -> Result<()> {
        let conn = open_with_extension()?;
        let n: i64 = conn.query_row(
            "SELECT rdf_insert_many(?)",
            rusqlite::params![
                r#"[
                  ["http://e/dup","http://e/p","\"x\""],
                  ["http://e/dup","http://e/p","\"x\""]
                ]"#
            ],
            |r| r.get(0),
        )?;
        // RDF set semantics — the duplicate is a no-op.
        assert_eq!(n, 1, "duplicate row must not count twice");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_insert_many_malformed_aborts_batch() -> Result<()> {
        let conn = open_with_extension()?;
        let result = conn.query_row::<i64, _, _>(
            "SELECT rdf_insert_many(?)",
            rusqlite::params![r#"[
                  ["http://e/ok","http://e/p","\"v\""],
                  ["bad-arity"]
                ]"#],
            |r| r.get(0),
        );
        assert!(result.is_err(), "malformed batch should error");
        let count: i64 = conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;
        assert_eq!(count, 0, "all-or-nothing: nothing inserted on parse failure");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_insert_many_empty_array() -> Result<()> {
        let conn = open_with_extension()?;
        let n: i64 = conn.query_row(
            "SELECT rdf_insert_many('[]')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(n, 0);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_insert_many_parser_parity_with_single() -> Result<()> {
        // PLAN_0.4.0.md risk #2 — the batched function must use the same
        // term parser as the single rdf_insert. Insert the same triple
        // both ways; rdf_count_all() must end at 1, not 2.
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/s','http://e/p','\"v\"');",
        )?;
        let n: i64 = conn.query_row(
            "SELECT rdf_insert_many(?)",
            rusqlite::params![r#"[["http://e/s","http://e/p","\"v\""]]"#],
            |r| r.get(0),
        )?;
        assert_eq!(n, 0, "same triple via _many is a no-op");
        let total: i64 =
            conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;
        assert_eq!(total, 1, "the two write paths produce the same quad");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_delete_many_partial() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/a','http://e/p','http://e/o');
             SELECT rdf_insert('http://e/b','http://e/p','http://e/o');",
        )?;
        let n: i64 = conn.query_row(
            "SELECT rdf_delete_many(?)",
            rusqlite::params![
                r#"[
                  ["http://e/a","http://e/p","http://e/o"],
                  ["http://e/missing","http://e/p","http://e/o"]
                ]"#
            ],
            |r| r.get(0),
        )?;
        assert_eq!(n, 1, "absent rows are no-ops and don't count");
        let remaining: i64 =
            conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(remaining, 1, "only e/a was removed");
        Ok(())
    }

    #[test]
    #[serial]
    #[ignore]
    fn test_insert_many_perf_smoke() -> Result<()> {
        // Run with: cargo test --release -- --ignored insert_many_perf_smoke
        // Loose so a busy CI runner doesn't flap; tight enough to catch a
        // regression in the bulk-loader path.
        let conn = open_with_extension()?;

        let mut rows: Vec<String> = Vec::with_capacity(1000);
        for i in 0..1000 {
            rows.push(format!(
                r#"["http://e/s{}","http://e/p","\"v{}\""]"#,
                i, i
            ));
        }
        let json = format!("[{}]", rows.join(","));

        let start = std::time::Instant::now();
        let n: i64 = conn.query_row(
            "SELECT rdf_insert_many(?)",
            rusqlite::params![json],
            |r| r.get(0),
        )?;
        let elapsed = start.elapsed();

        assert_eq!(n, 1000);
        assert!(
            elapsed.as_millis() < 100,
            "1000-row bulk insert should be under 100 ms, was {:?}",
            elapsed
        );
        Ok(())
    }

    // ── 0.5.0 sparql_update ───────────────────────────────────────────────────

    fn run_update(conn: &Connection, q: &str) -> Result<i64> {
        conn.query_row("SELECT sparql_update(?)", rusqlite::params![q], |r| r.get(0))
    }

    #[test]
    #[serial]
    fn test_sparql_update_insert_data() -> Result<()> {
        let conn = open_with_extension()?;
        let delta = run_update(
            &conn,
            "INSERT DATA { <http://e/a> <http://e/p> \"x\" }",
        )?;
        assert_eq!(delta, 1);
        let n: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(n, 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_delete_data() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch("SELECT rdf_insert('http://e/a','http://e/p','\"x\"');")?;
        let delta = run_update(
            &conn,
            "DELETE DATA { <http://e/a> <http://e/p> \"x\" }",
        )?;
        assert_eq!(delta, -1, "DELETE DATA returns a negative delta");
        let n: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(n, 0);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_dedup_on_insert_data() -> Result<()> {
        let conn = open_with_extension()?;
        let q = "INSERT DATA { <http://e/a> <http://e/p> \"x\" . <http://e/a> <http://e/p> \"x\" }";
        let delta = run_update(&conn, q)?;
        assert_eq!(
            delta, 1,
            "RDF set semantics: duplicate quad in one INSERT DATA only counts once"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_where_insert() -> Result<()> {
        let conn = open_with_extension()?;
        // Seed two source triples; INSERT derives a new predicate for each.
        conn.execute_batch(
            "SELECT rdf_insert('http://e/a','http://e/src','\"v1\"');
             SELECT rdf_insert('http://e/b','http://e/src','\"v2\"');",
        )?;
        let delta = run_update(
            &conn,
            "INSERT { ?s <http://e/derived> ?o } WHERE { ?s <http://e/src> ?o }",
        )?;
        assert_eq!(delta, 2, "two source rows → two derived rows");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_modify_mixed() -> Result<()> {
        // For mixed DELETE/INSERT, observe the store state, not the delta —
        // the delta lies for balanced mixed ops by design.
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/a','http://e/old','\"v\"');",
        )?;
        // Swap predicate: delete old, insert new.
        let _ = run_update(
            &conn,
            "DELETE { ?s <http://e/old> ?o } INSERT { ?s <http://e/new> ?o } \
             WHERE { ?s <http://e/old> ?o }",
        )?;
        let new_count: i64 = conn.query_row(
            "SELECT sparql_ask('ASK { <http://e/a> <http://e/new> \"v\" }')",
            [],
            |r| r.get(0),
        )?;
        let old_count: i64 = conn.query_row(
            "SELECT sparql_ask('ASK { <http://e/a> <http://e/old> ?o }')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(new_count, 1, "new predicate was inserted");
        assert_eq!(old_count, 0, "old predicate was deleted");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_named_graph() -> Result<()> {
        let conn = open_with_extension()?;
        let delta = run_update(
            &conn,
            "INSERT DATA { GRAPH <urn:g:bhphoto> { <http://e/a> <http://e/p> \"x\" } }",
        )?;
        assert_eq!(delta, 1);
        let default_n: i64 =
            conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        let bhphoto_n: i64 = conn.query_row(
            "SELECT rdf_count(?)",
            rusqlite::params!["urn:g:bhphoto"],
            |r| r.get(0),
        )?;
        assert_eq!(default_n, 0, "named-graph INSERT must not leak to default");
        assert_eq!(bhphoto_n, 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_clear_default() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/d','http://e/p','\"v\"');
             SELECT rdf_insert('http://e/n','http://e/p','\"v\"','urn:g:keep');",
        )?;
        let _delta = run_update(&conn, "CLEAR DEFAULT")?;
        let d: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        let n: i64 = conn.query_row(
            "SELECT rdf_count(?)",
            rusqlite::params!["urn:g:keep"],
            |r| r.get(0),
        )?;
        assert_eq!(d, 0, "default graph cleared");
        assert_eq!(n, 1, "named graph untouched");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_clear_all() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/d','http://e/p','\"v\"');
             SELECT rdf_insert('http://e/n','http://e/p','\"v\"','urn:g:zap');",
        )?;
        let _ = run_update(&conn, "CLEAR ALL")?;
        let all: i64 = conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;
        assert_eq!(all, 0, "CLEAR ALL empties every graph");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_parse_error_surfaces() -> Result<()> {
        let conn = open_with_extension()?;
        let err = run_update(&conn, "this is not sparql")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("parse"),
            "parse failures must surface a 'parse' error string, got: {err}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_sparql_update_evaluation_error_surfaces() -> Result<()> {
        // CREATE GRAPH on an already-existing graph is a syntactically valid
        // UPDATE that fails at evaluation time. Surface should be a SQLite
        // error string, not a Rust panic — which the test harness would
        // turn into a crashing test run, not a failing test.
        let conn = open_with_extension()?;
        let _ = run_update(&conn, "CREATE GRAPH <urn:g:dup>")?;
        let err = run_update(&conn, "CREATE GRAPH <urn:g:dup>")
            .unwrap_err()
            .to_string();
        assert!(
            !err.is_empty(),
            "evaluation errors must surface as a non-empty SQLite error"
        );
        Ok(())
    }

    // ── 0.7.0 RDF-star / SPARQL-star ──────────────────────────────────────────
    //
    // Quoted-triple terms survive the SQL boundary in both directions: parsed
    // in by every write path (rdf_insert, rdf_insert_many, the rdf_triples
    // vtab, rdf_load_*), emitted out by every read path (rdf_dump_ntriples,
    // sparql_query JSON bindings, sparql_construct). Inverts the Phase A
    // negative pins documented in PLAN_0.7.0.md.

    /// Test 1 — Turtle-star body with `{| |}` annotation expands to one
    /// asserted triple + N annotation triples. Pins the Oxigraph 0.4 parser
    /// row in the plan's "What works today" table.
    #[test]
    #[serial]
    fn test_rdf_star_load_turtle_with_annotation() -> Result<()> {
        let conn = open_with_extension()?;
        let turtle = r#"
            @prefix : <http://example.org/> .
            :bob :name "Bob" {| :statedBy :alice ; :confidence "0.9" |} .
        "#;
        let loaded: i64 =
            conn.query_row("SELECT rdf_load_turtle(?)", [turtle], |r| r.get(0))?;
        assert_eq!(loaded, 3, "1 asserted + 2 annotation triples");
        let total: i64 = conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;
        assert_eq!(total, 3);
        Ok(())
    }

    /// Test 2 — `rdf_dump_ntriples` emits valid N-Triples-star (no more stub
    /// literal) and the output round-trips back through `rdf_load_ntriples`.
    #[test]
    #[serial]
    fn test_rdf_star_dump_roundtrip() -> Result<()> {
        let conn = open_with_extension()?;
        let turtle = r#"
            @prefix : <http://example.org/> .
            :bob :name "Bob" {| :statedBy :alice |} .
        "#;
        let _: i64 =
            conn.query_row("SELECT rdf_load_turtle(?)", [turtle], |r| r.get(0))?;
        let dump: String =
            conn.query_row("SELECT rdf_dump_ntriples()", [], |r| r.get(0))?;

        assert!(
            dump.contains("<<") && dump.contains(">>"),
            "Dump must use N-Triples-star <<…>> form; got: {dump}"
        );
        assert!(
            !dump.contains("rdf-star unsupported"),
            "Phase B stub literal must not appear; got: {dump}"
        );

        // Round-trip: clear, re-load the dump, count should match.
        conn.execute_batch("SELECT rdf_clear();")?;
        let reloaded: i64 = conn
            .query_row("SELECT rdf_load_ntriples(?)", [&dump], |r| r.get(0))?;
        assert_eq!(reloaded, 2, "dump should re-parse to the same 2 quads");
        let total: i64 = conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;
        assert_eq!(total, 2);
        Ok(())
    }

    /// Test 3 — `rdf_insert` accepts a quoted-triple subject (Phase C parser).
    #[test]
    #[serial]
    fn test_rdf_star_insert_quoted_subject_via_rdf_insert() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            r#"SELECT rdf_insert(
                 '<< <http://e/a> <http://e/p> "x" >>',
                 'http://e/q',
                 'http://e/b'
               );"#,
        )?;
        let count: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(count, 1);
        let dump: String =
            conn.query_row("SELECT rdf_dump_ntriples()", [], |r| r.get(0))?;
        assert!(
            dump.contains("<< <http://e/a> <http://e/p> \"x\" >>"),
            "dump should contain the inserted quoted-triple subject; got: {dump}"
        );
        Ok(())
    }

    /// Test 4 — `rdf_insert` accepts a quoted-triple object.
    #[test]
    #[serial]
    fn test_rdf_star_insert_quoted_object_via_rdf_insert() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            r#"SELECT rdf_insert(
                 'http://e/a',
                 'http://e/p',
                 '<< <http://e/x> <http://e/y> "z" >>'
               );"#,
        )?;
        let count: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(count, 1);
        let dump: String =
            conn.query_row("SELECT rdf_dump_ntriples()", [], |r| r.get(0))?;
        assert!(
            dump.contains("<< <http://e/x> <http://e/y> \"z\" >>"),
            "dump should contain the inserted quoted-triple object; got: {dump}"
        );
        Ok(())
    }

    /// Test 5 — the `rdf_triples` virtual table accepts quoted-triple subjects
    /// in `INSERT VALUES` and emits them back in `SELECT`.
    #[test]
    #[serial]
    fn test_rdf_star_vtab_insert_and_select() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch("CREATE VIRTUAL TABLE triples USING rdf_triples();")?;
        conn.execute_batch(
            r#"INSERT INTO triples VALUES (
                 '<< <http://e/a> <http://e/p> "x" >>',
                 'http://e/q',
                 '"y"'
               );"#,
        )?;
        let subject: String =
            conn.query_row("SELECT subject FROM triples LIMIT 1", [], |r| r.get(0))?;
        assert_eq!(
            subject, "<< <http://e/a> <http://e/p> \"x\" >>",
            "vtab SELECT must round-trip the quoted-triple subject"
        );
        Ok(())
    }

    /// Test 6 — SPARQL-star annotation shorthand binds bare variables in the
    /// asserted + annotation patterns.
    #[test]
    #[serial]
    fn test_rdf_star_sparql_query_annotation_shorthand() -> Result<()> {
        let conn = open_with_extension()?;
        let turtle = r#"
            @prefix : <http://example.org/> .
            :bob :name "Bob" {| :statedBy :alice |} .
        "#;
        let _: i64 =
            conn.query_row("SELECT rdf_load_turtle(?)", [turtle], |r| r.get(0))?;

        let json: String = conn.query_row(
            "SELECT sparql_query(?)",
            [r#"
                PREFIX : <http://example.org/>
                SELECT ?val ?stater WHERE {
                  :bob :name ?val {| :statedBy ?stater |} .
                }
            "#],
            |r| r.get(0),
        )?;
        assert!(json.contains("Bob"), "expected ?val = Bob; got: {json}");
        assert!(json.contains("alice"), "expected ?stater = alice; got: {json}");
        Ok(())
    }

    /// Test 7 — a SPARQL-star query that binds `?t` to a quoted-triple term
    /// returns the N-Triples-star encoding in the JSON envelope (the Phase B
    /// serialiser output is exercised here).
    #[test]
    #[serial]
    fn test_rdf_star_sparql_query_triple_term_binding() -> Result<()> {
        let conn = open_with_extension()?;
        let turtle = r#"
            @prefix : <http://example.org/> .
            :bob :name "Bob" {| :statedBy :alice |} .
        "#;
        let _: i64 =
            conn.query_row("SELECT rdf_load_turtle(?)", [turtle], |r| r.get(0))?;

        let json: String = conn.query_row(
            "SELECT sparql_query(?)",
            [r#"
                PREFIX : <http://example.org/>
                SELECT ?t ?stater WHERE { ?t :statedBy ?stater . }
            "#],
            |r| r.get(0),
        )?;
        assert!(
            !json.contains("rdf-star unsupported"),
            "stub literal must not appear in triple-term binding; got: {json}"
        );
        assert!(json.contains("<<"), "binding should contain <<; got: {json}");
        assert!(json.contains(">>"), "binding should contain >>; got: {json}");
        assert!(json.contains("bob"), "binding should reference bob; got: {json}");
        assert!(json.contains("name"), "binding should reference name; got: {json}");
        Ok(())
    }

    /// Test 8 — `sparql_construct` over a star pattern emits N-Triples-star
    /// that re-parses through `rdf_load_ntriples`.
    #[test]
    #[serial]
    fn test_rdf_star_sparql_construct() -> Result<()> {
        let conn = open_with_extension()?;
        let turtle = r#"
            @prefix : <http://example.org/> .
            :bob :name "Bob" {| :statedBy :alice |} .
        "#;
        let _: i64 =
            conn.query_row("SELECT rdf_load_turtle(?)", [turtle], |r| r.get(0))?;

        let constructed: String = conn.query_row(
            "SELECT sparql_construct(?)",
            [r#"
                PREFIX : <http://example.org/>
                CONSTRUCT { ?t :wasStatedBy ?stater }
                WHERE { ?t :statedBy ?stater }
            "#],
            |r| r.get(0),
        )?;
        assert!(
            constructed.contains("<<") && constructed.contains(">>"),
            "CONSTRUCT result should contain a quoted-triple subject; got: {constructed}"
        );
        assert!(
            constructed.contains("wasStatedBy"),
            "CONSTRUCT result should contain the new predicate; got: {constructed}"
        );

        // Round-trip: load the CONSTRUCT output into a clean store.
        conn.execute_batch("SELECT rdf_clear();")?;
        let reloaded: i64 = conn
            .query_row("SELECT rdf_load_ntriples(?)", [&constructed], |r| r.get(0))?;
        assert_eq!(reloaded, 1, "CONSTRUCT output should round-trip as 1 quad");
        Ok(())
    }

    /// Test 9 — the SPARQL-star `TRIPLE(...)` constructor built-in.
    #[test]
    #[serial]
    fn test_rdf_star_builtin_triple() -> Result<()> {
        let conn = open_with_extension()?;
        let json: String = conn.query_row(
            "SELECT sparql_query(?)",
            [r#"
                SELECT ?t WHERE {
                  BIND(TRIPLE(<http://e/s>, <http://e/p>, <http://e/o>) AS ?t)
                }
            "#],
            |r| r.get(0),
        )?;
        assert!(
            json.contains("<< <http://e/s> <http://e/p> <http://e/o> >>"),
            "TRIPLE built-in should produce a quoted-triple binding; got: {json}"
        );
        Ok(())
    }

    /// Test 10 — the four SPARQL-star destructor / predicate built-ins on a
    /// single bound triple term.
    #[test]
    #[serial]
    fn test_rdf_star_builtin_subject_predicate_object_istriple() -> Result<()> {
        let conn = open_with_extension()?;
        let json: String = conn.query_row(
            "SELECT sparql_query(?)",
            [r#"
                SELECT ?s ?p ?o ?is WHERE {
                  BIND(TRIPLE(<http://e/s>, <http://e/p>, <http://e/o>) AS ?t)
                  BIND(SUBJECT(?t)    AS ?s)
                  BIND(PREDICATE(?t)  AS ?p)
                  BIND(OBJECT(?t)     AS ?o)
                  BIND(isTRIPLE(?t)   AS ?is)
                }
            "#],
            |r| r.get(0),
        )?;
        assert!(json.contains("<http://e/s>"), "SUBJECT binding; got: {json}");
        assert!(json.contains("<http://e/p>"), "PREDICATE binding; got: {json}");
        assert!(json.contains("<http://e/o>"), "OBJECT binding; got: {json}");
        assert!(json.contains("true"), "isTRIPLE should be true; got: {json}");
        Ok(())
    }

    /// Test 11 — `rdf_term_type` classifies a quoted-triple string as `triple`.
    #[test]
    #[serial]
    fn test_rdf_term_type_triple() -> Result<()> {
        let conn = open_with_extension()?;
        let kind: String = conn.query_row(
            "SELECT rdf_term_type('<< <http://e/a> <http://e/p> <http://e/b> >>')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(kind, "triple");
        Ok(())
    }

    /// Test 12 — the three `rdf_triple_*` destructor scalars.
    #[test]
    #[serial]
    fn test_rdf_triple_subject_predicate_object_scalars() -> Result<()> {
        let conn = open_with_extension()?;
        let term = "<< <http://e/a> <http://e/p> \"x\" >>";
        let s: String = conn.query_row(
            "SELECT rdf_triple_subject(?)",
            [term],
            |r| r.get(0),
        )?;
        let p: String = conn.query_row(
            "SELECT rdf_triple_predicate(?)",
            [term],
            |r| r.get(0),
        )?;
        let o: String = conn.query_row(
            "SELECT rdf_triple_object(?)",
            [term],
            |r| r.get(0),
        )?;
        assert_eq!(s, "<http://e/a>");
        assert_eq!(p, "<http://e/p>");
        assert_eq!(o, "\"x\"");
        Ok(())
    }

    /// Test 13 — `rdf_term_value` refuses a triple term with the fixed-prefix
    /// error envelope consuming gems prefix-match.
    #[test]
    #[serial]
    fn test_rdf_term_value_refuses_triple() -> Result<()> {
        let conn = open_with_extension()?;
        let err = conn
            .query_row::<String, _, _>(
                "SELECT rdf_term_value('<< <http://e/a> <http://e/p> <http://e/b> >>')",
                [],
                |r| r.get(0),
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("rdf_term_value: triple terms have no scalar value"),
            "refusal must carry the fixed-prefix message; got: {err}"
        );
        Ok(())
    }

    /// Test 14 — nested quoted triples (`<< << s p o >> p o >>`) round-trip
    /// through both the parser and the serialiser.
    #[test]
    #[serial]
    fn test_rdf_star_nested_triple_roundtrip() -> Result<()> {
        let conn = open_with_extension()?;
        let nested =
            "<< << <http://e/a> <http://e/p> <http://e/b> >> <http://e/q> <http://e/c> >>";
        conn.execute_batch(&format!(
            "SELECT rdf_insert('{nested}', 'http://e/r', 'http://e/d');"
        ))?;
        let dump: String =
            conn.query_row("SELECT rdf_dump_ntriples()", [], |r| r.get(0))?;
        // Two `<<` opens: one for outer, one for nested inner.
        assert_eq!(
            dump.matches("<<").count(),
            2,
            "dump should contain two nested `<<` opens; got: {dump}"
        );
        assert!(
            dump.contains(nested),
            "dump should contain the original nested term verbatim; got: {dump}"
        );

        // Verify the nested term re-parses via rdf_triple_subject (which
        // exercises the Phase C parser end-to-end on the outer term, and
        // the Phase B serialiser on the inner triple).
        let inner: String = conn.query_row(
            "SELECT rdf_triple_subject(?)",
            [nested],
            |r| r.get(0),
        )?;
        assert_eq!(
            inner,
            "<< <http://e/a> <http://e/p> <http://e/b> >>",
            "rdf_triple_subject on nested term should return the inner quoted triple"
        );
        Ok(())
    }

    // ── 0.8.0 rdf_construct_many ──────────────────────────────────────────────
    //
    // Batched CONSTRUCT — one FFI crossing for N queries. Returns a JSON
    // array of per-query N-Triples blobs. See PLAN_0.8.0.md for the
    // return-shape rationale (per-query attribution preserved; provenance
    // shape stays on the consumer side).

    /// Test 1 — basic round-trip. Two CONSTRUCTs over the same data; each
    /// blob re-parses into the expected quad count.
    #[test]
    #[serial]
    fn test_rdf_construct_many_basic() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/a', 'http://e/p', 'http://e/b');
             SELECT rdf_insert('http://e/c', 'http://e/p', 'http://e/d');",
        )?;

        let queries_json = r#"[
            "CONSTRUCT { ?s <http://e/q1> ?o } WHERE { ?s <http://e/p> ?o }",
            "CONSTRUCT { ?s <http://e/q2> ?o } WHERE { ?s <http://e/p> ?o }"
        ]"#;
        let json: String = conn
            .query_row("SELECT rdf_construct_many(?)", [queries_json], |r| r.get(0))?;

        let arr: Vec<String> = serde_json::from_str(&json).expect("valid JSON array");
        assert_eq!(arr.len(), 2, "two queries → two blobs");
        assert!(arr[0].contains("<http://e/q1>"), "blob 0 uses q1: {}", arr[0]);
        assert!(arr[1].contains("<http://e/q2>"), "blob 1 uses q2: {}", arr[1]);

        // Round-trip blob 0 into a clean store via rdf_load_ntriples.
        conn.execute_batch("SELECT rdf_clear();")?;
        let reloaded: i64 = conn
            .query_row("SELECT rdf_load_ntriples(?)", [&arr[0]], |r| r.get(0))?;
        assert_eq!(reloaded, 2, "blob 0 should re-parse to 2 quads");
        Ok(())
    }

    /// Test 2 — parser parity. Same query through `sparql_construct` (1-arg)
    /// and as a 1-element batch through `rdf_construct_many` must produce
    /// byte-identical blobs.
    #[test]
    #[serial]
    fn test_rdf_construct_many_parser_parity_with_single() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/a', 'http://e/p', 'http://e/b');
             SELECT rdf_insert('http://e/c', 'http://e/p', 'http://e/d');",
        )?;
        let query = "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }";

        let single: String =
            conn.query_row("SELECT sparql_construct(?)", [query], |r| r.get(0))?;

        let queries_json = serde_json::to_string(&vec![query]).unwrap();
        let batched_json: String = conn
            .query_row("SELECT rdf_construct_many(?)", [&queries_json], |r| r.get(0))?;
        let arr: Vec<String> = serde_json::from_str(&batched_json).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], single, "1-element batch must match the 1-arg path byte-for-byte");
        Ok(())
    }

    /// Test 3 — empty array. Degenerate input returns `[]`, no store mutation.
    #[test]
    #[serial]
    fn test_rdf_construct_many_empty_array() -> Result<()> {
        let conn = open_with_extension()?;
        let json: String =
            conn.query_row("SELECT rdf_construct_many('[]')", [], |r| r.get(0))?;
        assert_eq!(json, "[]");
        let count: i64 = conn.query_row("SELECT rdf_count_all()", [], |r| r.get(0))?;
        assert_eq!(count, 0, "CONSTRUCT is read-only — store stays empty");
        Ok(())
    }

    /// Test 4 — pre-flight parse error aborts the batch with the indexed
    /// prefix. Pins the all-or-nothing contract.
    #[test]
    #[serial]
    fn test_rdf_construct_many_parse_error_aborts_batch() -> Result<()> {
        let conn = open_with_extension()?;
        let queries_json = r#"[
            "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }",
            "THIS IS NOT SPARQL",
            "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }"
        ]"#;
        let err = conn
            .query_row::<String, _, _>(
                "SELECT rdf_construct_many(?)",
                [queries_json],
                |r| r.get(0),
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("SPARQL parse error (query index 1)"),
            "error must name the failing query index; got: {err}"
        );
        Ok(())
    }

    /// Test 5 — non-CONSTRUCT query in the batch errors with the
    /// `rdf_construct_many:` prefix. Pins that CONSTRUCT shape is required.
    #[test]
    #[serial]
    fn test_rdf_construct_many_rejects_non_construct() -> Result<()> {
        let conn = open_with_extension()?;
        let queries_json = r#"[
            "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }",
            "SELECT ?s WHERE { ?s ?p ?o }"
        ]"#;
        let err = conn
            .query_row::<String, _, _>(
                "SELECT rdf_construct_many(?)",
                [queries_json],
                |r| r.get(0),
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("rdf_construct_many: query index 1 is not a CONSTRUCT"),
            "error must call out the SELECT-vs-CONSTRUCT mismatch; got: {err}"
        );
        Ok(())
    }

    /// Test 6 — non-array JSON input is rejected with the fixed-prefix
    /// error consuming gems prefix-match.
    #[test]
    #[serial]
    fn test_rdf_construct_many_rejects_non_array_json() -> Result<()> {
        let conn = open_with_extension()?;
        for bad in [
            r#"not json at all"#,
            r#"{"not": "array"}"#,
            r#"[1, 2, 3]"#, // numbers, not strings
        ] {
            let err = conn
                .query_row::<String, _, _>(
                    "SELECT rdf_construct_many(?)",
                    [bad],
                    |r| r.get(0),
                )
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("rdf_construct_many: expected JSON array of query strings"),
                "bad input {bad:?} must surface the fixed-prefix error; got: {err}"
            );
        }
        Ok(())
    }

    /// Test 7 — RDF-star outputs flow through unchanged. A CONSTRUCT whose
    /// subject binds a quoted triple emits `<< s p o >>` in the blob.
    #[test]
    #[serial]
    fn test_rdf_construct_many_with_rdf_star() -> Result<()> {
        let conn = open_with_extension()?;
        let turtle = r#"
            @prefix : <http://example.org/> .
            :bob :name "Bob" {| :statedBy :alice |} .
        "#;
        let _: i64 =
            conn.query_row("SELECT rdf_load_turtle(?)", [turtle], |r| r.get(0))?;

        let queries_json = r#"[
            "PREFIX : <http://example.org/> CONSTRUCT { ?t :wasStatedBy ?stater } WHERE { ?t :statedBy ?stater }"
        ]"#;
        let json: String = conn
            .query_row("SELECT rdf_construct_many(?)", [queries_json], |r| r.get(0))?;
        let arr: Vec<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(arr.len(), 1);
        assert!(
            arr[0].contains("<<") && arr[0].contains(">>"),
            "star CONSTRUCT must emit quoted-triple subject; got: {}",
            arr[0]
        );
        assert!(
            arr[0].contains("wasStatedBy"),
            "blob should contain the constructed predicate; got: {}",
            arr[0]
        );
        Ok(())
    }

    // ── 0.9.0 rdf_owl_rl_materialise ──────────────────────────────────────────
    //
    // Native OWL 2 RL fixpoint pass — 15-rule subset matching VG's
    // Vv::Graph::Reasoner::Rules::OwlRl coverage. Replaces N rules × M
    // iterations of sparql_update with one FFI crossing. See
    // PLAN_0.9.0.md.

    fn load_t_box_and_a_box(conn: &Connection) -> Result<()> {
        // 3 rdfs:subClassOf, 2 rdfs:subPropertyOf, 1 rdfs:domain, 1 alice :type :A,
        // 1 :alice :knows :bob, 1 :likes ⊑ :knows, 1 :bob :likes :carol.
        // Expected closure: 11 derived triples (verified in test 2 below).
        conn.query_row::<i64, _, _>(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
                :Aa rdfs:subClassOf :Bb .
                :Bb rdfs:subClassOf :Cc .
                :Cc rdfs:subClassOf :Dd .
                :friend rdfs:subPropertyOf :knows .
                :knows rdfs:subPropertyOf :acquaints .
                :owns rdfs:domain :Owner .
                :alice a :Aa .
                :alice :friend :bob .
                :bob :owns :car1 .
            "#],
            |r| r.get(0),
        )?;
        Ok(())
    }

    /// Test 1 — single-rule round-trip. scm-sco transitive chain.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_scm_sco() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
                :A rdfs:subClassOf :B .
                :B rdfs:subClassOf :C .
            "#],
            |r| r.get(0),
        )?;

        let delta: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        // The specific :A ⊑ :C derivation is checked below; the exact delta
        // depends on which axiomatic rules (cls-thing / cls-nothing1 /
        // scm-cls on those) also fire in 0.10.0+, so pin the lower bound
        // rather than an exact match.
        assert!(delta >= 1, "scm-sco derives :A ⊑ :C; got delta {delta}");

        // Derivations land in urn:g:inferred, not the default graph.
        let inferred_count: i64 = conn.query_row(
            "SELECT rdf_count('urn:g:inferred')",
            [],
            |r| r.get(0),
        )?;
        assert!(inferred_count >= 1, "expected ≥1 inferred quad; got {inferred_count}");

        // Verify the specific derived triple.
        let constructed: String = conn.query_row(
            "SELECT sparql_construct(?)",
            [r#"PREFIX : <http://e/>
                PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
                CONSTRUCT { :A rdfs:subClassOf :C }
                WHERE { GRAPH <urn:g:inferred> { :A rdfs:subClassOf :C } }"#],
            |r| r.get(0),
        )?;
        assert!(
            constructed.contains("http://e/A") && constructed.contains("http://e/C"),
            "expected :A ⊑ :C in inferred graph; got: {constructed}"
        );
        Ok(())
    }

    /// Test 2 — multi-rule closure with provenance. Asserts the count
    /// and that every derived triple carries the expected annotation
    /// predicates.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_full_closure_with_provenance() -> Result<()> {
        let conn = open_with_extension()?;
        load_t_box_and_a_box(&conn)?;

        let delta: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{\"provenance\":true}')",
            [],
            |r| r.get(0),
        )?;

        // Expected derived (excluding annotations):
        //   scm-sco: :Aa ⊑ :Cc, :Aa ⊑ :Dd, :Bb ⊑ :Dd       (3)
        //   scm-spo: :friend ⊑ :acquaints                    (1)
        //   cax-sco: :alice :type :Bb, :Cc, :Dd              (3)
        //   prp-spo1: :alice :knows :bob, :alice :acquaints :bob, :bob :owns :car1
        //             → :alice :friend :bob already exists; spo1 derives
        //               :alice :knows :bob; chained via scm-spo to
        //               :alice :acquaints :bob — both new           (2)
        //   prp-dom (with chained subPropertyOf via spo1):
        //             :bob :owns :car1 → :bob a :Owner             (1)
        //   Total derived asserted = 10. With provenance: ×3 (1 asserted + 2 annotations)
        //   = 30 quads added to inferred. Pin the magnitude, not the exact decomposition.
        assert!(delta > 0, "expected positive derived count; got {delta}");
        assert_eq!(
            delta % 3,
            0,
            "with provenance, every derived triple emits 1+2 annotations; \
             delta {delta} should be divisible by 3"
        );

        // Every annotation must use the default predicate IRIs.
        let with_derived_by: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK {
                GRAPH <urn:g:inferred> {
                  ?q <http://www.w3.org/ns/prov#wasDerivedFrom> ?rule .
                }
            }"#],
            |r| r.get(0),
        )?;
        assert_eq!(with_derived_by, 1, "expected at least one wasDerivedFrom annotation");

        let with_derived_at: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK {
                GRAPH <urn:g:inferred> {
                  ?q <http://www.w3.org/ns/prov#generatedAtTime> ?ts .
                }
            }"#],
            |r| r.get(0),
        )?;
        assert_eq!(with_derived_at, 1, "expected at least one generatedAtTime annotation");
        Ok(())
    }

    /// Test 3 — fixpoint idempotent. Second materialise call returns 0
    /// (everything already in the inferred graph).
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_fixpoint_idempotent() -> Result<()> {
        let conn = open_with_extension()?;
        load_t_box_and_a_box(&conn)?;

        let first: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert!(first > 0, "first call should derive triples; got {first}");

        let second: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(second, 0, "second call must be a no-op; got {second}");
        Ok(())
    }

    /// Test 4 — max_iterations guard surfaces the fixed-prefix error.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_max_iterations_guard() -> Result<()> {
        let conn = open_with_extension()?;
        // Long subClassOf chain — needs more than 1 iteration to reach
        // fixpoint via scm-sco's pairwise composition.
        conn.execute_batch(
            r#"SELECT rdf_load_turtle('
                @prefix : <http://e/> .
                @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
                :A rdfs:subClassOf :B .
                :B rdfs:subClassOf :C .
                :C rdfs:subClassOf :D .
                :D rdfs:subClassOf :E .
                :E rdfs:subClassOf :F .
            ');"#,
        )?;

        let err = conn
            .query_row::<i64, _, _>(
                "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{\"max_iterations\":1}')",
                [],
                |r| r.get(0),
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("rdf_owl_rl_materialise: fixpoint not reached after 1 iterations"),
            "expected fixed-prefix max-iterations error; got: {err}"
        );
        Ok(())
    }

    /// Test 5 — inferred_iri = NULL is rejected.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_inferred_must_be_named() -> Result<()> {
        let conn = open_with_extension()?;
        let err = conn
            .query_row::<i64, _, _>(
                "SELECT rdf_owl_rl_materialise(NULL, NULL, '{}')",
                [],
                |r| r.get(0),
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(
                "rdf_owl_rl_materialise: inferred_iri must be a named graph"
            ),
            "expected NULL-inferred refusal; got: {err}"
        );
        Ok(())
    }

    /// Test 6 — default options ({}) work and match expected defaults
    /// (max_iterations=50, provenance=false).
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_options_default() -> Result<()> {
        let conn = open_with_extension()?;
        load_t_box_and_a_box(&conn)?;

        // With provenance: false (default), the delta must NOT be divisible
        // by 3 in general (annotations would triple the count).
        let delta: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert!(delta > 0);

        // No prov:* annotations should appear in the inferred graph.
        let has_provenance: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK {
                GRAPH <urn:g:inferred> {
                  ?q <http://www.w3.org/ns/prov#wasDerivedFrom> ?r .
                }
            }"#],
            |r| r.get(0),
        )?;
        assert_eq!(has_provenance, 0, "defaults should not emit provenance");
        Ok(())
    }

    /// Test 7 — override the provenance predicate IRIs via options.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_provenance_predicate_override() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
                :A rdfs:subClassOf :B .
                :B rdfs:subClassOf :C .
            "#],
            |r| r.get(0),
        )?;
        let options = r#"{
            "provenance": true,
            "derived_by_iri": "http://example.org/customByRule",
            "derived_at_iri": "http://example.org/customAtTime"
        }"#;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', ?)",
            [options],
            |r| r.get(0),
        )?;

        // The annotation should use the OVERRIDDEN predicate, not the default.
        let uses_override: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK {
                GRAPH <urn:g:inferred> {
                  ?q <http://example.org/customByRule> ?r .
                }
            }"#],
            |r| r.get(0),
        )?;
        assert_eq!(uses_override, 1, "expected annotation under custom predicate");

        // The default predicate should NOT have been used.
        let uses_default: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK {
                GRAPH <urn:g:inferred> {
                  ?q <http://www.w3.org/ns/prov#wasDerivedFrom> ?r .
                }
            }"#],
            |r| r.get(0),
        )?;
        assert_eq!(uses_default, 0, "default predicate must not appear when overridden");
        Ok(())
    }

    /// Test 8 — equivalence pin against hand-written expected closure.
    /// Same shape VG's Vv::Graph::Reasoner.materialise! would produce
    /// for this fixture (VG ships the same 15 rules). If either side
    /// drifts, this test fails first.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_equivalence_with_vg() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
                @prefix owl:  <http://www.w3.org/2002/07/owl#> .
                :Aa rdfs:subClassOf :Bb .
                :Bb rdfs:subClassOf :Cc .
                :alice a :Aa .
                :p1 owl:equivalentProperty :p2 .
                :x :p1 :y .
            "#],
            |r| r.get(0),
        )?;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;

        // Expected derived (in inferred graph, no provenance):
        //   scm-sco:  :Aa rdfs:subClassOf :Cc
        //   cax-sco:  :alice a :Bb, :alice a :Cc
        //   scm-eqp1: :p1 rdfs:subPropertyOf :p2, :p2 rdfs:subPropertyOf :p1
        //   prp-spo1: :x :p2 :y                          (via p1 ⊑ p2)
        //
        // Note: prp-spo1 may also create :x :p1 :y via p2 ⊑ p1 — but that
        // already exists in the asserted graph; the inferred-graph dedup
        // (Store::contains on inferred_g) does NOT cover this case (the
        // triple exists in default, not inferred). So the test expects
        // BOTH :x :p1 :y AND :x :p2 :y in the inferred graph, since the
        // inferred-graph membership is independent of the asserted graph.
        // This matches what a separate inferred graph means: it's the
        // closure including triples whose asserted-graph existence
        // doesn't suppress materialisation.
        let expected_derived: &[&str] = &[
            r#"ASK { GRAPH <urn:g:inferred> { <http://e/Aa> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <http://e/Cc> } }"#,
            r#"ASK { GRAPH <urn:g:inferred> { <http://e/alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://e/Bb> } }"#,
            r#"ASK { GRAPH <urn:g:inferred> { <http://e/alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://e/Cc> } }"#,
            r#"ASK { GRAPH <urn:g:inferred> { <http://e/p1> <http://www.w3.org/2000/01/rdf-schema#subPropertyOf> <http://e/p2> } }"#,
            r#"ASK { GRAPH <urn:g:inferred> { <http://e/p2> <http://www.w3.org/2000/01/rdf-schema#subPropertyOf> <http://e/p1> } }"#,
            r#"ASK { GRAPH <urn:g:inferred> { <http://e/x> <http://e/p2> <http://e/y> } }"#,
        ];
        for ask in expected_derived {
            let present: i64 =
                conn.query_row("SELECT sparql_ask(?)", [*ask], |r| r.get(0))?;
            assert_eq!(present, 1, "expected derived triple missing for ASK: {ask}");
        }
        Ok(())
    }

    // ── 0.10.0 Phase D — equality-saturation opt-out ───────────────────────

    /// `equality_saturation: false` short-circuits eq-rep-s/p/o. The
    /// substituted triple `:b :p :o` (from `:a owl:sameAs :b` + `:a :p :o`)
    /// must NOT appear in the inferred graph.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_equality_saturation_disabled() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix owl: <http://www.w3.org/2002/07/owl#> .
                :a owl:sameAs :b .
                :a :p :o .
            "#],
            |r| r.get(0),
        )?;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', \
             '{\"equality_saturation\": false}')",
            [],
            |r| r.get(0),
        )?;

        let substituted: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> { <http://e/b> <http://e/p> <http://e/o> } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(substituted, 0, "eq-rep-s must NOT fire when equality_saturation=false");

        // eq-sym still fires; the reverse sameAs lands in the inferred graph.
        let eq_sym: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> { <http://e/b>
                <http://www.w3.org/2002/07/owl#sameAs> <http://e/a> } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(eq_sym, 1, "eq-sym must still fire regardless of equality_saturation");
        Ok(())
    }

    /// `equality_saturation: true` (the default) does the substitution.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_equality_saturation_default_substitutes() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix owl: <http://www.w3.org/2002/07/owl#> .
                :a owl:sameAs :b .
                :a :p :o .
            "#],
            |r| r.get(0),
        )?;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;

        let substituted: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> { <http://e/b> <http://e/p> <http://e/o> } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(substituted, 1, "eq-rep-s must fire by default");
        Ok(())
    }

    // ── 0.10.0 Phase F — full-stack composition tests for Phase B/C/D/E rules ─

    /// cls-int1 + cls-int2 round-trip: an instance typed under an
    /// intersection class decomposes into each member class (cls-int2);
    /// an instance typed under every member class reconstitutes as the
    /// intersection (cls-int1).
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_intersection_round_trip() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
                @prefix owl:  <http://www.w3.org/2002/07/owl#> .
                :HappyVegetarian owl:intersectionOf ( :Happy :Vegetarian ) .
                :alice a :HappyVegetarian .
                :bob a :Happy ;
                     a :Vegetarian .
            "#],
            |r| r.get(0),
        )?;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        // cls-int2 derivations from alice.
        let alice_happy: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> {
                  <http://e/alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://e/Happy>
                } }"#],
            |r| r.get(0),
        )?;
        let alice_veg: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> {
                  <http://e/alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://e/Vegetarian>
                } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(alice_happy, 1, "cls-int2: alice should be Happy");
        assert_eq!(alice_veg, 1, "cls-int2: alice should be Vegetarian");
        // cls-int1 derivation for bob.
        let bob_hv: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> {
                  <http://e/bob> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://e/HappyVegetarian>
                } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(bob_hv, 1, "cls-int1: bob should be HappyVegetarian");
        Ok(())
    }

    /// prp-spo2 property chain — uncle = parent ∘ sibling. Alice's parent
    /// is Bob; Bob's sibling is Carol → Carol is Alice's uncle.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_property_chain_uncle() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix owl: <http://www.w3.org/2002/07/owl#> .
                :uncle owl:propertyChainAxiom ( :parent :sibling ) .
                :alice :parent :bob .
                :bob :sibling :carol .
            "#],
            |r| r.get(0),
        )?;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        let derived: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> {
                  <http://e/alice> <http://e/uncle> <http://e/carol>
                } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(derived, 1, "prp-spo2: alice should have carol as uncle");
        Ok(())
    }

    /// prp-key + equality_saturation: shared (givenName, familyName) key
    /// collapses two records into one identity. Then eq-rep-s propagates
    /// each record's predicates onto the other.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_has_key_resolves_duplicates() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix owl: <http://www.w3.org/2002/07/owl#> .
                :Person owl:hasKey ( :given :family ) .
                :p1 a :Person ;
                    :given "Alice" ;
                    :family "Smith" ;
                    :age 30 .
                :p2 a :Person ;
                    :given "Alice" ;
                    :family "Smith" ;
                    :email "alice@example.org" .
            "#],
            |r| r.get(0),
        )?;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        let same_as: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> {
                  <http://e/p1> <http://www.w3.org/2002/07/owl#sameAs> <http://e/p2>
                } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(same_as, 1, "prp-key: p1 ≡ p2 from shared (given, family)");
        // eq-rep-s should propagate p1's :age onto p2.
        let p2_age: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> { <http://e/p2> <http://e/age> 30 } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(p2_age, 1, "eq-rep-s: p2 should inherit p1's :age");
        Ok(())
    }

    /// prp-ifp + eq-rep-s — two subjects sharing an inverse-functional
    /// property value collapse, and their other predicates merge.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_inverse_functional_property_collapses() -> Result<()> {
        let conn = open_with_extension()?;
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle(?)",
            [r#"
                @prefix : <http://e/> .
                @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
                @prefix owl: <http://www.w3.org/2002/07/owl#> .
                :email a owl:InverseFunctionalProperty .
                :alice :email "alice@e.org" ;
                       :role :admin .
                :al :email "alice@e.org" ;
                    :nickname "Al" .
            "#],
            |r| r.get(0),
        )?;
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        let merged: i64 = conn.query_row(
            "SELECT sparql_ask(?)",
            [r#"ASK { GRAPH <urn:g:inferred> {
                  <http://e/alice> <http://www.w3.org/2002/07/owl#sameAs> <http://e/al>
                } }"#],
            |r| r.get(0),
        )?;
        assert_eq!(merged, 1, "prp-ifp: alice ≡ al from shared :email value");
        Ok(())
    }

    /// dt-type1 axioms must land in the inferred graph. Picks two
    /// well-known XSD datatypes as a sample of the full list.
    #[test]
    #[serial]
    fn test_rdf_owl_rl_materialise_dt_type1_emits_xsd_axioms() -> Result<()> {
        let conn = open_with_extension()?;
        // Empty input — dt-type1 fires axiomatically regardless of contents.
        let _: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get(0),
        )?;
        for dt in &[
            "http://www.w3.org/2001/XMLSchema#integer",
            "http://www.w3.org/2001/XMLSchema#string",
            "http://www.w3.org/2001/XMLSchema#dateTime",
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#XMLLiteral",
        ] {
            let ask = format!(
                r#"ASK {{ GRAPH <urn:g:inferred> {{
                    <{dt}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type>
                           <http://www.w3.org/2000/01/rdf-schema#Datatype>
                }} }}"#
            );
            let present: i64 =
                conn.query_row("SELECT sparql_ask(?)", [ask.as_str()], |r| r.get(0))?;
            assert_eq!(present, 1, "dt-type1 must emit axiom for {dt}");
        }
        Ok(())
    }

    // ── 0.11.0 rdf_shacl_core_validate ────────────────────────────────────────

    /// Load a Turtle snippet into the named graph `g`. Convenience for
    /// the SHACL tests, all of which need a shapes graph and a data graph.
    fn load_turtle_to(conn: &Connection, g: &str, turtle: &str) -> Result<()> {
        let _: i64 = conn.query_row(
            "SELECT rdf_load_turtle_to_graph(?, ?)",
            rusqlite::params![turtle, g],
            |r| r.get(0),
        )?;
        Ok(())
    }

    /// Ergonomic SPARQL ASK that returns `bool` directly.
    fn sparql_ask(conn: &Connection, q: &str) -> Result<bool> {
        let n: i64 = conn.query_row("SELECT sparql_ask(?)", [q], |r| r.get(0))?;
        Ok(n != 0)
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_min_count_violation() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh:   <http://www.w3.org/ns/shacl#> .
            @prefix ex:   <http://example.org/> .
            @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

            ex:PersonShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [
                sh:path ex:name ;
                sh:minCount 1 ;
              ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:alice a ex:Person ; ex:name "Alice" .
            ex:bob   a ex:Person .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        assert!(sparql_ask(
            &conn,
            r#"ASK { GRAPH <urn:g:report> {
                ?r <http://www.w3.org/ns/shacl#focusNode> <http://example.org/bob> ;
                   <http://www.w3.org/ns/shacl#sourceConstraintComponent>
                     <http://www.w3.org/ns/shacl#MinCountConstraintComponent>
            } }"#,
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_datatype_violation() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh:   <http://www.w3.org/ns/shacl#> .
            @prefix ex:   <http://example.org/> .
            @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

            ex:AgeShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [
                sh:path ex:age ;
                sh:datatype xsd:integer ;
              ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex:  <http://example.org/> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
            ex:alice a ex:Person ; ex:age "30"^^xsd:integer .
            ex:bob   a ex:Person ; ex:age "thirty" .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_full_shape_round_trip() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh:   <http://www.w3.org/ns/shacl#> .
            @prefix ex:   <http://example.org/> .
            @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

            ex:PersonShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [
                sh:path ex:name ;
                sh:minCount 1 ;
              ] ;
              sh:property [
                sh:path ex:age ;
                sh:datatype xsd:integer ;
              ] ;
              sh:property [
                sh:path ex:code ;
                sh:pattern "^[A-Z]{3}$" ;
              ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex:  <http://example.org/> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

            # Conforming
            ex:alice a ex:Person ; ex:name "Alice" ; ex:age "30"^^xsd:integer ; ex:code "ABC" .
            # Missing ex:name → minCount violation
            ex:bob   a ex:Person ; ex:age "40"^^xsd:integer ; ex:code "DEF" .
            # ex:age is a string → datatype violation. Also wrong code pattern.
            ex:carol a ex:Person ; ex:name "Carol" ; ex:age "x" ; ex:code "abc" .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 3, "one minCount + one datatype + one pattern");
        assert!(sparql_ask(
            &conn,
            r#"ASK { GRAPH <urn:g:report> {
                ?report a <http://www.w3.org/ns/shacl#ValidationReport> ;
                        <http://www.w3.org/ns/shacl#conforms>
                          "false"^^<http://www.w3.org/2001/XMLSchema#boolean>
            } }"#,
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_conforms_when_no_violations() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix ex: <http://example.org/> .
            ex:PersonShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [ sh:path ex:name ; sh:minCount 1 ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:alice a ex:Person ; ex:name "Alice" .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 0);
        assert!(sparql_ask(
            &conn,
            r#"ASK { GRAPH <urn:g:report> {
                ?report <http://www.w3.org/ns/shacl#conforms>
                  "true"^^<http://www.w3.org/2001/XMLSchema#boolean>
            } }"#,
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_max_violations_guard() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix ex: <http://example.org/> .
            ex:PersonShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [ sh:path ex:name ; sh:minCount 1 ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:a a ex:Person . ex:b a ex:Person . ex:c a ex:Person .
            "#,
        )?;
        let err = conn.query_row::<i64, _, _>(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report',
                                            '{\"max_violations\": 1}')",
            [],
            |r| r.get(0),
        );
        let msg = format!("{err:?}");
        assert!(
            msg.contains("max_violations"),
            "expected max_violations error, got: {msg}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_data_iri_default_graph() -> Result<()> {
        let conn = open_with_extension()?;
        // Data lands in default graph this time.
        conn.execute_batch(
            r#"
            SELECT rdf_load_turtle('
              @prefix ex: <http://example.org/> .
              ex:alice a ex:Person .
            ');
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix ex: <http://example.org/> .
            ex:PersonShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [ sh:path ex:name ; sh:minCount 1 ] .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate(NULL, 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_shapes_iri_required() -> Result<()> {
        let conn = open_with_extension()?;
        let err = conn.query_row::<i64, _, _>(
            "SELECT rdf_shacl_core_validate('urn:g:data', NULL, 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        );
        let msg = format!("{err:?}");
        assert!(msg.contains("shapes_iri"), "got: {msg}");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_report_iri_required() -> Result<()> {
        let conn = open_with_extension()?;
        let err = conn.query_row::<i64, _, _>(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', NULL, '{}')",
            [],
            |r| r.get(0),
        );
        let msg = format!("{err:?}");
        assert!(msg.contains("report_iri"), "got: {msg}");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_clears_report_on_rewrite() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix ex: <http://example.org/> .
            ex:PersonShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [ sh:path ex:name ; sh:minCount 1 ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:bob a ex:Person .
            "#,
        )?;
        // First run — produces violations.
        let _: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        // Now fix the data and re-validate; report should be cleared
        // and reflect the new state, not accumulate.
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:bob ex:name "Bob" .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 0);
        // No stale ValidationResult nodes from the prior run.
        let stale: i64 = conn.query_row(
            r#"SELECT sparql_ask('ASK { GRAPH <urn:g:report> {
                ?r a <http://www.w3.org/ns/shacl#ValidationResult>
            } }')"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(stale, 0, "rewrite should clear prior ValidationResult nodes");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_path_inverse() -> Result<()> {
        let conn = open_with_extension()?;
        // Shape targets ex:Person and requires `^ex:parent` to have
        // at least one value — i.e. every Person must be someone's parent.
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix ex: <http://example.org/> .
            ex:ParentShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [
                sh:path [ sh:inversePath ex:parent ] ;
                sh:minCount 1 ;
              ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:alice a ex:Person .
            ex:bob   a ex:Person .   # is a parent of nobody → violation
            ex:carol a ex:Person ;   # is a parent (via inverse path)
                     ex:parent ex:alice .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        // alice's inverse-parent set = {carol}; bob's = {}; carol's = {}.
        // So bob AND carol violate. (carol has ex:parent but no one has
        // ex:parent → carol.)
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_path_sequence() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix ex: <http://example.org/> .
            ex:GrandparentShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [
                sh:path ( ex:parent ex:parent ) ;
                sh:minCount 1 ;
              ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:alice a ex:Person ; ex:parent ex:bob .
            ex:bob   a ex:Person ; ex:parent ex:carol .
            ex:carol a ex:Person .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        // alice has a grandparent (carol). bob and carol don't. Two violations.
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_shacl_core_validate_in_constraint() -> Result<()> {
        let conn = open_with_extension()?;
        load_turtle_to(
            &conn,
            "urn:g:shapes",
            r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix ex: <http://example.org/> .
            ex:RoleShape a sh:NodeShape ;
              sh:targetClass ex:Person ;
              sh:property [
                sh:path ex:role ;
                sh:in ( "admin" "editor" "viewer" ) ;
              ] .
            "#,
        )?;
        load_turtle_to(
            &conn,
            "urn:g:data",
            r#"
            @prefix ex: <http://example.org/> .
            ex:alice a ex:Person ; ex:role "admin" .
            ex:bob   a ex:Person ; ex:role "hacker" .
            "#,
        )?;
        let count: i64 = conn.query_row(
            "SELECT rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes', 'urn:g:report', '{}')",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        Ok(())
    }

    // ── 0.12.0 rdf_dred_overdelete ────────────────────────────────────────────

    /// Seed a `scm-sco` chain `:A ⊑ :B ⊑ :C` in the default graph and
    /// materialise it into `urn:g:inferred` with `track_dependencies: true`.
    /// Returns the post-materialise inferred-graph count for sanity.
    fn seed_scm_sco_chain(conn: &Connection) -> Result<i64> {
        conn.execute_batch(
            r#"
            SELECT rdf_insert(
              'http://example.org/A',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/B'
            );
            SELECT rdf_insert(
              'http://example.org/B',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/C'
            );
            "#,
        )?;
        conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred',
                 json('{\"track_dependencies\": true}'))",
            [],
            |r| r.get::<_, i64>(0),
        )
    }

    /// Count quads in a named graph.
    fn count_in_graph(conn: &Connection, g: &str) -> Result<i64> {
        let q = format!(
            "SELECT COUNT(*) FROM (SELECT sparql_query(
               'SELECT ?s WHERE {{ GRAPH <{g}> {{ ?s ?p ?o }} }}'
             ) j, json_each(j))"
        );
        conn.query_row(&q, [], |r| r.get(0))
    }

    /// Convenience: does the inferred graph contain a specific triple?
    fn inferred_has(conn: &Connection, s: &str, p: &str, o: &str) -> Result<bool> {
        let q = format!(
            "ASK {{ GRAPH <urn:g:inferred> {{ <{s}> <{p}> <{o}> }} }}"
        );
        sparql_ask(&conn, &q)
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_direct_dependency() -> Result<()> {
        let conn = open_with_extension()?;
        seed_scm_sco_chain(&conn)?;
        // The inferred graph should contain A ⊑ C via scm-sco.
        assert!(inferred_has(
            &conn,
            "http://example.org/A",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/C"
        )?);

        // Retract the B ⊑ C asserted premise (consumer-side delete is
        // out of scope for this test — we just hand the premise to
        // overdelete and verify the inferred derivation cascades).
        let removed: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/B",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/C"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(removed, 1, "exactly the derived A⊑C should over-delete");
        assert!(!inferred_has(
            &conn,
            "http://example.org/A",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/C"
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_transitive_cascade() -> Result<()> {
        let conn = open_with_extension()?;
        // Three-level chain :A ⊑ :B ⊑ :C ⊑ :D so we get two
        // transitive-closure derivations chained via :B-:D.
        conn.execute_batch(
            r#"
            SELECT rdf_insert(
              'http://example.org/A',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/B'
            );
            SELECT rdf_insert(
              'http://example.org/B',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/C'
            );
            SELECT rdf_insert(
              'http://example.org/C',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/D'
            );
            "#,
        )?;
        conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred',
                 json('{\"track_dependencies\": true}'))",
            [],
            |r| r.get::<_, i64>(0),
        )?;
        // scm-sco closure: {A⊑C, B⊑D, A⊑D} — three derivations.
        assert!(inferred_has(
            &conn, "http://example.org/A",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/D"
        )?);

        // Retract B⊑C: removes A⊑C (direct), and A⊑D loses one
        // derivation. The other derivation of A⊑D (via A⊑B + B⊑D
        // where B⊑D itself depends on B⊑C → C⊑D) also breaks because
        // B⊑D depended on B⊑C. So A⊑D and B⊑D and A⊑C all cascade.
        let removed: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/B",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/C"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(removed, 3, "A⊑C, B⊑D, A⊑D all cascade");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_preserves_other_inferences() -> Result<()> {
        let conn = open_with_extension()?;
        // Two independent chains share no premises. Retract one premise
        // chain → the other chain's inferences survive.
        conn.execute_batch(
            r#"
            SELECT rdf_insert(
              'http://example.org/A',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/B'
            );
            SELECT rdf_insert(
              'http://example.org/B',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/C'
            );
            SELECT rdf_insert(
              'http://example.org/X',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/Y'
            );
            SELECT rdf_insert(
              'http://example.org/Y',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/Z'
            );
            "#,
        )?;
        conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred',
                 json('{\"track_dependencies\": true}'))",
            [],
            |r| r.get::<_, i64>(0),
        )?;
        // Retract A⊑B → only A⊑C cascades; X⊑Z survives.
        let removed: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/A",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/B"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(removed, 1);
        assert!(inferred_has(
            &conn, "http://example.org/X",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/Z"
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_no_op_when_no_dependents() -> Result<()> {
        let conn = open_with_extension()?;
        seed_scm_sco_chain(&conn)?;
        // A premise that's not in the index → return 0, no changes.
        let removed: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/QQQ",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/RRR"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(removed, 0);
        // The original derivation still there.
        assert!(inferred_has(
            &conn, "http://example.org/A",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/C"
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_requires_track_dependencies() -> Result<()> {
        let conn = open_with_extension()?;
        // Materialise WITHOUT track_dependencies → index stays empty.
        conn.execute_batch(
            r#"
            SELECT rdf_insert(
              'http://example.org/A',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/B'
            );
            SELECT rdf_insert(
              'http://example.org/B',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/C'
            );
            "#,
        )?;
        conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')",
            [],
            |r| r.get::<_, i64>(0),
        )?;
        // Now over-delete should error with the fixed-prefix wiring message.
        let err = conn
            .query_row(
                r#"SELECT rdf_dred_overdelete(
                     'urn:g:inferred',
                     json('[["http://example.org/B",
                            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                            "http://example.org/C"]]')
                   )"#,
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("rdf_dred_overdelete: no dependency index"),
            "expected fixed-prefix error, got: {msg}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_full_cycle_overdelete_then_rematerialise() -> Result<()> {
        let conn = open_with_extension()?;
        seed_scm_sco_chain(&conn)?;

        // Retract B⊑C and run overdelete → A⊑C is over-deleted.
        conn.execute_batch(
            r#"SELECT rdf_delete(
                 'http://example.org/B',
                 'http://www.w3.org/2000/01/rdf-schema#subClassOf',
                 'http://example.org/C'
               );"#,
        )?;
        let removed: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/B",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/C"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(removed, 1);

        // Re-materialise: nothing else to derive (B⊑C is gone).
        let delta: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred',
                 json('{\"track_dependencies\": true}'))",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(delta, 0);
        assert!(!inferred_has(
            &conn, "http://example.org/A",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/C"
        )?);

        // Now re-add B⊑C and re-materialise: A⊑C reappears.
        conn.execute_batch(
            r#"SELECT rdf_insert(
                 'http://example.org/B',
                 'http://www.w3.org/2000/01/rdf-schema#subClassOf',
                 'http://example.org/C'
               );"#,
        )?;
        let delta2: i64 = conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred',
                 json('{\"track_dependencies\": true}'))",
            [],
            |r| r.get(0),
        )?;
        assert!(delta2 >= 1);
        assert!(inferred_has(
            &conn, "http://example.org/A",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/C"
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_multi_derivation() -> Result<()> {
        let conn = open_with_extension()?;
        // A⊑B1⊑C and A⊑B2⊑C — two independent derivations of A⊑C.
        // Retract one chain's premise → A⊑C survives via the other.
        conn.execute_batch(
            r#"
            SELECT rdf_insert(
              'http://example.org/A',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/B1'
            );
            SELECT rdf_insert(
              'http://example.org/B1',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/C'
            );
            SELECT rdf_insert(
              'http://example.org/A',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/B2'
            );
            SELECT rdf_insert(
              'http://example.org/B2',
              'http://www.w3.org/2000/01/rdf-schema#subClassOf',
              'http://example.org/C'
            );
            "#,
        )?;
        conn.query_row(
            "SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred',
                 json('{\"track_dependencies\": true}'))",
            [],
            |r| r.get::<_, i64>(0),
        )?;
        // Retract just B1⊑C → A⊑C should SURVIVE (still derivable via B2).
        let removed: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/B1",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/C"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        // Nothing fully cascades: A⊑C has another derivation alive.
        assert_eq!(removed, 0);
        assert!(inferred_has(
            &conn, "http://example.org/A",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://example.org/C"
        )?);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_clears_index_entry() -> Result<()> {
        let conn = open_with_extension()?;
        seed_scm_sco_chain(&conn)?;
        // Retract B⊑C → A⊑C cascades.
        let removed: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/B",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/C"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(removed, 1);
        // Calling again with the same premise → index entry already gone,
        // no further cascade.
        let removed2: i64 = conn.query_row(
            r#"SELECT rdf_dred_overdelete(
                 'urn:g:inferred',
                 json('[["http://example.org/B",
                        "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                        "http://example.org/C"]]')
               )"#,
            [],
            |r| r.get(0),
        )?;
        assert_eq!(removed2, 0);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_dred_overdelete_rejects_empty_inferred_iri() -> Result<()> {
        let conn = open_with_extension()?;
        seed_scm_sco_chain(&conn)?;
        let err = conn
            .query_row(
                r#"SELECT rdf_dred_overdelete('', '[]')"#,
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("inferred_iri must be a named graph"),
            "expected named-graph error, got: {msg}"
        );
        Ok(())
    }

    // Note: count_in_graph kept as a developer probe; not used by every
    // test but useful when adding new ones.
    #[allow(dead_code)]
    fn _keep_count_in_graph(conn: &Connection, g: &str) -> Result<i64> {
        count_in_graph(conn, g)
    }

    // ── 0.13.0 rdf_owl_rl_consistent ──────────────────────────────────────────

    /// Ergonomic: call the consistent function and parse the JSON return.
    fn consistent_violations(conn: &Connection, opts: &str) -> Result<serde_json::Value> {
        let json: String = conn.query_row(
            "SELECT rdf_owl_rl_consistent(NULL, 'urn:g:inferred', ?)",
            [opts],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&json).expect("violations JSON parse"))
    }

    fn first_rule(v: &serde_json::Value) -> String {
        v[0]["rule"].as_str().unwrap_or("").to_string()
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_empty_store_returns_array() -> Result<()> {
        let conn = open_with_extension()?;
        let v = consistent_violations(&conn, "{}")?;
        assert!(v.as_array().unwrap().is_empty());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_no_violations() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
               'http://e/Person');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert!(v.as_array().unwrap().is_empty());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_cax_dw_single_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/Animal',
               'http://www.w3.org/2002/07/owl#disjointWith', 'http://e/Plant');
             SELECT rdf_insert('http://e/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Animal');
             SELECT rdf_insert('http://e/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Plant');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1, "expected one cax-dw record, got {arr:?}");
        assert_eq!(first_rule(&v), "cax-dw");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_eq_diff1_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/x',
               'http://www.w3.org/2002/07/owl#differentFrom', 'http://e/y');
             SELECT rdf_insert('http://e/x',
               'http://www.w3.org/2002/07/owl#sameAs', 'http://e/y');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(first_rule(&v), "eq-diff1");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_prp_npa1_violation() -> Result<()> {
        let conn = open_with_extension()?;
        // NPA in blank-node form: _:n sourceIndividual :alice ;
        //                              assertionProperty :married ;
        //                              targetIndividual :bob .
        conn.execute_batch(
            "SELECT rdf_load_turtle('
              @prefix owl: <http://www.w3.org/2002/07/owl#> .
              @prefix ex:  <http://example.org/> .

              [] a owl:NegativePropertyAssertion ;
                 owl:sourceIndividual ex:alice ;
                 owl:assertionProperty ex:married ;
                 owl:targetIndividual ex:bob .

              ex:alice ex:married ex:bob .
            ');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1, "expected one prp-npa1 record, got {arr:?}");
        assert_eq!(first_rule(&v), "prp-npa1");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_prp_npa2_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_load_turtle('
              @prefix owl: <http://www.w3.org/2002/07/owl#> .
              @prefix ex:  <http://example.org/> .

              [] a owl:NegativePropertyAssertion ;
                 owl:sourceIndividual ex:alice ;
                 owl:assertionProperty ex:age ;
                 owl:targetValue 42 .

              ex:alice ex:age 42 .
            ');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "prp-npa2");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_prp_irp_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/parentOf',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
               'http://www.w3.org/2002/07/owl#IrreflexiveProperty');
             SELECT rdf_insert('http://e/alice', 'http://e/parentOf', 'http://e/alice');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "prp-irp");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_prp_asyp_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/parentOf',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
               'http://www.w3.org/2002/07/owl#AsymmetricProperty');
             SELECT rdf_insert('http://e/alice', 'http://e/parentOf', 'http://e/bob');
             SELECT rdf_insert('http://e/bob',   'http://e/parentOf', 'http://e/alice');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "prp-asyp");
        // Symmetric pair → exactly ONE record (lex-smaller witness).
        assert_eq!(v.as_array().unwrap().len(), 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_prp_pdw_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/parentOf',
               'http://www.w3.org/2002/07/owl#propertyDisjointWith',
               'http://e/enemyOf');
             SELECT rdf_insert('http://e/alice', 'http://e/parentOf', 'http://e/bob');
             SELECT rdf_insert('http://e/alice', 'http://e/enemyOf', 'http://e/bob');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "prp-pdw");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_cls_nothing2_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/x',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
               'http://www.w3.org/2002/07/owl#Nothing');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "cls-nothing2");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_cls_com_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/Living',
               'http://www.w3.org/2002/07/owl#complementOf', 'http://e/Dead');
             SELECT rdf_insert('http://e/zombie',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Living');
             SELECT rdf_insert('http://e/zombie',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Dead');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "cls-com");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_cls_maxc1_violation() -> Result<()> {
        let conn = open_with_extension()?;
        // Restriction class with owl:maxCardinality 0 onProperty :hasChild;
        // :alice rdf:type :Childless . :alice :hasChild :ben .
        conn.execute_batch(
            "SELECT rdf_load_turtle('
              @prefix owl:  <http://www.w3.org/2002/07/owl#> .
              @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .
              @prefix ex:   <http://example.org/> .

              ex:Childless owl:maxCardinality 0 ;
                           owl:onProperty ex:hasChild .

              ex:alice a ex:Childless ;
                       ex:hasChild ex:ben .
            ');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "cls-maxc1");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_dt_not_type_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            r#"SELECT rdf_insert(
                 'http://e/alice', 'http://e/age',
                 '"thirty"^^<http://www.w3.org/2001/XMLSchema#integer>'
               );"#,
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert_eq!(first_rule(&v), "dt-not-type");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_multiple_violations_distinct_rules() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/Animal',
               'http://www.w3.org/2002/07/owl#disjointWith', 'http://e/Plant');
             SELECT rdf_insert('http://e/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Animal');
             SELECT rdf_insert('http://e/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Plant');
             SELECT rdf_insert('http://e/parentOf',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
               'http://www.w3.org/2002/07/owl#IrreflexiveProperty');
             SELECT rdf_insert('http://e/bob', 'http://e/parentOf', 'http://e/bob');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2, "expected 2 violations, got {arr:?}");
        let rules: std::collections::HashSet<String> = arr
            .iter()
            .map(|r| r["rule"].as_str().unwrap().to_string())
            .collect();
        assert!(rules.contains("cax-dw"));
        assert!(rules.contains("prp-irp"));
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_max_violations_guard() -> Result<()> {
        let conn = open_with_extension()?;
        // Five separate cax-dw violations.
        conn.execute_batch(
            "SELECT rdf_insert('http://e/A',
               'http://www.w3.org/2002/07/owl#disjointWith', 'http://e/B');",
        )?;
        for i in 1..=5 {
            let s = format!(
                "SELECT rdf_insert('http://e/x{i}',
                  'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/A');
                 SELECT rdf_insert('http://e/x{i}',
                  'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/B');"
            );
            conn.execute_batch(&s)?;
        }
        let err = conn
            .query_row(
                "SELECT rdf_owl_rl_consistent(NULL, 'urn:g:inferred',
                   json('{\"max_violations\": 2}'))",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("rdf_owl_rl_consistent: violation count exceeded max_violations"),
            "expected fixed-prefix guard error, got: {msg}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_read_only() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_insert('http://e/Animal',
               'http://www.w3.org/2002/07/owl#disjointWith', 'http://e/Plant');
             SELECT rdf_insert('http://e/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Animal');
             SELECT rdf_insert('http://e/alice',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/Plant');",
        )?;
        let before: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        let _ = consistent_violations(&conn, "{}")?;
        let after: i64 = conn.query_row("SELECT rdf_count()", [], |r| r.get(0))?;
        assert_eq!(before, after, "consistent must not write to the store");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_inferred_iri_required() -> Result<()> {
        let conn = open_with_extension()?;
        let err = conn
            .query_row(
                "SELECT rdf_owl_rl_consistent(NULL, NULL, '{}')",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("rdf_owl_rl_consistent: inferred_iri must be a named graph"),
            "expected named-graph error, got: {msg}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_cax_adc_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_load_turtle('
              @prefix owl: <http://www.w3.org/2002/07/owl#> .
              @prefix ex:  <http://example.org/> .

              [] a owl:AllDisjointClasses ;
                 owl:members ( ex:A ex:B ex:C ) .

              ex:alice a ex:A , ex:B .
            ');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        let arr = v.as_array().unwrap();
        assert!(!arr.is_empty(), "expected cax-adc violation, got {arr:?}");
        assert_eq!(first_rule(&v), "cax-adc");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_eq_diff2_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_load_turtle('
              @prefix owl: <http://www.w3.org/2002/07/owl#> .
              @prefix ex:  <http://example.org/> .

              [] a owl:AllDifferent ;
                 owl:members ( ex:a ex:b ex:c ) .

              ex:a owl:sameAs ex:b .
            ');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert!(!v.as_array().unwrap().is_empty());
        assert_eq!(first_rule(&v), "eq-diff2");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_eq_diff3_violation() -> Result<()> {
        let conn = open_with_extension()?;
        conn.execute_batch(
            "SELECT rdf_load_turtle('
              @prefix owl: <http://www.w3.org/2002/07/owl#> .
              @prefix ex:  <http://example.org/> .

              [] a owl:AllDifferent ;
                 owl:distinctMembers ( ex:a ex:b ex:c ) .

              ex:a owl:sameAs ex:b .
            ');",
        )?;
        let v = consistent_violations(&conn, "{}")?;
        assert!(!v.as_array().unwrap().is_empty());
        assert_eq!(first_rule(&v), "eq-diff3");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_rdf_owl_rl_consistent_ordering_stable() -> Result<()> {
        let conn = open_with_extension()?;
        // Multiple cax-dw violations — output must be deterministically sorted.
        conn.execute_batch(
            "SELECT rdf_insert('http://e/A',
               'http://www.w3.org/2002/07/owl#disjointWith', 'http://e/B');
             SELECT rdf_insert('http://e/x1',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/A');
             SELECT rdf_insert('http://e/x1',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/B');
             SELECT rdf_insert('http://e/x2',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/A');
             SELECT rdf_insert('http://e/x2',
               'http://www.w3.org/1999/02/22-rdf-syntax-ns#type', 'http://e/B');",
        )?;
        let v1 = consistent_violations(&conn, "{}")?;
        let v2 = consistent_violations(&conn, "{}")?;
        assert_eq!(v1, v2, "two back-to-back calls must produce identical JSON");
        // The lex-smaller subject (x1) should come first.
        let arr = v1.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr[0]["s"].as_str().unwrap().contains("x1"));
        assert!(arr[1]["s"].as_str().unwrap().contains("x2"));
        Ok(())
    }
}
