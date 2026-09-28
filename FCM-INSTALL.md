# Fallout Chat Mod package import

Use **Mods → Install mod** or the usual drag-and-drop window to add a production
FCM package. Quick Configuration recognizes the visible HUD ZIP, its ZFE or
xScal provider folder, and the optional Server Bridge folder inside Windows
setup, portable, or Linux overlay ZIPs. It reviews the FCM file changes in the
normal import flow before installing. It never runs or installs the overlay
executable or Linux installer from those ZIPs. The visible HUD and invisible
Server Bridge cannot be active together.

Select the Fallout 76 game and active INI directories in the Quick Configuration
profile. Close Fallout 76, then import the downloaded ZIP or folder. If exactly
one native provider (ZFE or xScal) is already installed, Quick Configuration
selects its matching HUD folder automatically. If neither is installed, choose
the provider in the import dialog. If HUDModLoader's BA2 or INI is missing, the
same dialog offers to download and install it. Quick Configuration gets the newest main ZIP
listed by the official Nexus Mods API for each missing requirement. Nexus Mods
Premium accounts can download directly; other accounts use the official Mod
Manager Download button on the opened Nexus page. Sign in to Nexus Mods in
Quick Configuration first. The fork registers itself as the `nxm://` handler if
needed for that button. Existing unrecognized `dxgi.dll` files block the
install so another proxy DLL is never overwritten. The provider, loader and FCM
changes are reviewed and backed up together before applying.

The imported package supplies its version: the HUD's
production installation metadata or BA2 stamp, and the bridge's production
`BUILD.json` and BA2 checksum. The installer validates the selected provider and
required files. It does not fetch a different FCM release or silently replace
the package you selected. An incomplete FCM package fails instead of entering the generic mod
deployment path. To remove the active FCM mod, use **Mods → More → Remove FCM
mod**.

The installer backs up every changed file under the app configuration directory's
`fcm-backups/` folder and displays that backup path when complete. It merges
`Data/hudmodloader.ini` and the active `Fallout76Custom.ini` archive list without
replacing unrelated entries. A visible HUD install creates `Data/FCMChat.ini` or
the ZFE fragment only if absent, preserving edited copies. On xScal it merges
only `[Chat] enabled` and `relayEndpoint` from the supplied package example.
If HUDModLoader later ships an FCM entry by default, the installer recognizes
existing FCM lines, removes duplicates, and leaves one line for the chosen mode.
The bridge never changes provider chat settings. Removal clears the FCM BA2 and
its loader/archive entries; it keeps editable FCM configuration files so user
customizations are not lost. Restore a backup manually if an earlier setup is
needed. A changed file between preview and apply blocks the operation.

Quick Configuration's normal INI save now checks whether any file changed on
disk since it was loaded. If so, it refuses to overwrite the newer file and
asks you to reload. Normal saves write only changed keys and preserve unrelated
lines, comments, and existing file permissions. After a guided install, the UI
reloads its INI and resource list state. The fork retains Quick Configuration's
upstream update check and installation controls. Installing an upstream update can replace this fork's
FCM integration, so reinstall a fork build afterward if that happens.
Local unsigned builds and fork CI use `src-tauri/tauri.fcm-unsigned.conf.json`
to omit updater signature artifacts while keeping the upstream updater in the app.

On Linux systems using NVIDIA, a blank or partly rendered window may require
starting the AppImage with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. The included
`scripts/launch-appimage-linux.sh` sets this when it detects NVIDIA, honors an
existing setting, and removes stale AppImage environment inherited from other
applications. Point a desktop launcher's `Exec` to this script followed by the
absolute AppImage path, or use the script from a terminal.

## Local replacement and acceptance

On Linux, back up the current AppImage and the app configuration folder before
removing the AppImage. A built AppImage can replace the old file; keep the same
Quick Configuration profile and inspect its detected game/INI paths before
installing FCM. On Windows, back up the profile and game files before uninstalling
the existing Quick Configuration build and installing the unsigned fork build.

For each provider and each FCM choice, compare the game files before deployment,
after deployment, after a game launch and exit, and after reopening Quick
Configuration. Confirm exactly one FCM BA2 and loader entry, one matching archive
entry, and byte-for-byte preservation of unrelated INI lines. Then repeat an
install, switch modes, remove, and restore the prior setup. Native chat and
Server Bridge behavior require separate in-game verification.

## Headless Linux acceptance

The opt-in Rust acceptance test runs the same package detection, preview,
planning, and apply code as **Mods → Install mod** without opening the app.
Provide the downloaded production Linux overlay ZIP and HUD ZIP, plus the game
and active Proton INI directories:

```bash
export FCM_LINUX_ZIP=/path/to/linux-overlay.zip
export FCM_HUD_ZIP=/path/to/production-hud.zip
export FCM_GAME_DIR=/path/to/Fallout76
export FCM_INI_DIR='/path/to/My Games/Fallout 76'
cargo test --manifest-path src-tauri/Cargo.toml headless_production_linux_install -- --ignored --nocapture --test-threads=1
```

This first run copies the current HUD and provider files into a temporary
fixture. It installs the bridge, repeats that install, switches to the HUD,
checks the selected BA2, loader and archive entries, and restores the fixture.
To repeat against the live game, close Fallout 76, save a separate copy of every
affected file, and rerun with `FCM_TEST_LIVE=1` and `FCM_BACKUP_DIR` set to a
persistent directory outside the game. The test restores the original live
bytes even if a validation check fails; the separate copy covers an interrupted
process or failed restoration. Each apply also writes its normal backup
manifest under `FCM_BACKUP_DIR`. Check the live files against the separate copy
afterward. This test does not exercise the graphical file picker or native chat.
