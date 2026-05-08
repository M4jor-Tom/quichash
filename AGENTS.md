# AGENTS.md — QuicHash

## Commands

| Action | Command |
|--------|---------|
| Build (debug) | `cargo build` |
| Build (release) | `cargo build --release` |
| All tests | `cargo test` |
| Single test | `cargo test <name>` |
| Run binary | `cargo run -- <args>` (binary name is `hash`, not `quichash`) |

Release profile: fat LTO, strip, panic=abort, codegen-units=1.

## Architecture

All source in `src/`. The crate produces both a library (`quichash`) and a binary (`hash`):

| File | Crate | Responsibility |
|------|-------|----------------|
| `lib.rs` | lib | Module declarations, re-exports public API |
| `main.rs` | bin | ~40-line entry point, stdin detection, output dispatch |
| `cli.rs` | lib | Clap derive commands: hash, scan, verify, compare, analyze, dedup, benchmark, list |
| `commands.rs` | lib | Orchestration layer — calls engines, formats output as `String` |
| `hash.rs` | lib | `Hasher` trait + algorithm registry — implement this to add algorithms |
| `scan.rs` | lib | Parallel directory scan (rayon + jwalk), progress bars |
| `verify.rs` | lib | Compare live dir against stored database |
| `compare.rs` | lib | Two-database diff (changed/moved/added/removed) |
| `dedup.rs` | lib | Duplicate detection by hash |
| `database.rs` | lib | Parse/write standard, hashdeep, JSON, LZMA-compressed (.xz) formats |
| `error.rs` | lib | Centralized error types with path/operation context |
| `wildcard.rs` | lib | `*`, `?`, `[...]` expansion |
| `ignore_handler.rs` | lib | `.hashignore` gitignore-style matching |
| `path_utils.rs` | lib | Path canonicalization cache |

Engines (`ScanEngine`, `VerifyEngine`) use builder-style configuration.
Use `quichash::commands` for binary-style orchestration or use individual engines directly for library consumption.

## Testing

- **Inline unit tests** — inside `src/` modules
- **Integration tests** — `tests/` directory (2 files):
  - `regression_cli_contracts_test.rs` — CLI JSON contract + edge-case filename round-trips
  - `international_filenames_test.rs` — Unicode filename handling
- Tests use `tempfile` and invoke the binary via `env!("CARGO_BIN_EXE_hash")`
- Some filename tests are `#[cfg(unix)]` only
- CI (`.github/workflows/ci.yml`) skips `cargo test` for cross-compiled targets (FreeBSD, musl, ARM Linux cross, ARM Windows)

## Key Conventions

- **Binary is `hash`**, not `quichash` — configured in `Cargo.toml` `[[bin]]` block
- **Library is `quichash`** — configured in `Cargo.toml` `[lib]` block
- **Default algorithm: BLAKE3**, default mode: parallel (rayon)
- **`--hdd` flag** switches to sequential processing (old mechanical drives)
- **`-f` (fast mode)** samples 300MB — only works on files, not stdin/text
- **Wildcard patterns must be quoted** (e.g. `"*.txt"`) to prevent shell expansion
- **`.hashignore`** is read only by `scan` and `dedup`, NOT by `verify`. It is looked up in the scanned directory and its ancestors only, NOT in subdirectories during traversal. Only `.hashignore` is read (not `.gitignore`).
- **Database format**: `<hash>  <algorithm>  <mode>  <filepath>` (standard) or CSV (hashdeep)
- New hash algorithms: implement `Hasher` trait in `hash.rs` and register it

## CI / Release

- **CI** triggers on push/PR to main, master, develop — 10-target matrix, tests run on native builds only
- **Release** triggers on `v*` tags — builds release binaries for all targets, creates draft GitHub release
- Cross-compilation uses `cross` for FreeBSD and musl targets
- ARM Linux cross-compilation requires gcc cross-toolchains installed
- Docker image published to `ghcr.io/vyrti/hash` (Dockerfile not in this repo)

## License

Dual MIT / Apache-2.0. Contributions use inbound=outbound licensing.
