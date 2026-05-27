# frozen_string_literal: true

module SqliteSparql
  # Pinned at packaging time to match the engine's `VERSION` file at the
  # repo root. The `test_version.rb` integration test reads both and
  # asserts equality so drift is caught immediately. The Rakefile's
  # `bump_version` task (Phase D) updates both in lockstep.
  VERSION = "0.14.0"
end
