# Third-party source provenance

- Cold Clear 2: https://github.com/MinusKelvin/cold-clear-2
- Revision: `ed8b19327b6bd1410ddd873d8611485bd45d8fae`
- Author: MinusKelvin (Mark Carlson)
- License: MIT OR Apache-2.0. Both licenses are preserved in `third_party/cold-clear-2/`.
- The bundled `engines/cold-clear-2.exe` was built from that revision with Rust 1.98.1, `cargo build --release`, on Windows x64 MSVC. No algorithm changes were applied.
- Vendored Cargo.toml adds an empty `[workspace]` solely to make the independent engine buildable inside the outer project. Its build script reports `unknown` without Git metadata; this does not change the pinned source revision.
- SRS true-rotation base cells and kick offset tables in `game-state`/`movement` follow the cited engine's data.rs/movegen.rs and are used under its MIT license.
- Protocol reference: https://github.com/tetris-bot-protocol/tbp-spec (text/0000-mvp.md).
- Other dependencies and exact versions are recorded in Cargo.lock and retain their respective upstream licenses.
