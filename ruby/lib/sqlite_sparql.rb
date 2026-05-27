# frozen_string_literal: true

require "sqlite3"
require "fileutils"
require_relative "sqlite_sparql/version"

# Ruby wrapper for the sqlite-sparql loadable SQLite extension.
#
# Quick start:
#
#   require "sqlite3"
#   require "sqlite_sparql"
#
#   db = SQLite3::Database.new(":memory:")
#   SqliteSparql.load(db)
#
#   db.execute("SELECT rdf_insert(?, ?, ?)",
#              ["http://example.org/alice",
#               "http://xmlns.com/foaf/0.1/name",
#               "\"Alice\""])
#   db.get_first_value("SELECT rdf_count()")  # => 1
#
# For the ergonomic Ruby surface use `SqliteSparql::Store.new(db)`;
# for Rails models include `SqliteSparql::HasRdfTriples`.
module SqliteSparql
  # SQLite auto-derives the extension's init function name from the cdylib
  # basename — strip "lib" prefix, strip extension, prepend "sqlite3_",
  # append "_init". Our engine's entrypoint is `sqlite3_sqlitesparql_init`
  # (no underscore between "sqlite" and "sparql"), so the vendored
  # cdylib MUST be named `libsqlitesparql.{dylib,so,dll}` for the
  # `sqlite3` gem's 2.x `#load_extension(path)` API — which calls the
  # C-level `sqlite3_load_extension(db, path, NULL, &err)` — to find it
  # without an explicit entrypoint argument.
  VENDORED_BASENAME = "libsqlitesparql"

  # Same function name the Rust extension exports via `#[sqlite_entrypoint]`.
  # Documentary only — auto-derivation handles the lookup at load time.
  ENTRYPOINT = "sqlite3_sqlitesparql_init"

  class LoadError < StandardError; end

  # Load the compiled extension into the given `SQLite3::Database`
  # connection. Idempotent: loading a second time on the same connection
  # surfaces SQLite's "function already exists" error which we swallow.
  def self.load(db)
    db.enable_load_extension(true)
    begin
      db.load_extension(path)
    rescue ::SQLite3::SQLException => e
      raise unless already_loaded_error?(e)
    end
    db.enable_load_extension(false)
    db
  end

  # Absolute path to the cdylib for the current platform, named so that
  # SQLite's auto-derivation produces `sqlite3_sqlitesparql_init`.
  #
  # Resolution order:
  #
  #   1. `ENV["SQLITE_SPARQL_CDYLIB"]` — explicit override. Caller is
  #      responsible for ensuring auto-derivation matches (i.e. the
  #      file ends in `libsqlitesparql.{dylib,so,dll}` OR they've
  #      arranged their own entrypoint hookup).
  #   2. Vendored binary under `ruby/vendor/<arch>-<os>/`.
  #   3. Development fallback: rewrap the engine's
  #      `target/release/libsqlite_sparql.{ext}` build artifact under
  #      a temp path with the required basename, so SQLite finds the
  #      entrypoint via auto-derivation. The rewrap is a hardlink when
  #      possible (cheap, same inode), otherwise a copy.
  def self.path
    return ENV["SQLITE_SPARQL_CDYLIB"] if ENV["SQLITE_SPARQL_CDYLIB"]

    ext = library_extension
    vendored = File.expand_path(
      "../vendor/#{platform_dir}/#{VENDORED_BASENAME}.#{ext}",
      __dir__
    )
    return vendored if File.exist?(vendored)

    dev = rewrap_dev_build(ext)
    return dev if dev

    raise LoadError, <<~MSG
      sqlite-sparql: no vendored binary found for #{platform_dir}.
        Expected: #{vendored}
        Also tried rewrapping a dev build at:
          #{engine_root_release_path(ext)}
        and that does not exist either.

      Either:
        - Run `cargo build --release` in the engine root; or
        - `cd ruby && rake native` to vendor the host-platform binary; or
        - Set SQLITE_SPARQL_CDYLIB to an absolute path whose basename
          is `libsqlitesparql.#{ext}` so SQLite auto-derives the
          `sqlite3_sqlitesparql_init` entrypoint correctly.
    MSG
  end

  # File extension for the current platform's loadable library.
  def self.library_extension
    case Gem::Platform.local.os
    when /darwin/                    then "dylib"
    when /linux|bsd/                 then "so"
    when /mingw|mswin|cygwin/        then "dll"
    else raise LoadError, "sqlite-sparql: unsupported platform #{Gem::Platform.local.os}"
    end
  end

  def self.platform_dir
    "#{Gem::Platform.local.cpu}-#{Gem::Platform.local.os}"
  end

  def self.engine_root_release_path(ext)
    File.expand_path("../../target/release/libsqlite_sparql.#{ext}", __dir__)
  end
  private_class_method :engine_root_release_path

  # Hardlink or copy `target/release/libsqlite_sparql.{ext}` to a temp
  # path basename `libsqlitesparql.{ext}` so SQLite's filename-based
  # entrypoint auto-derivation produces `sqlite3_sqlitesparql_init`.
  # Memoised across calls within a process.
  def self.rewrap_dev_build(ext)
    return @_rewrapped if defined?(@_rewrapped) && @_rewrapped && File.exist?(@_rewrapped)

    src = engine_root_release_path(ext)
    return nil unless File.exist?(src)

    tmpdir = File.join(Dir.tmpdir, "sqlite-sparql-#{Process.pid}")
    FileUtils.mkdir_p(tmpdir)
    dst = File.join(tmpdir, "#{VENDORED_BASENAME}.#{ext}")
    unless File.exist?(dst) && File.size(dst) == File.size(src)
      File.unlink(dst) if File.exist?(dst)
      begin
        File.link(src, dst)
      rescue Errno::EXDEV, Errno::EPERM, Errno::EACCES
        FileUtils.cp(src, dst)
      end
    end
    @_rewrapped = dst
  end
  private_class_method :rewrap_dev_build

  def self.already_loaded_error?(error)
    msg = error.message.to_s
    msg.include?("already exists") || msg.include?("already loaded")
  end
  private_class_method :already_loaded_error?
end

require "tmpdir"
require_relative "sqlite_sparql/store"
