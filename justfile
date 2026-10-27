# Task runner for an image a plugin runs as its upstream. `just` with no argument lists these.
default:
    @just --list

# Turn on the repository's own git hooks. Once per clone.
hooks:
    git config core.hooksPath .githooks
    @echo "hooks on: .githooks/commit-msg, .githooks/pre-push"

# The gates that need nothing but this tree. The image is built and proved in CI.
ci: hooks typos links

# Spell check.
typos:
    typos

# Link check.
links:
    lychee --no-progress .
