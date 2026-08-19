# Acelus

A Minecraft: Java Edition launcher that authenticates for real and treats your disk, your time, and
your crash logs with more respect than the alternatives.

> Acelus is not an official Minecraft product. It is not approved by or associated with Mojang or
> Microsoft.

## Status

The command line launcher works end to end. It resolves a version from Mojang, downloads and
verifies every file, provisions the matching Java runtime, and starts the game under supervision.

Signing in additionally needs an Azure application approved by Mojang — see
[docs/AZURE_SETUP.md](docs/AZURE_SETUP.md). Everything up to and including install works without one.

```
acelus create Survival 1.21.11
acelus install survival
acelus login
acelus launch survival
```

Measured against Mojang on 1.12.2, on one machine:

| | time | disk added |
|---|---|---|
| First install (client, 39 libraries, natives, assets, Java 8 runtime) | 25.5 s | 403 MiB |
| Second instance of the same version | **0.43 s** | **2.1 MiB** |

The second instance costs only its unpacked natives. Everything else is hardlinked out of the
content addressed store.

## What it does differently

**One copy of everything on disk.** Libraries, assets, jars and natives live in a content addressed
store and are linked into instances by reflink or hardlink. Twenty instances of the same version
cost the disk space of one. Existing launchers duplicate heavily per instance.

**Instances are reproducible.** Every instance carries an `acelus.lock` pinning each artifact by
content hash, byte size and origin URL. `acelus verify` re-checks an entire tree against it. The
lockfile is plain JSON and diffs cleanly in git, so an instance can be shared and rebuilt
bit-for-bit.

**Crashes get diagnosed, not just displayed.** A rule engine reads the exit code, the crash report
and the log, then names the cause: which mod's mixin failed to apply, which dependency is missing,
which loader version is wrong, whether the heap ran out, whether the GPU driver gave up. Every other
launcher hands you a stack trace and wishes you luck.

**Conflicts surface at install time.** Dependency resolution across Modrinth and CurseForge runs
through a real solver, so incompatible mods are caught when you add them, with an explanation of
why, rather than at first launch.

**The game runs sandboxed.** Mods are arbitrary code. Acelus confines the game process — user
namespaces on Linux, `sandbox-exec` on macOS, a job object with a restricted token on Windows — with
the filesystem scoped to the instance.

**No telemetry. No advertising. No account required beyond the one you already own.**

## Genuine accounts only

Acelus implements the real Microsoft authentication chain: Microsoft OAuth, Xbox Live, XSTS,
Minecraft services, then an entitlement check whose JWT signature is verified against Mojang's
public key. Launch is refused without a valid entitlement and profile.

There is no offline mode and there will not be one. See [CONTRIBUTING.md](CONTRIBUTING.md).

Game files are never redistributed; they are fetched from Mojang's CDN at install time.

## Building

Requires Rust (stable), Go 1.24+, and Zig 0.16+. On Linux, credential storage uses the Secret
Service, so `libdbus-1-dev` and `pkg-config` must be present.

```
cargo build --release
(cd cli && go build ./cmd/acelus)
(cd native/procguard && zig build)
```

Zig also cross-compiles the native shims for every supported target from any host:

```
cd native/procguard && zig build -Dtarget=x86_64-windows-gnu
```

Running against real accounts additionally requires an Azure application approved by Mojang —
see [docs/AZURE_SETUP.md](docs/AZURE_SETUP.md). The auth chain is fully testable against recorded
fixtures without one.

## Architecture

| Component | Language | Responsibility |
|---|---|---|
| `core/` | Rust | Auth, metadata, downloads, integrity, the object store, launching |
| `cli/` | Go | The `acelus` command, and an optional self-hosted instance sync server |
| `native/` | Zig | Process supervision and hardware probing as static C-ABI libraries |
| `ui/` | TypeScript, Tauri | Desktop application (phase 3) |

Rust owns the engine because it parses untrusted JSON, archives and jars pulled from the internet.
Go owns the client and the sync server, which are separate processes regardless. Zig owns the two
places where a small freestanding native library beats both, and provides `zig cc` as the
cross-compilation linker.

The daemon and every client talk JSON-RPC defined by `schema/rpc.json`, so the command line and the
graphical client cannot drift apart.

## Roadmap

- **Phase 0** — repository, toolchains, schemas, CI — *done*
- **Phase 1** — Microsoft auth, vanilla download and verify, launching from the command line — *done*
- **Phase 2** — Fabric, Quilt, NeoForge, Forge; Modrinth and CurseForge; modpack import and export
- **Phase 3** — desktop application
- **Phase 4** — crash diagnosis, sandboxing, instance sync
- **Phase 5** — signed and notarized packages, auto-update

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
