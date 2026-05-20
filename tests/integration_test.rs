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
}
