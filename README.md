# Acelus

A Minecraft: Java Edition launcher that authenticates for real and treats your disk, your time, and
your crash logs with more respect than the alternatives.

> Acelus is not an official Minecraft product. It is not approved by or associated with Mojang or
> Microsoft.

## Status

Installing works end to end. Acelus resolves a version from Mojang, downloads and verifies every
file against its published digest, provisions the matching Java runtime, and writes a lockfile.
Fabric and Quilt instances are resolved from the loaders' own metadata and merged into the same
lockfile. All of this is measured below on real hardware.

The sign in chain runs against real Microsoft accounts through OAuth, Xbox Live and XSTS, and
stops at the last step, where `api.minecraftservices.com` answers **403** until Mojang approves the
Azure application — see [docs/AZURE_SETUP.md](docs/AZURE_SETUP.md). Launching is gated behind a
signature-verified entitlement and so waits on that approval; it has not yet run against a real
account.

```
acelus create Survival 1.21.11
acelus install survival
acelus login
acelus launch survival
```

For a modded instance, name the loader when creating it. Bare takes the newest build published for
that Minecraft version; `=` pins one.

```
acelus create Modded 1.21.11 --fabric
acelus create Pinned 1.21.11 --quilt=0.29.2
```

Measured against Mojang, cold store, then a second instance of the same version:

| | | time | disk added |
|---|---|---|---|
| 1.12.2 | first install — client, 39 libraries, natives, assets, Java 8 | 25.5 s | 403 MiB |
| | second instance | **0.43 s** | **2.1 MiB** |
| 1.21.11 + Fabric | second instance, btrfs | **0.16 s** | **0 MiB** |

A repeated instance costs only what has to be written fresh. On 1.12.2 that is the unpacked
natives, which is why it is not quite free; versions from 1.19 on name their natives as ordinary
classpath entries and unpack nothing, so a second instance costs nothing measurable at all.

Reproducing this needs the right instrument. `du` deduplicates by inode, which sees hardlinks but
not reflinks — on btrfs or XFS the store clones extents into a fresh inode, and `du` will report
every instance at full size while the filesystem stores one copy. Measure free space instead:

```
before=$(df -B1 --output=avail ~/.local/share/acelus | tail -1)
acelus create Second 1.21.11 --fabric && acelus install second
after=$(df -B1 --output=avail ~/.local/share/acelus | tail -1)
echo $(( (before - after) / 1024 / 1024 )) MiB
```

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

Most people should not build anything: install a package from
[Releases](https://github.com/jaysyrk/Acelus/releases). Windows gets a setup executable, Linux a
deb and an AppImage, macOS a dmg. Neither Windows nor macOS builds are signed yet, so both will
warn on first launch.

To build it yourself:

```
cargo build --release
(cd cli && go build -o ../target/release/acelus ./cmd/acelus)
```

The desktop application builds separately, because it needs its own toolchain and bundles the
daemon beside itself:

```
cd ui && npm install
npm run tauri dev
npm run tauri build
```

On Linux that also needs `webkit2gtk` and `gtk3` development packages. Set
`ACELUS_DEFAULT_CLIENT_ID` when building if you are distributing to people who should never see
an Azure portal; see [docs/AZURE_SETUP.md](docs/AZURE_SETUP.md).

That leaves `acelus` and `acelusd` side by side in `target/release`. The client starts the
daemon itself, looking beside its own binary before falling back to PATH, so putting that one
directory on PATH is enough:

```
export PATH="$PWD/target/release:$PATH"
```

Zig also cross-compiles the native shims for every supported target from any host:

```
cd native/procguard && zig build -Dtarget=x86_64-windows-gnu
```

Running against real accounts additionally requires an Azure application approved by Mojang —
see [docs/AZURE_SETUP.md](docs/AZURE_SETUP.md). Without one the chain still runs to its last step
and every stage is testable against recorded fixtures.

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
- **Phase 2** — Fabric and Quilt — *done*; NeoForge and Forge; Modrinth and CurseForge; modpack
  import and export
- **Phase 3** — desktop application
- **Phase 4** — crash diagnosis, sandboxing, instance sync
- **Phase 5** — signed and notarized packages, auto-update

## Licence

Copyright © 2026 jaysyrk.

Acelus is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version. It is distributed in the hope that it will be
useful, but WITHOUT ANY WARRANTY, without even the implied warranty of MERCHANTABILITY or FITNESS
FOR A PARTICULAR PURPOSE. See [LICENSE](LICENSE) for the full terms.

Minecraft is a trademark of Mojang Synergies AB. Acelus is not affiliated with, endorsed by, or
supported by Mojang or Microsoft.
