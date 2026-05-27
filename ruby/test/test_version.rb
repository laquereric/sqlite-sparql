# frozen_string_literal: true

require_relative "test_helper"

class TestVersion < Minitest::Test
  def test_gem_version_matches_engine_version
    engine_version = File.read(File.expand_path("../../VERSION", __dir__)).strip
    assert_equal engine_version, SqliteSparql::VERSION,
                 "gem VERSION (#{SqliteSparql::VERSION}) must match engine VERSION (#{engine_version}); update lib/sqlite_sparql/version.rb in lockstep with /VERSION"
  end
end
