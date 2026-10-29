# Third-party code

`src/network.rs` and `src/network/` derive from TwoDrive's isolated dev
transport at commit `507fef1cb4e8ec1639d0c15e95401368f0415dfc` (branch `dev`,
retrieved 2026-10-20):
https://github.com/LumiaBlack51/twodrive/tree/507fef1cb4e8ec1639d0c15e95401368f0415dfc/experiments/twodrive-dev/src

Copyright (c) 2026 TwoDrive contributors. MIT license retained in
`licenses/TwoDrive-MIT.txt`. The WebDAV engine, cloud providers, desktop app,
credentials, pairing state and file caches are not included. Adaptations add
strict protection of QUIC UDP sockets and omit unused HTTP engine helpers.

`vendor/netwatch/` is netwatch 0.19.3, from n0-computer/net-tools commit
`0030e57fc70981895bb86212ff0f059cc41c8a4e`, package path `netwatch`:
https://github.com/n0-computer/net-tools/tree/0030e57fc70981895bb86212ff0f059cc41c8a4e/netwatch

MIT OR Apache-2.0; both upstream license files are retained. The local patch
exports a once-only socket protector and invokes it before every UDP bind,
including rebinds. The normalized crate manifest omits upstream integration test
entries whose fixtures are outside the packaged dependency. The remaining
library and platform sources are retained to support native Linux and Windows.
`tests/udp_policy.rs` checks that protection failures cannot create an
unprotected replacement socket. This patch is necessary because iroh 1.3.0's
public bind API selects a source IP without exposing OS interface socket options.

`vendor/iroh/` is the iroh 1.3.0 crates.io source archive, SHA256
`885787b892b5e2507c701f132ecbd45d2bad4bbb75157a19427087c16dadb833`,
upstream commit `0072d7d84b233f9e7185eb676f049beaf557ac03` (package path
`iroh`). The archive was verified before extraction. Its redundant `Cargo.lock`
and `.cargo_vcs_info.json` are omitted; upstream sources and manifest are retained.
MIT OR Apache-2.0; root license files from that exact upstream commit are retained,
along with the archive's BSD license notice.

The local patch deduplicates pending path-open retries and caps the queue at 64
distinct candidates, discarding the oldest candidate on overflow. It follows
the mechanism fixed in [upstream #4551](https://github.com/n0-computer/iroh/pull/4551)
and adds a hard bound for address churn as suggested in
[upstream #4509](https://github.com/n0-computer/iroh/issues/4509).
`tests/path_retry_queue.rs` exercises the same helper used by the dependency,
including retries across several connections. See `docs/incidents.md` for the
local incident evidence and the limits of this fix.

Other dependencies are downloaded by Cargo, pinned in `Cargo.lock`, and retain
their respective licenses. iroh 1.3.0 supplies QUIC/TLS, NAT traversal,
relay fallback and signed device discovery. No new cryptography is implemented.
