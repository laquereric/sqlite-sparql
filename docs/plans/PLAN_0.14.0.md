# PLAN 0.14.0 — `sqlite-sparql-ruby` gem wrapper

> Ship a companion Ruby gem (`sqlite-sparql`) that vendors the
> compiled extension, exposes a `SqliteSparql.load(db)` loader that
> mirrors `sqlite-vec`'s pattern, and bundles a small ergonomic
> layer for Rails apps that prefer Ruby helpers over raw SQL. The
> gem lives in a new top-level `ruby/` subdirectory of this repo
> so the engine and its first-party language wrapper version
> together; cross-platform binary distribution via GitHub Releases
> follows in a downstream plan.

Driver: `CLAUDE.md` "Completing the Implementation" item **#11
— Rails Gem Wrapper (`sqlite-sparql-ruby`)**. Forward-leaning
ship per the project pattern (`feedback_forward_leaning_ship` —
substrate work doesn't need a CONSUMER ask; consumers adopt on
their cadence). With 0.13.0's `rdf_owl_rl_consistent` rounding
out the reasoning surface, the engine has stabilised enough for
a language wrapper to make sense.

Depends on every prior milestone — the gem's loader is a thin
shell over the cdylib that 0.1.0+ produces. No engine code
changes; the only Rust touch is a `release-binaries.yml` CI
stub (Phase F, optional follow-on).

VG posture: the gem is independent of `vv-graph`. `vv-graph`
loads the extension via Rails 8's `extensions:` config key
today; the new gem doesn't replace that — it complements it
for non-Rails Ruby consumers who don't have a
`config/database.yml`. Documented in the gem README.

---

## Goal

`cd ruby && bundle exec rake test` passes end-to-end. The test
suite:

1. Builds the cdylib via `cargo build --release` (via a Rake
   task that shells out one directory up).
2. Copies the `.dylib` into `ruby/vendor/<platform>/` so the
   loader can find it without an external path.
3. Loads the extension into a fresh `SQLite3::Database.new(":memory:")`
   via `SqliteSparql.load(db)`.
4. Round-trips one triple through `rdf_insert` / `rdf_count` /
   `sparql_query`.
5. Exercises `SqliteSparql::Store#insert` / `#sparql` / `#count`.
6. Loads the AR concern in a tiny `ActiveRecord::Base` subclass
   against an in-memory SQLite connection and round-trips
   through `Knowledge.sparql("…")`.

Plus a `gem build sqlite-sparql.gemspec` smoke pass that produces
a `.gem` file with the host-platform `.dylib` vendored under
`vendor/<platform>/`.

---

## What 0.14.0 covers vs. doesn't

| Status | Item |
|---|---|
| **0.14.0 in scope** | `ruby/` subdirectory; `sqlite-sparql.gemspec`; `lib/sqlite_sparql.rb` (loader); `lib/sqlite_sparql/store.rb` (ergonomic wrapper); `lib/sqlite_sparql/has_rdf_triples.rb` (AR concern, optional require); `lib/sqlite_sparql/version.rb` (reads engine VERSION); `Rakefile` (`build`, `test`, `release` tasks); `test/` (minitest); host-platform `.dylib` vendoring at `gem build` time |
| **0.14.0 out of scope** | Cross-platform fat-gem binaries (mac-arm64 + mac-x86_64 + linux-x86_64 + linux-arm64 + linux-musl + windows-x86_64). Deferred to PLAN_0.14.1 or PLAN_0.15.0 — needs a CI workflow + GitHub Releases distribution. The 0.14.0 gem requires the consumer to `cargo build --release` locally (documented) until the binary-distribution plan lands. |
| **0.14.0 out of scope** | RubyGems publication (`gem push`). Land the gem in-tree first; publish only after PLAN_0.14.x or PLAN_0.15.x cross-platform binaries are ready, so consumers don't need a Rust toolchain. |
| **0.14.0 out of scope** | SPARQL HTTP endpoint (`Rack`/`Rails` middleware exposing `/sparql`). That's the next plan (PLAN_0.15.0). |

---

## Surface design

### Loader (mirrors `sqlite-vec`)

```ruby
require "sqlite3"
require "sqlite_sparql"

db = SQLite3::Database.new(":memory:")
SqliteSparql.load(db)

# Now every SQL function the cdylib registers is callable:
db.execute("SELECT rdf_insert(?, ?, ?)", ["<urn:a>", "<urn:b>", "<urn:c>"])
db.get_first_value("SELECT rdf_count()")  # => 1
```

`SqliteSparql.load(db)`:
- Sets `enable_load_extension(true)` on the connection.
- Calls `db.load_extension(SqliteSparql.path, "sqlite3_sqlitesparql_init")`.
- Idempotent — loading twice is a no-op (`db.execute_batch("SELECT rdf_count()")`
  succeeds either way; the engine's `rdf_clear` is called on first load
  to match the integration-test convention).

`SqliteSparql.path` returns the absolute path to the vendored
binary for the host platform (Gem::Platform.local). Falls back
to `ENV["SQLITE_SPARQL_CDYLIB"]` for development overrides
(matches the engine's existing `build.rs` env var).

### Ergonomic wrapper

```ruby
store = SqliteSparql::Store.new(db)

store.insert("<urn:alice>", "<urn:knows>", "<urn:bob>")
store.delete("<urn:alice>", "<urn:knows>", "<urn:bob>")
store.count                           # → Integer
store.clear                           # → true
store.sparql("SELECT ?o WHERE { <urn:alice> ?p ?o }")
  # → Array of Hashes (parsed from the engine's JSON return)
store.ask("ASK { <urn:alice> ?p ?o }")    # → Boolean
store.construct("CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }")
  # → String (N-Triples)
store.load_turtle(turtle_text)        # → Integer (count)
store.load_turtle(turtle_text, graph: "urn:g:catalogue")
store.materialise(asserted: nil, inferred: "urn:g:inferred",
                  options: {"provenance" => true, "track_dependencies" => true})
store.consistent?(asserted: nil, inferred: "urn:g:inferred")
  # → Boolean (true iff JSON return is "[]")
store.consistency_violations(asserted: nil, inferred: "urn:g:inferred")
  # → Array<Hash> of {rule, s, p, o} (since 0.13.0)
store.shacl_validate(data: "urn:g:data", shapes: "urn:g:shapes",
                     report: "urn:g:report", options: {})
  # → Integer (violation count)
store.dred_overdelete(inferred: "urn:g:inferred",
                      retracted_premises: [["<s>", "<p>", "<o>"]])
  # → Integer (over-deleted count)
```

`Store#sparql` always returns parsed JSON (`JSON.parse(...)`) —
the underlying scalar already returns JSON for SELECT. The Ruby
side hides the round-trip and lets the consumer think in
arrays of result-binding hashes.

### ActiveRecord concern (Rails)

```ruby
require "sqlite_sparql/has_rdf_triples"

class Knowledge < ApplicationRecord
  include SqliteSparql::HasRdfTriples

  rdf_triple_set do
    triple :subject_iri, :predicate_iri, :object_value
    on :create
    on :destroy
  end
end

Knowledge.rdf_store           # → SqliteSparql::Store backed by the AR connection
Knowledge.sparql("SELECT …")  # → delegates to rdf_store.sparql
Knowledge.materialise(...)    # → delegates to rdf_store.materialise

knowledge = Knowledge.create!(subject_iri: "<urn:alice>",
                              predicate_iri: "<urn:knows>",
                              object_value: "<urn:bob>")
# `after_create :sync_to_rdf_store` fires → triple lands in the store.
knowledge.destroy!
# `after_destroy :remove_from_rdf_store` fires → triple retracts.
```

The concern is **optional require** (`require "sqlite_sparql/has_rdf_triples"`)
so non-Rails consumers don't pay the ActiveRecord dependency
cost. The gemspec lists `activerecord` only under
`add_development_dependency` for testing; the runtime dep is
just `sqlite3`.

`rdf_triple_set do … end` block is a tiny DSL that records the
column names + lifecycle hooks. `on :create` / `on :destroy`
flip the auto-sync. Operators who want full manual control
skip the block entirely and use `Knowledge.rdf_store` directly.

---

## Directory layout

```
ruby/
├── Gemfile
├── Gemfile.lock                # gitignored
├── Rakefile                    # tasks: build, test, release
├── sqlite-sparql.gemspec
├── README.md
├── lib/
│   ├── sqlite_sparql.rb        # entrypoint + loader
│   └── sqlite_sparql/
│       ├── version.rb          # reads ../VERSION
│       ├── store.rb            # ergonomic wrapper
│       └── has_rdf_triples.rb  # AR concern (optional require)
├── vendor/
│   └── <platform>/             # populated at `gem build` time
│       └── libsqlite_sparql.dylib  (or .so / .dll)
└── test/
    ├── test_helper.rb
    ├── test_loader.rb
    ├── test_store.rb
    └── test_has_rdf_triples.rb
```

---

## Phase A — gem skeleton + loader

`ruby/sqlite-sparql.gemspec`:

```ruby
Gem::Specification.new do |s|
  s.name          = "sqlite-sparql"
  s.version       = File.read(File.expand_path("../VERSION", __dir__)).strip
  s.summary       = "Embedded RDF triple store and SPARQL engine for SQLite"
  s.description   = "Native SQLite extension via Oxigraph — see https://github.com/laquereric/sqlite-sparql"
  s.authors       = ["sqlite-sparql contributors"]
  s.license       = "MIT"
  s.required_ruby_version = ">= 3.0"
  s.files         = Dir["lib/**/*.rb", "vendor/**/*", "README.md"]
  s.require_paths = ["lib"]
  s.add_runtime_dependency "sqlite3", ">= 1.6"
  s.add_development_dependency "activerecord", ">= 7.0"
  s.add_development_dependency "minitest", "~> 5.0"
  s.add_development_dependency "rake", "~> 13.0"
end
```

`ruby/lib/sqlite_sparql.rb`:

```ruby
require "sqlite3"
require_relative "sqlite_sparql/version"

module SqliteSparql
  ENTRYPOINT = "sqlite3_sqlitesparql_init"

  class LoadError < StandardError; end

  def self.load(db)
    db.enable_load_extension(true)
    db.load_extension(path, ENTRYPOINT)
    db.enable_load_extension(false)
    db
  end

  def self.path
    return ENV["SQLITE_SPARQL_CDYLIB"] if ENV["SQLITE_SPARQL_CDYLIB"]

    platform = Gem::Platform.local.os
    arch = Gem::Platform.local.cpu
    ext = case platform
          when /darwin/ then "dylib"
          when /linux/  then "so"
          when /mingw|mswin|cygwin/ then "dll"
          else
            raise LoadError, "sqlite-sparql: unsupported platform #{platform}"
          end
    path = File.expand_path(
      "../vendor/#{arch}-#{platform}/libsqlite_sparql.#{ext}",
      __dir__,
    )
    unless File.exist?(path)
      raise LoadError, <<~MSG
        sqlite-sparql: no vendored binary for #{arch}-#{platform} at #{path}.
        Either:
          - Run `cargo build --release` in the engine root and re-build the gem; or
          - Set SQLITE_SPARQL_CDYLIB to point at your local build's .dylib/.so.
      MSG
    end
    path
  end
end
```

`ruby/lib/sqlite_sparql/version.rb`:

```ruby
module SqliteSparql
  VERSION = File.read(File.expand_path("../../../../VERSION", __FILE__)).strip
end
```

(`File.expand_path("../../../../VERSION", __FILE__)` resolves to
the engine repo's root `VERSION` so gem + engine stay locked.
Matches the Vv::Rails convention from
`~/Developer/.../ruby.md`'s "VERSION read from repo root.")

### Exit criteria

```
cd ruby && bundle install && rake test  # passes loader + sanity tests
```

---

## Phase B — `SqliteSparql::Store` ergonomic wrapper

`ruby/lib/sqlite_sparql/store.rb`:

```ruby
require "json"

module SqliteSparql
  class Store
    attr_reader :db

    def initialize(db)
      SqliteSparql.load(db) unless self.class.loaded?(db)
      @db = db
    end

    # Triple management
    def insert(s, p, o, graph: nil)
      sql = graph ? "SELECT rdf_insert(?, ?, ?, ?)"
                  : "SELECT rdf_insert(?, ?, ?)"
      args = graph ? [s, p, o, graph] : [s, p, o]
      @db.get_first_value(sql, args).to_i
    end

    def delete(s, p, o, graph: nil)
      sql = graph ? "SELECT rdf_delete(?, ?, ?, ?)"
                  : "SELECT rdf_delete(?, ?, ?)"
      args = graph ? [s, p, o, graph] : [s, p, o]
      @db.get_first_value(sql, args).to_i
    end

    def clear
      @db.get_first_value("SELECT rdf_clear()").to_i == 1
    end

    def count(graph: nil)
      sql = graph ? "SELECT rdf_count(?)" : "SELECT rdf_count()"
      args = graph ? [graph] : []
      @db.get_first_value(sql, args).to_i
    end

    def count_all
      @db.get_first_value("SELECT rdf_count_all()").to_i
    end

    # SPARQL
    def sparql(query)
      json = @db.get_first_value("SELECT sparql_query(?)", [query])
      JSON.parse(json)
    end

    def ask(query)
      @db.get_first_value("SELECT sparql_ask(?)", [query]).to_i == 1
    end

    def construct(query)
      @db.get_first_value("SELECT sparql_construct(?)", [query])
    end

    def update(query)
      @db.get_first_value("SELECT sparql_update(?)", [query]).to_i
    end

    # Bulk load
    def load_turtle(text, graph: nil)
      if graph
        @db.get_first_value("SELECT rdf_load_turtle_to_graph(?, ?)", [text, graph]).to_i
      else
        @db.get_first_value("SELECT rdf_load_turtle(?)", [text]).to_i
      end
    end
    def load_ntriples(text, graph: nil) = bulk_load("ntriples", text, graph)
    def load_rdfxml(text, graph: nil)   = bulk_load("rdfxml",   text, graph)

    # Reasoning / validation (since 0.9.0 / 0.11.0 / 0.12.0 / 0.13.0)
    def materialise(asserted: nil, inferred:, options: {})
      @db.get_first_value(
        "SELECT rdf_owl_rl_materialise(?, ?, ?)",
        [asserted, inferred, JSON.dump(options)],
      ).to_i
    end

    def consistency_violations(asserted: nil, inferred:, options: {})
      json = @db.get_first_value(
        "SELECT rdf_owl_rl_consistent(?, ?, ?)",
        [asserted, inferred, JSON.dump(options)],
      )
      JSON.parse(json)
    end

    def consistent?(asserted: nil, inferred:, options: {})
      consistency_violations(asserted: asserted, inferred: inferred, options: options).empty?
    end

    def shacl_validate(data: nil, shapes:, report:, options: {})
      @db.get_first_value(
        "SELECT rdf_shacl_core_validate(?, ?, ?, ?)",
        [data, shapes, report, JSON.dump(options)],
      ).to_i
    end

    def dred_overdelete(inferred:, retracted_premises:)
      @db.get_first_value(
        "SELECT rdf_dred_overdelete(?, ?)",
        [inferred, JSON.dump(retracted_premises)],
      ).to_i
    end

    private

    def bulk_load(format, text, graph)
      sql = graph ? "SELECT rdf_load_#{format}_to_graph(?, ?)"
                  : "SELECT rdf_load_#{format}(?)"
      args = graph ? [text, graph] : [text]
      @db.get_first_value(sql, args).to_i
    end

    def self.loaded?(db)
      db.get_first_value("SELECT rdf_count()") && true
    rescue SQLite3::Exception
      false
    end
  end
end
```

### Exit criteria

`test/test_store.rb` exercises every public method against an
in-memory database. ~25 assertions; all green.

---

## Phase C — ActiveRecord concern

`ruby/lib/sqlite_sparql/has_rdf_triples.rb`:

```ruby
require "active_support/concern"
require_relative "store"

module SqliteSparql
  module HasRdfTriples
    extend ActiveSupport::Concern

    class_methods do
      def rdf_store
        @_rdf_store ||= SqliteSparql::Store.new(connection.raw_connection)
      end

      def sparql(query)         = rdf_store.sparql(query)
      def ask(query)            = rdf_store.ask(query)
      def construct(query)      = rdf_store.construct(query)
      def materialise(**kwargs) = rdf_store.materialise(**kwargs)
      def consistent?(**kwargs) = rdf_store.consistent?(**kwargs)

      def rdf_triple_set(&block)
        @_rdf_triple_set_block = block
        # Lifecycle wiring deferred to first use — block is evaluated lazily.
      end
    end

    included do
      after_create  :_sqlite_sparql_sync
      after_destroy :_sqlite_sparql_retract
    end

    def rdf_store
      self.class.rdf_store
    end

    private

    def _sqlite_sparql_sync
      # Default implementation is a no-op; override sync_to_rdf_store
      # in the model OR define the DSL block via `rdf_triple_set`.
      sync_to_rdf_store if respond_to?(:sync_to_rdf_store, true)
    end

    def _sqlite_sparql_retract
      remove_from_rdf_store if respond_to?(:remove_from_rdf_store, true)
    end
  end
end
```

The concern is intentionally thin: it wires the lifecycle hooks
and provides `rdf_store` accessor. Operators define
`sync_to_rdf_store` / `remove_from_rdf_store` instance methods
to opt in to auto-sync. This avoids guessing at the
column-to-triple mapping — every domain is different.

### Exit criteria

`test/test_has_rdf_triples.rb`:

```ruby
class FakeKnowledge < ActiveRecord::Base
  self.table_name = "knowledge"
  include SqliteSparql::HasRdfTriples

  def sync_to_rdf_store
    rdf_store.insert(subject_iri, predicate_iri, object_iri)
  end

  def remove_from_rdf_store
    rdf_store.delete(subject_iri, predicate_iri, object_iri)
  end
end

# create! → triple lands in the store; destroy! → triple retracts.
```

---

## Phase D — Rakefile + native build glue

`ruby/Rakefile`:

```ruby
require "rake"
require "rake/testtask"

ROOT = File.expand_path("..", __dir__)

Rake::TestTask.new(:test) do |t|
  t.libs << "lib"
  t.libs << "test"
  t.test_files = FileList["test/test_*.rb"]
end

desc "Build the engine cdylib via cargo and vendor it for the host platform"
task :native do
  sh "cd #{ROOT} && cargo build --release"
  platform_dir = "vendor/#{Gem::Platform.local.cpu}-#{Gem::Platform.local.os}"
  mkdir_p platform_dir
  ext = case Gem::Platform.local.os
        when /darwin/ then "dylib"
        when /linux/  then "so"
        when /mingw|mswin|cygwin/ then "dll"
        end
  cp "#{ROOT}/target/release/libsqlite_sparql.#{ext}",
     "#{platform_dir}/libsqlite_sparql.#{ext}"
end

desc "Build the gem with the vendored binary"
task build: :native do
  sh "gem build sqlite-sparql.gemspec"
end

task default: :test
```

### Exit criteria

`rake native` populates `ruby/vendor/<arch>-<os>/libsqlite_sparql.{dylib,so,dll}`.
`rake build` produces `sqlite-sparql-0.14.0.gem` (or whatever
the engine VERSION says) in `ruby/`.
`rake test` passes.

---

## Phase E — tests

Under `ruby/test/`:

1. `test_loader.rb`
   - Loads into a fresh `:memory:` database; runs `rdf_count()`
     and expects 0.
   - Loading twice on the same connection: idempotent (no error).
   - `SqliteSparql.path` returns an existing file path.
2. `test_store.rb`
   - Round-trips one triple via `insert` / `count` / `delete`.
   - SPARQL SELECT round-trip; result is `Array<Hash>`.
   - SPARQL ASK returns `Boolean`.
   - Named-graph variants (`insert(s, p, o, graph: "urn:g:x")`).
   - Bulk-load Turtle → count grows.
   - `materialise` with `track_dependencies: true` → integer delta.
   - `consistent?` returns `true` on a clean graph; `false` +
     `consistency_violations` non-empty on a cax-dw graph.
   - `shacl_validate` against a contrived shapes graph → violation
     count.
   - `dred_overdelete` after a tracked materialise → integer.
3. `test_has_rdf_triples.rb`
   - Defines a `FakeKnowledge < ActiveRecord::Base` with the
     concern.
   - `create!` triggers `sync_to_rdf_store`; assert via `rdf_store.count`.
   - `destroy!` triggers `remove_from_rdf_store`; assert count drops.
4. `test_version.rb`
   - `SqliteSparql::VERSION == File.read("../../VERSION").strip`.

### Exit criteria

```
cd ruby && bundle exec rake test
```
all green. ~30 assertions across the four test files.

---

## Phase F — docs

- **`ruby/README.md`** — quick-start, installation (`cargo build
  --release` requirement until cross-platform binaries ship),
  loader pattern, Store API, AR concern, link back to the engine
  repo's README.
- **`README.md`** (engine) — add a "Ruby / Rails" subsection
  under "Rails Integration" pointing at the gem.
- **`CLAUDE.md`** — replace item #11's "Create a companion Ruby
  gem" stub with the "LANDED" entry; describe the `ruby/`
  layout.
- **`CHANGELOG.md`** — 0.14.0 entry leading with the gem layout,
  loader pattern, the `cargo build --release` prerequisite,
  and the cross-platform-binary follow-on.
- **`PLAN_0.2.0.md`** — mark 0.14.0 row.
- **`CONSUMER_REQUIREMENT_MM.md`** — note the gem becomes
  available; MM still loads via Rails 8's `extensions:` config
  key (which is what `vv-graph` does), so the gem doesn't change
  MM's wiring path. Document for completeness.
- **`CONSUMER_REQUIREMENT_VvGraph.md`** — note: `vv-graph` could
  potentially switch from raw `extensions:` to
  `SqliteSparql.load(db)` to gain the ergonomic helpers, but
  there's no consumer ask; current path stays.

---

## Phase G — commit + tag

Same as PLAN_0.11.0 / 0.12.0 / 0.13.0. Single chained `git add
… && git commit …` invocation. Push when authorised. After push,
**do not yet publish to RubyGems** — that waits on the
cross-platform binary distribution plan.

---

## Risks

- **`File.expand_path("../../../../VERSION", __FILE__)` fragility.**
  When the gem is installed via `gem install`, the gem's `lib/`
  is no longer four directories under the engine root — it lives
  under the user's RubyGems install path. Mitigation: bake the
  VERSION string into `version.rb` at `gem build` time via a
  Rakefile substitution, or `require_relative "../../VERSION"`-style
  read that has a fallback when the engine root isn't reachable.
  Phase A ships the simple read; Phase D's `rake build` task
  substitutes the literal string before packaging.
- **`Gem::Platform.local` quirks.** On Apple Silicon under
  Rosetta, `local.cpu` returns `x86_64` even though the dylib
  is arm64. Mitigation: document the override via
  `SQLITE_SPARQL_CDYLIB`. Native distribution channels (the
  follow-on plan) avoid this by shipping platform-specific gems.
- **AR concern's `raw_connection`.** ActiveRecord's
  `connection.raw_connection` returns the underlying `SQLite3::Database`
  for the `sqlite3` adapter but is undocumented and may change
  across Rails versions. Mitigation: pin tested Rails versions
  (7.x, 8.x) in the gemspec dev deps; document that the AR
  concern requires the `sqlite3` adapter and the `sqlite3` gem
  >= 1.6.
- **`enable_load_extension` permission.** Some `sqlite3` gem
  builds disable extension loading at compile time. Mitigation:
  document the requirement, add a fixed-prefix error to the
  loader that explains the rebuild needed
  (`SqliteSparql::LoadError: SQLite3 was built without
  extension-loading support…`).
- **No CI yet.** The engine repo has no GitHub Actions. The
  gem's tests run locally only. Mitigation: Phase G commits a
  minimal `.github/workflows/ruby.yml` that runs `rake test`
  on push (optional — can be a follow-on if it bloats the
  diff). Document the local-test convention.
- **Vendor directory ballooning.** `vendor/<arch>-<os>/` will
  hold a multi-MB `.dylib`. With release-mode strip + LTO it's
  smaller (target/release/libsqlite_sparql.dylib is ~10–15 MB
  on Apple Silicon, per my read of `Cargo.toml`'s `strip = true`).
  Add `vendor/` to `.gitignore` from day one; the binary is
  rebuilt by `rake native` on every `gem build`. Cross-platform
  fat-gem distribution lands them in the gem itself but not in
  git.
- **Version drift between engine and gem.** Mitigated by
  `version.rb` reading the engine's `VERSION` file at build
  time. The gem cannot diverge.

---

## Out of scope

- **Cross-platform binary distribution.** A `.github/workflows/release-binaries.yml`
  that builds for mac-arm64 / mac-x86_64 / linux-x86_64 /
  linux-arm64 / linux-musl / windows-x86_64 on tag push, uploads
  to GitHub Releases, and a separate `sqlite-sparql-<platform>`
  native gem per platform. Tied to PLAN_0.14.1 or PLAN_0.15.x.
- **RubyGems publication.** Hold until the binaries ship —
  publishing a Ruby gem that requires the consumer to have a
  Rust toolchain is a non-starter for non-devs.
- **JRuby / TruffleRuby support.** The gem assumes MRI's
  `sqlite3` gem with extension loading. Other Rubies will need
  separate work; no consumer ask.
- **RBS / Sorbet type signatures.** Could ship `sig/` directory
  with RBS, but adds maintenance overhead. Defer.
- **SPARQL HTTP endpoint middleware.** That's PLAN_0.15.0.
- **A Rails generator (`bin/rails g sqlite_sparql:install`)**
  that drops a `config/initializers/sqlite_sparql.rb` + a
  migration. Future polish; not in 0.14.0.

---

## Re-numbering downstream milestones

| Version | Topic |
|---|---|
| 0.14.0 | `sqlite-sparql-ruby` gem wrapper (this file) |
| 0.14.x | Cross-platform binary distribution via GitHub Releases (separate plan) |
| 0.15.0 | SPARQL HTTP endpoint middleware |
| Deferred | Persistent RocksDB; differential dataflow; SHACL-SPARQL; SHACL Rules native pass |

Unchanged from PLAN_0.13.0's renumber.
