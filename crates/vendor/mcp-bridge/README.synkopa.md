# Vendored: tauri-plugin-mcp-bridge 0.13.2

Vendored (MIT, see LICENSE) so the debug automation bridge refuses
Origin-bearing WebSocket handshakes. A browser page cannot suppress the
Origin header its WebSocket API always sends; automation clients send none.
Upstream 0.13.2 accepts any handshake and dispatches `execute_js`, which
would let a same-machine browser page drive the app. Upstream issue:
hypothesi/mcp-server-tauri#38.

Our only delta is the handshake check in `src/websocket.rs`
(`accept_async` → `accept_hdr_async` + Origin rejection), marked with
"VENDORED PATCH" in the source. The bridge itself is registered only in
debug builds and only when `SYNKOPA_MCP_BRIDGE` is set, bound to 127.0.0.1.
Re-vendor from crates.io when upstream publishes the origin check.

The vendored crate is a workspace member so its handshake regression tests
(`src/websocket.rs` `origin_tests`: an Origin-bearing handshake is refused
with 403 and a masked frame written after it is never read; a no-Origin
client upgrades and holds a live session) run with `cargo test --workspace`,
and so clippy and rustfmt gate it like the rest of the house. Upstream
source was reformatted and lint-annotated for those gates; the behavioural
delta is the marked handshake check only.
