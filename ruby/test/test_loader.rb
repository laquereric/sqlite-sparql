# frozen_string_literal: true

require_relative "test_helper"

class TestLoader < Minitest::Test
  def setup
    @db = SQLite3::Database.new(":memory:")
  end

  def teardown
    @db.close if @db && !@db.closed?
  end

  def test_load_makes_rdf_count_callable
    SqliteSparql.load(@db)
    @db.execute("SELECT rdf_clear()")
    count = @db.get_first_value("SELECT rdf_count()")
    assert_equal 0, count
  end

  def test_load_is_idempotent
    SqliteSparql.load(@db)
    SqliteSparql.load(@db) # second load must not raise
    @db.execute("SELECT rdf_clear()")
    assert_equal 0, @db.get_first_value("SELECT rdf_count()")
  end

  def test_path_resolves_to_existing_file
    assert File.exist?(SqliteSparql.path), "expected cdylib at #{SqliteSparql.path}"
  end

  def test_round_trip_one_triple
    SqliteSparql.load(@db)
    @db.execute("SELECT rdf_clear()")
    @db.execute(
      "SELECT rdf_insert(?, ?, ?)",
      ["http://example.org/alice", "http://example.org/knows", "http://example.org/bob"]
    )
    assert_equal 1, @db.get_first_value("SELECT rdf_count()")
  end
end
