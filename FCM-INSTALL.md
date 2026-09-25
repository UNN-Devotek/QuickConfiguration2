# Fallout Chat Mod guided installation

The **Mods → FCM installer** tab installs one optional in-game mod: the visible
FCM Chat Widget or the invisible FCM Server Bridge. It does not install or change
the separate Fallout Chat Mod desktop overlay. The two in-game mods cannot be
active together.

Install HUDModLoader and one native provider (ZFE or xScal) first. Select the
Fallout 76 game and active INI directories in the Quick Configuration profile.
Close Fallout 76, select an FCM option, and review the exact file paths before
applying. The installer reads the production HUD version from
`https://falloutchatmod.com/api/releases`. It reads published production bridge
ZIP assets from the `UNN-Devotek/FCM-Fallout-Chat-Mod` GitHub releases API; a
newer overlay release without a bridge asset does not replace the last bridge
release. It rejects drafts, prereleases, wrong hosts, mismatched versions and
invalid packages. GitHub's SHA-256 asset digest is mandatory for bridge ZIPs.
The current HUD download is a unified archive with separate ZFE and xScal
folders; the installer reads only the selected provider folder.

The installer backs up every changed file under the app configuration directory's
`fcm-backups/` folder and displays that backup path when complete. It merges
`Data/hudmodloader.ini` and the active `Fallout76Custom.ini` archive list without
replacing unrelated entries. A visible HUD install creates `Data/FCMChat.ini` or
the ZFE fragment only if absent, preserving edited copies. On xScal it merges
only `[Chat] enabled` and `relayEndpoint` from the published package example.
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
