# Quick Configuration 2 — Fallout Chat Mod fork

This fork of [Quick Configuration 2](https://github.com/FelisDiligens/QuickConfiguration2) is a focused installer for the optional Fallout Chat Mod in-game packages. The original Quick Configuration source is MIT licensed; see [LICENSE](LICENSE).

The app installs **either** the visible FCM HUD **or** the invisible FCM Server Bridge. It does not install or update the Fallout Chat Mod desktop overlay. The game must be closed while applying a package.

## Use

1. Select or create a game profile with the Fallout 76 installation and active INI directory.
2. Download a published production FCM HUD or overlay ZIP. On the main **Install mod** screen, choose the ZIP/folder or drop it onto the app window. A full Linux or Windows overlay ZIP is accepted; the overlay executable is ignored.
3. Review the package version, provider, and exact file changes before applying. The installer saves a backup and reports its location.
4. To switch between HUD and Server Bridge, import the other production ZIP. To remove FCM, use **Remove FCM** on the same screen.

The app detects ZFE or xScal and HUDModLoader. When either is missing, it offers the appropriate official Nexus Mods download. Sign in under **Nexus Mods login** and choose a **Downloads** folder first. If neither ZFE nor xScal is present, choose the provider you want. See [FCM-INSTALL.md](FCM-INSTALL.md) for package rules, file preservation, recovery, and headless acceptance.

This fork keeps profiles and the existing app configuration format so an existing Quick Configuration installation can be used. It has no game tweak editor, generic mod deployment, Archive2 tools, screenshot gallery, or upstream auto-updater. It does not fetch a different FCM version when you import a package; the ZIP you select supplies the installed version.

## Build and test

See [BUILD.md](BUILD.md). CI runs Rust and UI tests on Linux and Windows and produces an unsigned Windows installer for native validation. This fork has not been published as a signed release.
