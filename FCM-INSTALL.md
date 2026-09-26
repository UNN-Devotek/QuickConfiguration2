# Fallout Chat Mod package import

Use **Mods → Install mod** or the usual drag-and-drop window to add a production
FCM package. Quick Configuration recognizes the visible HUD ZIP, its ZFE or
xScal provider folder, and the optional Server Bridge folder inside Windows
setup, portable, or Linux overlay ZIPs. It reviews the FCM file changes in the
normal import flow before installing. It never runs or installs the overlay
executable or Linux installer from those ZIPs. The visible HUD and invisible
Server Bridge cannot be active together.

Install HUDModLoader and one native provider (ZFE or xScal) first. Select the
Fallout 76 game and active INI directories in the Quick Configuration profile.
Close Fallout 76, import the downloaded ZIP or folder, and review the exact file
paths before applying. The imported package supplies its version: the HUD's
production installation metadata or BA2 stamp, and the bridge's production
`BUILD.json` and BA2 checksum. The installer validates the selected provider and
required files. It does not fetch a release or silently replace the package you
selected. An incomplete FCM package fails instead of entering the generic mod
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
lines and comments. After a guided install, the UI reloads its INI and resource
list state. The fork does not contact or install updates from the upstream
Quick Configuration release channel.

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
