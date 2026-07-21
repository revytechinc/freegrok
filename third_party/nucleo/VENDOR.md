Vendored from https://github.com/helix-editor/nucleo
rev: 5b74652e482f7c07d827f18c6d21e7540c242c69 (short 5b74652)

Reason: FreeBSD ports cargo sets CARGO_FREEBSD_PORTS_SKIP_GIT_UPDATE=1, so git
dependencies cannot be resolved during port builds. Path-vendoring matches the
sqlite-vec FreeBSD pattern.

Nested [workspace] was removed so this tree is a normal path crate under
grok-build (matcher remains a path dep of nucleo).
