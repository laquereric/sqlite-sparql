require_relative "lib/sqlite_sparql/version"

Gem::Specification.new do |s|
  s.name          = "sqlite-sparql"
  s.version       = SqliteSparql::VERSION
  s.summary       = "Embedded RDF triple store and SPARQL engine for SQLite"
  s.description   = <<~DESC
    A SQLite loadable extension that embeds the Oxigraph RDF/SPARQL engine.
    Once loaded, every SQLite connection gains native SQL functions for
    inserting, querying, and serialising RDF triples (including SPARQL 1.1
    SELECT / ASK / CONSTRUCT / UPDATE), plus OWL 2 RL reasoning, SHACL Core
    validation, and DRed incremental over-deletion.

    This gem ships the compiled cdylib and a small Ruby loader that mirrors
    sqlite-vec's pattern.
  DESC
  s.homepage      = "https://github.com/laquereric/sqlite-sparql"
  s.authors       = ["sqlite-sparql contributors"]
  s.license       = "MIT"
  s.required_ruby_version = ">= 3.0"

  s.files = Dir[
    "lib/**/*.rb",
    "vendor/**/*",
    "README.md",
    "../LICENSE-MIT",
    "../LICENSE-APACHE",
  ]
  s.require_paths = ["lib"]

  s.add_runtime_dependency "sqlite3", ">= 1.6"

  s.add_development_dependency "activerecord", ">= 7.0"
  s.add_development_dependency "minitest", "~> 5.0"
  s.add_development_dependency "rake", "~> 13.0"
end
