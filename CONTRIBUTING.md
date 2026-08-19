# Contributing to Acelus

## Two rules that are not negotiable

### 1. Genuine authentication only

Acelus authenticates through Microsoft and verifies game ownership. It will never gain an offline
mode, an authentication server override, a cracked or "no premium" login path, or any other means of
running the game without a valid entitlement.

Concretely, the following will be closed without review:

- Offline or "cracked" account support
- Any change that makes the entitlement check optional, non-fatal, or bypassable
- Weakening the entitlement JWT signature verification
- Redistributing Minecraft game jars or assets rather than fetching them from Mojang's CDN

If you want a launcher without these restrictions, Acelus is not that project and will not become it.

### 2. No comments in code

Code is expected to explain itself through naming, small functions, and types that carry meaning. If
a comment feels necessary, that is a signal the code needs restructuring — do that instead.

Not comments, and therefore allowed:

- Licence headers where a licence requires them
- `#![doc]` and `///` rustdoc on public API items
- `//go:build` and `//go:generate` directives
- `#[allow(...)]` and similar compiler attributes
- `description` fields in JSON and YAML schemas

Everything else — explanatory comments, section banners, `TODO` notes, commented-out code — gets
removed in review.

## Toolchains

| Component | Toolchain | Purpose |
|---|---|---|
| `core/` | Rust (stable, see `rust-toolchain.toml`) | The engine: auth, download, verification, launch |
| `cli/` | Go 1.24+ | Command line client and the optional sync server |
| `native/` | Zig 0.16+ | Static C-ABI shims for OS process control and hardware probing |
| `ui/` | Node 20+ with Tauri v2 | Desktop application, from phase 3 |

Zig doubles as the cross-compilation linker via `zig cc`, driven by `cargo xtask`.

## Before opening a pull request

```
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
(cd cli && go vet ./... && go test ./...)
(cd native/procguard && zig build test)
```

## Boundaries between languages

`schema/rpc.json` is the single source of truth for the daemon interface. Both the Rust server and
the Go client are generated from it. Change the schema first, regenerate, then implement — never
hand-edit generated code on one side only.

Zig code is exposed to Rust as a static library with a C ABI, bound in `core/acelus-native-sys`.
Keep that surface small and stable; it is not a place for logic that could live in Rust.

## Security

Report anything that could compromise an account or execute untrusted code by email rather than a
public issue. Mod jars are arbitrary code from the internet — treat every parser and extraction path
as handling hostile input, because it is.
