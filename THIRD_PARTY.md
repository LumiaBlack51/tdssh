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

Other dependencies are downloaded by Cargo, pinned in `Cargo.lock`, and retain
their respective licenses. iroh 1.3.0 supplies QUIC/TLS, NAT traversal,
relay fallback and signed device discovery. No new cryptography is implemented.
