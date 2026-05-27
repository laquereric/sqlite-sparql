# frozen_string_literal: true

require "minitest/autorun"
require "sqlite3"

# Fail loudly at boot if the engine cdylib hasn't been built — the
# loader's dev-rewrap fallback expects `target/release/libsqlite_sparql.{ext}`.
ext = case RUBY_PLATFORM
      when /darwin/             then "dylib"
      when /linux|bsd/          then "so"
      when /mingw|mswin|cygwin/ then "dll"
      else raise "unsupported test platform: #{RUBY_PLATFORM}"
      end
candidate = File.expand_path("../../target/release/libsqlite_sparql.#{ext}", __dir__)
unless File.exist?(candidate)
  raise <<~MSG
    sqlite-sparql test suite: no compiled cdylib found at
      #{candidate}
    Run `cargo build --release` from the engine root before `rake test`.
  MSG
end

$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "sqlite_sparql"
