# lemonfiber-door

The image of the guarded door in a [lemonfiber](https://github.com/lemonfiber/lemonfiber) stack:
Caddy, rebuilt without the file capability Caddy's own image carries on its binary, so
it starts with no added kernel capability. It serves the media server's byte routes over TLS, asks the core about every item before it serves one, and refuses what no question about an item covers.

This repository builds `ghcr.io/lemonfiber/door`. You do not run it yourself:
[`plugin-door`](https://github.com/lemonfiber/plugin-door) runs it as the upstream its
adapter speaks lemonfiber's `stream.guard` contract for, and pins it by digest.

## What is here

| Path | What it is |
| --- | --- |
| `Dockerfile` | The image, from Caddy's own pinned by digest |
| `config/door/Caddyfile` | The configuration the image starts with |

## How it is published

Every commit on `main` builds `linux/amd64` and `linux/arm64` under one index,
tagged `main-<commit>`, and moves `plugin-door`'s pin to it on a pull request there.
The release train never tags this repository: `plugin-door`'s release pins the image
its `main` holds. The specification has the details:
[`30-repos/lemonfiber-door.md`](https://github.com/lemonfiber/spec/blob/main/30-repos/lemonfiber-door.md).

## Contributing and security

Read the [contributing guide](https://github.com/lemonfiber/spec/blob/main/50-governance/contributing.md)
before opening a pull request. Report a vulnerability as the
[security policy](https://github.com/lemonfiber/.github/blob/main/SECURITY.md)
describes, not in a public issue.

## Licence

[Hippocratic License 3.0](LICENSE).
