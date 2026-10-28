# Task runner for lemonfiber/lemonfiber-door. `just` with no argument lists these.
default:
    @just --list

# Turn on the repository's own git hooks. Once per clone.
hooks:
    git config core.hooksPath .githooks
    @echo "hooks on: .githooks/commit-msg, .githooks/pre-push"

# The gates that need nothing but this tree, in CI's order. The image, built and run
# under the posture plugin-door's manifest declares, runs in CI.
ci: hooks fmt clippy test coverage deny typos links

# The door's format.
fmt:
    cargo fmt --check

# The door's lints, every warning an error.
clippy:
    cargo clippy --all-targets --locked -- -D warnings

# The door's tests.
test:
    cargo test --locked

# Every line of the door the tests can reach, reached.
coverage:
    cargo llvm-cov --locked --ignore-filename-regex "$(cat .config/coverage-skipped)" --fail-under-lines 100

# The door's dependencies, held to `deny.toml`.
deny:
    cargo deny --log-level warn check

# Spell check.
typos:
    typos

# Link check.
links:
    lychee --no-progress .
