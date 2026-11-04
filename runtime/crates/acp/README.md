# Runtime ACP execution

This crate owns adapter Registry discovery, installation receipts, Node/npm
preparation, executable resolution and local stdio process groups. Its service
composes the shared `aow-acp-client` with a node-owned transport. The daemon's
`acpd` crate exposes management HTTP and native ACP WebSocket connections.

Conversation state, official ACP protocol handling, permissions and history are
implemented once in [the shared client](../../../common/crates/acp-client/README.md).
Server uses that library through its `runtime-client` SDK and cannot invoke this
crate's adapter preparation or process APIs.

The shared client's `backport/sources.json` records upstream provenance for both
shared client code and node adapter code. Integration tests here exercise local
execution using fixtures in `common/fixtures/acp/`.
