# Developing OpenLogi

This document covers the local development workflow for OpenLogi. For end-user
build instructions, see the [README](../README.md).

## Toolchain

- Stable Rust (Edition 2024, MSRV 1.85)
- macOS: Xcode 16+ with the optional **Metal Toolchain** component (required by
  GPUI's `gpui_macos` build script to compile shaders)
- `create-dmg` for packaging (`brew install create-dmg`); `cargo-bundle` is
  installed automatically by `scripts/package-macos.sh`

## Building from source

CLI:

```sh
git clone https://github.com/AprilNEA/OpenLogi
cd OpenLogi
cargo run -p openlogi-cli --release -- list
```

Desktop app:

```sh
cargo run -p openlogi-gui --release
```

On macOS the desktop binary is launched from inside a throwaway
`target/dev/OpenLogi.app` — a Cargo `runner` wired in `.cargo/config.toml`
(`scripts/cargo-run-macos.sh`). This makes the dev build show the real
**OpenLogi** name in the menu bar and the app icon in the Dock; a bare
`cargo run` binary has no bundle, so macOS would otherwise fall back to the
`openlogi-gui` executable name and a generic icon. The binary is hardlinked in
(no copy) and the icon is generated once by `scripts/macos-icns.sh`. The runner
is a transparent passthrough for everything else (the CLI, tests); set
`OPENLOGI_DEV_BUNDLE=0` to launch the raw `openlogi-gui` binary instead.

To install the CLI binary on `PATH`:

```sh
cargo build -p openlogi-cli --release
cp target/release/openlogi ~/.local/bin/
```

## Using devenv (macOS)

The repo's `devenv.nix` provisions a Nix-based dev shell with sccache, the
stable Rust toolchain, and the env overrides GPUI needs. It exposes tasks that
mirror CI and packaging:

```sh
devenv tasks run openlogi:gui      # run the desktop app
devenv tasks run openlogi:check    # fmt + clippy + tests (run before committing)
devenv tasks run openlogi:dmg      # build the macOS DMG
```

The first time you `cd` into the repo after pulling a change to `devenv.nix`,
**reload direnv** so the new env vars (`DEVELOPER_DIR`, `SDKROOT`, the PATH
filter that strips Nix's `xcbuild` xcrun stub) take effect:

```sh
direnv reload    # or: exit your shell and `cd` back in
```

Without that, GPUI's `gpui_macos` build script can't find Apple's `metal`
shader compiler, and link errors about missing `_write` / `_sysconf` /
`_waitpid` symbols show up because the Nix `apple-sdk-14.4` stub doesn't
expose `libSystem` the way Apple's real linker wants.

## Project layout

```
crates/
  openlogi-core/    types, config (TOML), paths, button + action catalog — no HID, no async
  openlogi-hid/     hidpp + async-hid: enumerate(), DPI (0x2201) and SmartShift (0x2111) writes
  openlogi-assets/  device-render registry schema + cached HTTP fetch from assets.openlogi.org
  openlogi-hook/    macOS CGEventTap mouse hook + Accessibility + frontmost-app detection
  openlogi-cli/     the `openlogi` binary
  openlogi-gui/     the `openlogi-gui` binary — GPUI + gpui-component
```

## Pre-commit checklist

Before committing, the following must pass:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Equivalent to `devenv tasks run openlogi:check`.

## Packaging the macOS DMG

```sh
bash scripts/package-macos.sh          # → target/release/OpenLogi.dmg
```

Environment overrides:

- `OPENLOGI_BUNDLE_ASSETS=1` — bundle every device render into the `.app` for a
  fully offline build (default: fetched on demand at first launch).
- `OPENLOGI_SIGN_IDENTITY=<identity>` — codesign the `.app` and `.dmg` with the
  given Developer ID.
