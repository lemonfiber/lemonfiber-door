# AGENTS.md — lemonfiber-door

> **Start at the roadmap and board on [lemonfiber.app](https://lemonfiber.app),
> rendered from the report of where every unreleased version stands. Then the
> rules** every repository shares:
> [working in the repositories](https://github.com/lemonfiber/spec/blob/main/50-governance/working-in-the-repositories.md)
> and [the rules for agents](https://github.com/lemonfiber/spec/blob/main/50-governance/ai-contributors.md).
> This file holds only what is true of this repository.

## What this repo is

The guarded door: `ghcr.io/lemonfiber/door`, which `plugin-door` runs (`OPS-R93`). One
Rust crate that serves the household's ports, 8096 plain and 8920 encrypted, and
speaks `stream.guard@1` to the core on 8081, as its own upstream
([ADR-0056](https://github.com/lemonfiber/spec/blob/main/00-overview/decisions/0056-the-guarded-door-is-a-rust-service-that-checks-the-request-it-forwards.md)).
See the spec:
[`30-repos/lemonfiber-door.md`](https://github.com/lemonfiber/spec/blob/main/30-repos/lemonfiber-door.md).

| Module | What it holds |
| --- | --- |
| `target` | A request's target as it arrived, decoded once, and the normal form |
| `rules` | The envelope compiled, what it decides about one request, the token it reads, and the question it asks |
| `client` | Which client a request came from |
| `holding` | The envelope the core applies, kept owner-only under `/state`, and the certificate's digest |
| `serving` | Asking the question or the pass service, and forwarding the target checked |
| `listening` | The plain and encrypted ports, HTTP/1.1 with upgrades and HTTP/2 |
| `tls` | The certificate the core writes, read again when it changes |
| `speaking` | `stream.guard@1` on the contract port |

## The rules you cannot break

- **It writes under `/state` alone.** `/config`, where the core writes its key and its
  certificate, is read-only to it; the envelope it is applied is kept in `/state`.
- **It fails closed.** Until the core applies an envelope it answers `503` to every
  household request, and an envelope it cannot compile is refused and never held.
- **It reads the target as it arrived and forwards that target.** Every pattern is
  matched against the path as it arrived and decoded once; a byte or live-stream path
  carries no `%` at all; what goes on is the checked target, byte for byte, with only
  `lfpass` taken off and a token put in the query where the declaration says.
- **It names no media server's routes.** What it enforces is the declaration in the
  envelope; nothing here knows Jellyfin.
- **It believes `X-Forwarded-For` from the envelope's `proxy` alone**, takes that
  proxy's rightmost entry, and tells the media server that one client.
- **A request carries one member's token**: it is read from the bearer header, the
  header it is presented in, each token parameter and each token header the declaration
  names; the same token in several of them is one token, and different tokens, one place
  carrying a token twice, another spelling and another shape are refused. A value in the
  presentation header its template does not hold is no token where the question goes to
  the media server, and refused where it goes to the adapter.
- **An item found in a query value is read decoded once**: the parameter carried once
  under its declared spelling, its value matched whole, holding no `+`, no `%` once
  decoded, no dot segment, doubled slash, `;`, backslash, fragment or control character,
  and the item made of unreserved bytes.
- **A question answers only for reading**: a request on a byte or live-stream path that
  is asked about, with any method but `GET` and `HEAD`, is refused.
- **Every query parameter the token reaches the media server in counts**, the one it is
  presented in among them, and a token in one holding `%` or `+` is refused.
- **The adapter is asked in the byte path's space**: `may?space={space}&item={item}`, and
  a byte path naming no space is never asked about there.
- **A question is built from parsed parts**: its scheme and host are those the envelope
  names, its target a path from the root; every item and space is made of plain bytes,
  every token put in a query is percent-encoded, and every pattern is anchored here,
  whatever the declaration writes, so nothing a request carries adds a parameter, a
  segment or a host to what is asked.
- **A question to the adapter carries the member's token alone**, presented as the
  declaration presents it to the media server, and no other header the request carried;
  a request carrying an `Authorization` header that is not that token is refused.
- **No `unwrap`, `expect`, `panic` or indexing** outside tests. The lints in
  `Cargo.toml` are the core's, and every line the tests can reach is reached.
- **Every base image is pinned by tag and digest** (ADR-0025).
- **The manifest is `plugin-door`'s.** Its posture and its recordings live there, and a
  change here is proved against that manifest's `main`.
- **No Python here.** Every check is this crate's or spec's shared workflows'.
