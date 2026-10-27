# AGENTS.md — lemonfiber-door

> **Start at the roadmap and board on [lemonfiber.app](https://lemonfiber.app),
> rendered from the report of where every unreleased version stands. Then the
> rules** every repository shares:
> [working in the repositories](https://github.com/lemonfiber/spec/blob/main/50-governance/working-in-the-repositories.md)
> and [the rules for agents](https://github.com/lemonfiber/spec/blob/main/50-governance/ai-contributors.md).
> This file holds only what is true of this repository.

## What this repo is

The image of the guarded door: `ghcr.io/lemonfiber/door`, which `plugin-door` runs as its upstream
(`OPS-R93`). See the spec:
[`30-repos/lemonfiber-door.md`](https://github.com/lemonfiber/spec/blob/main/30-repos/lemonfiber-door.md).

## The rules you cannot break

- **The image starts with no added kernel capability.** The `Dockerfile` removes the
  file capability Caddy's binary carries, and runs as a non-root user.
- **Every base image is pinned by tag and digest** (ADR-0025).
- **The manifest is `plugin-door`'s.** Its posture, its starting files and its
  recordings live there, and a change here is proved against that manifest's `main`.
- **No Python here.** Every check is spec's shared workflows'.
