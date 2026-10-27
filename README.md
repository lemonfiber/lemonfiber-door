# lemonfiber-door

The guarded door in a [lemonfiber](https://github.com/lemonfiber/lemonfiber) stack: the
one service the household reaches the media server's bytes through. Before a stream, a
picture or a file goes out, it asks the media server whether the member presenting the
request may see that item, or asks lemonfiber's pass service about a live stream, and it
refuses what no question covers. What it enforces is the declaration lemonfiber applies
to it over the `stream.guard` contract, which it speaks itself.

This repository builds `ghcr.io/lemonfiber/door`. You do not run it yourself:
[`plugin-door`](https://github.com/lemonfiber/plugin-door) runs it and pins it by digest.

## What is here

| Path | What it is |
| --- | --- |
| `src/` | The door: one Rust crate |
| `Dockerfile` | Its image, distroless and non-root |

It serves the household on 8096, and on 8920 with the certificate lemonfiber writes
into its configuration directory, read-only to it, and speaks `stream.guard@1` to
lemonfiber on 8081. The envelope lemonfiber applies is kept in `/state`, the one
directory it writes.

## Checks

```sh
just ci
```

Its format, its lints, its tests with every line they can reach reached, its dependency
policy, spelling and links. The image is built and run under the posture
`plugin-door`'s manifest declares in CI.

## How it is published

A commit on `main` that changes what the image is built from builds `linux/amd64` and
`linux/arm64` under one index, tagged `main-<commit>`, and moves `plugin-door`'s pin to
it on a pull request there. The release train never tags this repository. The
specification has the details:
[`30-repos/lemonfiber-door.md`](https://github.com/lemonfiber/spec/blob/main/30-repos/lemonfiber-door.md).

## Contributing and security

Read the [contributing guide](https://github.com/lemonfiber/spec/blob/main/50-governance/contributing.md)
before opening a pull request. Report a vulnerability as the
[security policy](https://github.com/lemonfiber/.github/blob/main/SECURITY.md)
describes, not in a public issue.

## Licence

[Hippocratic License 3.0](LICENSE).
