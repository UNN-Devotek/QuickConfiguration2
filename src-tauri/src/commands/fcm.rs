use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use tauri::State;
use uuid::Uuid;
use zip::ZipArchive;

use super::errors::CommandResult;

mod import;

#[cfg(test)]
mod acceptance;

#[tauri::command]
#[specta::specta]
pub fn fcm_detect_import(paths: Vec<String>) -> CommandResult<bool> {
    Ok(import::detect_import(&paths)?)
}

#[tauri::command]
#[specta::specta]
pub fn fcm_preview_import(
    game_path: String,
    ini_path: String,
    ini_prefix: String,
    paths: Vec<String>,
    state: State<'_, FcmPlans>,
) -> CommandResult<FcmPreview> {
    Ok(import::preview_import(
        &game_path,
        &ini_path,
        &ini_prefix,
        &paths,
        &state,
    )?)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FcmAction {
    InstallHud,
    InstallBridge,
    Remove,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FcmPackageInfo {
    pub version: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FcmChange {
    pub path: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FcmPreview {
    pub token: String,
    pub action: FcmAction,
    pub provider: String,
    pub installed: Option<String>,
    pub package: Option<FcmPackageInfo>,
    pub changes: Vec<FcmChange>,
}

struct FileChange {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    description: String,
}

struct Plan {
    changes: Vec<FileChange>,
}

#[derive(Default)]
pub struct FcmPlans(Mutex<HashMap<String, Plan>>);

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn zip_file(zip: &mut ZipArchive<Cursor<Vec<u8>>>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("Missing {name} in package"))?;
    ensure!(
        entry.size() <= limit && entry.is_file(),
        "Invalid {name} in package"
    );
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "Invalid {name} size");
    Ok(bytes)
}

fn package_files(
    action: FcmAction,
    info: &FcmPackageInfo,
    bytes: Vec<u8>,
    provider: &str,
) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    let mut files = Vec::new();
    match action {
        FcmAction::InstallHud => {
            let readme = String::from_utf8(zip_file(&mut zip, "README.txt", 200_000)?)?;
            ensure!(
                readme
                    .lines()
                    .any(|line| line.trim() == format!("Version: {}", info.version))
                    && readme
                        .lines()
                        .any(|line| line.trim() == "Package provider: unified"),
                "HUD package version or provider mismatch"
            );
            let folder = if provider == "zfe" {
                "ZFE (Install for ZFE only)"
            } else {
                "xScal (Install for xScal only)"
            };
            let data = format!("{folder}/Data (drag the contents into data folder)");
            let ba2 = zip_file(
                &mut zip,
                &format!("{data}/FCMChatWidget.ba2"),
                20 * 1024 * 1024,
            )?;
            ensure!(
                ba2.starts_with(b"BTDX")
                    && ba2
                        .windows(info.version.len())
                        .any(|window| window == info.version.as_bytes()),
                "Invalid HUD BA2 or version stamp"
            );
            files.push((PathBuf::from("Data/FCMChatWidget.ba2"), ba2));
            files.push((
                PathBuf::from("Data/FCMChat.ini"),
                zip_file(&mut zip, &format!("{data}/FCMChat.ini"), 100_000)?,
            ));
            if provider == "zfe" {
                files.push((
                    PathBuf::from("Data/ZFE/TextChat/fragments/FCMChatWidget.ini"),
                    zip_file(
                        &mut zip,
                        &format!("{data}/ZFE/TextChat/fragments/FCMChatWidget.ini"),
                        100_000,
                    )?,
                ));
            } else {
                files.push((
                    PathBuf::from("xscal.ini.example"),
                    zip_file(&mut zip, &format!("{folder}/xscal.ini"), 10_000)?,
                ));
            }
        }
        FcmAction::InstallBridge => {
            let build: serde_json::Value =
                serde_json::from_slice(&zip_file(&mut zip, "BUILD.json", 10_000)?)?;
            ensure!(
                build["version"] == info.version && build["target"] == "prod",
                "Bridge package version or target mismatch"
            );
            let ba2 = zip_file(&mut zip, "Data/FCMServerBridge.ba2", 2 * 1024 * 1024)?;
            ensure!(ba2.starts_with(b"BTDX"), "Invalid bridge BA2");
            if let Some(expected) = build["ba2Sha256"].as_str() {
                ensure!(hex_digest(&ba2) == expected, "Bridge BA2 checksum mismatch");
            }
            files.push((PathBuf::from("Data/FCMServerBridge.ba2"), ba2));
        }
        FcmAction::Remove => bail!("Remove does not use a package"),
    }
    Ok(files)
}

fn game_running() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.flatten() {
                if !entry
                    .file_name()
                    .to_string_lossy()
                    .chars()
                    .all(|c| c.is_ascii_digit())
                {
                    continue;
                }
                let cmd = fs::read(entry.path().join("cmdline")).unwrap_or_default();
                if cmd.split(|byte| *byte == 0).any(|part| {
                    part.rsplit(|byte| *byte == b'/')
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case(b"Fallout76.exe"))
                }) {
                    return true;
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Fallout76.exe", "/FO", "CSV", "/NH"])
            .output()
        {
            if String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("\"fallout76.exe\"")
            {
                return true;
            }
        }
    }
    false
}

fn provider(game: &Path) -> Result<String> {
    ensure!(
        game.join("Fallout76.exe").is_file(),
        "Select the Fallout 76 game directory"
    );
    ensure!(
        game.join("Data/HUDModLoader.ba2").is_file(),
        "Install HUDModLoader first"
    );
    let dll_path = game.join("dxgi.dll");
    ensure!(
        fs::metadata(&dll_path)
            .context("Install ZFE or xScal first")?
            .len()
            <= 100 * 1024 * 1024,
        "Provider DLL is unexpectedly large"
    );
    let dll = fs::read(dll_path)?;
    let dll = String::from_utf8_lossy(&dll).to_ascii_lowercase();
    let xscal = dll.contains("xscalchatv1");
    let zfe = dll.contains("zfe-chat-v1");
    match (xscal, zfe) {
        (true, false) => {
            ensure!(
                game.join("xscal.ini").is_file(),
                "xScal requires xscal.ini beside the game"
            );
            Ok("xscal".to_owned())
        }
        (false, true) => Ok("zfe".to_owned()),
        _ => bail!("Could not identify exactly one supported provider in dxgi.dll"),
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    if path.exists() {
        Ok(Some(fs::read(path)?))
    } else {
        Ok(None)
    }
}

fn add_change(
    changes: &mut Vec<FileChange>,
    path: PathBuf,
    after: Option<Vec<u8>>,
    description: &str,
) -> Result<()> {
    let before = read_optional(&path)?;
    if before != after {
        changes.push(FileChange {
            path,
            before,
            after,
            description: description.to_owned(),
        });
    }
    Ok(())
}

fn loader_text(input: &str, action: FcmAction) -> String {
    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<&str> = input
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| {
            !is_fcm_loader_entry(line, "FCMChatWidget")
                && !is_fcm_loader_entry(line, "FCMServerBridge")
        })
        .collect();
    match action {
        FcmAction::InstallHud => lines.push("FCMChatWidget"),
        FcmAction::InstallBridge => lines.push("FCMServerBridge"),
        FcmAction::Remove => {}
    }
    format!(
        "{}{}",
        lines.join(newline),
        if lines.is_empty() { "" } else { newline }
    )
}

fn is_fcm_loader_entry(line: &str, name: &str) -> bool {
    let entry = line.trim();
    entry.eq_ignore_ascii_case(name) || entry.eq_ignore_ascii_case(&format!("{name}.swf"))
}

pub(super) fn ini_update(
    input: &str,
    section: &str,
    key: &str,
    value: Option<&str>,
) -> Result<String> {
    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = input
        .lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect();
    let mut section_start = None;
    let mut section_end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        if line.trim().eq_ignore_ascii_case(&format!("[{section}]")) {
            ensure!(section_start.is_none(), "Duplicate [{section}] section");
            section_start = Some(index);
        } else if section_start.is_some()
            && line.trim().starts_with('[')
            && line.trim().ends_with(']')
        {
            section_end = index;
            break;
        }
    }
    let Some(start) = section_start else {
        if let Some(value) = value {
            if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(format!("{key}={value}"));
        }
        return Ok(format!(
            "{}{}",
            lines.join(newline),
            if lines.is_empty() { "" } else { newline }
        ));
    };
    let matches: Vec<usize> = ((start + 1)..section_end)
        .filter(|index| {
            lines[*index]
                .split_once('=')
                .is_some_and(|(candidate, _)| candidate.trim().eq_ignore_ascii_case(key))
        })
        .collect();
    ensure!(matches.len() <= 1, "Duplicate {key} key in [{section}]");
    match (matches.first().copied(), value) {
        (Some(index), Some(value)) => lines[index] = format!("{key}={value}"),
        (Some(index), None) => {
            lines.remove(index);
        }
        (None, Some(value)) => lines.insert(section_end, format!("{key}={value}")),
        (None, None) => {}
    }
    Ok(format!(
        "{}{}",
        lines.join(newline),
        if lines.is_empty() { "" } else { newline }
    ))
}

fn ini_value(input: &str, section: &str, key: &str) -> Result<Option<String>> {
    let mut inside = false;
    let mut found = None;
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            inside = trimmed.eq_ignore_ascii_case(&format!("[{section}]"));
        } else if inside
            && let Some((candidate, value)) = line.split_once('=')
            && candidate.trim().eq_ignore_ascii_case(key)
        {
            ensure!(found.is_none(), "Duplicate {key} key in [{section}]");
            found = Some(value.trim().to_owned());
        }
    }
    Ok(found)
}

fn archive_text(input: &str, action: FcmAction) -> Result<String> {
    let current = ini_value(input, "Archive", "sResourceArchive2List")?.unwrap_or_default();
    let mut entries: Vec<String> = current
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|item| {
            !["FCMChatWidget.ba2", "FCMServerBridge.ba2"]
                .iter()
                .any(|name| item.eq_ignore_ascii_case(name))
        })
        .map(ToOwned::to_owned)
        .collect();
    if action != FcmAction::Remove {
        if !entries
            .iter()
            .any(|name| name.eq_ignore_ascii_case("HUDModLoader.ba2"))
        {
            entries.push("HUDModLoader.ba2".to_owned());
        }
        entries.push(
            match action {
                FcmAction::InstallHud => "FCMChatWidget.ba2",
                _ => "FCMServerBridge.ba2",
            }
            .to_owned(),
        );
    }
    ini_update(
        input,
        "Archive",
        "sResourceArchive2List",
        Some(&entries.join(",")),
    )
}

fn current_mode(loader: &str) -> Result<Option<String>> {
    let hud = loader
        .lines()
        .any(|line| is_fcm_loader_entry(line, "FCMChatWidget"));
    let bridge = loader
        .lines()
        .any(|line| is_fcm_loader_entry(line, "FCMServerBridge"));
    Ok(match (hud, bridge) {
        (true, true) => Some("HUD and Server Bridge (conflict)".to_owned()),
        (true, false) => Some("HUD".to_owned()),
        (false, true) => Some("Server Bridge".to_owned()),
        (false, false) => None,
    })
}

fn make_plan(
    game: &Path,
    ini_dir: &Path,
    prefix: &str,
    action: FcmAction,
    info: Option<&FcmPackageInfo>,
    package: Vec<(PathBuf, Vec<u8>)>,
) -> Result<(Plan, FcmPreview)> {
    ensure!(!game_running(), "Close Fallout 76 before changing mods");
    ensure!(
        prefix == "Fallout76" || prefix == "Project76",
        "Unsupported INI prefix"
    );
    ensure!(
        game.join("Fallout76.exe").is_file(),
        "Select the Fallout 76 game directory"
    );
    let selected_provider = if action == FcmAction::Remove {
        "not required".to_owned()
    } else {
        provider(game)?
    };
    ensure!(
        ini_dir.is_dir(),
        "Select the active Fallout 76 INI directory"
    );
    let custom = ini_dir.join(format!("{prefix}Custom.ini"));
    let custom_before =
        fs::read_to_string(&custom).with_context(|| format!("Cannot read {}", custom.display()))?;
    let loader = game.join("Data/hudmodloader.ini");
    let loader_before = read_optional(&loader)?
        .map(String::from_utf8)
        .transpose()?
        .unwrap_or_default();
    let installed = current_mode(&loader_before)?;
    let mut changes = Vec::new();
    let expected_ba2 = match action {
        FcmAction::InstallHud => "FCMChatWidget.ba2",
        FcmAction::InstallBridge => "FCMServerBridge.ba2",
        FcmAction::Remove => "",
    };
    for ba2 in ["FCMChatWidget.ba2", "FCMServerBridge.ba2"] {
        if ba2 != expected_ba2 {
            let path = game.join("Data").join(ba2);
            if path.is_file() {
                add_change(&mut changes, path, None, "Remove the other FCM BA2")?;
            }
        }
    }
    for (relative, bytes) in package {
        if relative == Path::new("xscal.ini.example") {
            let path = game.join("xscal.ini");
            let before = fs::read_to_string(&path)?;
            let example = String::from_utf8(bytes)?;
            let enabled = ini_value(&example, "Chat", "enabled")?
                .context("HUD package has no xScal Chat enabled value")?;
            let endpoint = ini_value(&example, "Chat", "relayEndpoint")?
                .context("HUD package has no xScal relay endpoint")?;
            let merged = ini_update(&before, "Chat", "enabled", Some(&enabled))?;
            let merged = ini_update(&merged, "Chat", "relayEndpoint", Some(&endpoint))?;
            add_change(
                &mut changes,
                path,
                Some(merged.into_bytes()),
                "Merge xScal chat settings",
            )?;
        } else {
            let path = game.join(&relative);
            if relative
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
                && path.exists()
            {
                continue;
            }
            add_change(&mut changes, path, Some(bytes), "Install FCM package file")?;
        }
    }
    let loader_after = loader_text(&loader_before, action);
    add_change(
        &mut changes,
        loader,
        Some(loader_after.into_bytes()),
        "Select one FCM loader child",
    )?;
    let custom_after = archive_text(&custom_before, action)?;
    add_change(
        &mut changes,
        custom,
        Some(custom_after.into_bytes()),
        "Merge the archive list",
    )?;
    let token = Uuid::new_v4().to_string();
    let preview = FcmPreview {
        token,
        action,
        provider: selected_provider,
        installed,
        package: info.cloned(),
        changes: changes
            .iter()
            .map(|change| FcmChange {
                path: change.path.display().to_string(),
                description: change.description.clone(),
            })
            .collect(),
    };
    Ok((Plan { changes }, preview))
}

#[tauri::command]
#[specta::specta]
pub fn fcm_preview_remove(
    game_path: String,
    ini_path: String,
    ini_prefix: String,
    state: State<'_, FcmPlans>,
) -> CommandResult<FcmPreview> {
    let (plan, preview) = make_plan(
        Path::new(&game_path),
        Path::new(&ini_path),
        &ini_prefix,
        FcmAction::Remove,
        None,
        Vec::new(),
    )?;
    state.0.lock()?.insert(preview.token.clone(), plan);
    Ok(preview)
}

#[derive(Serialize)]
struct BackupEntry {
    original: String,
    saved: Option<String>,
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("File has no parent")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("File has no parent")?)?;
    temp.write_all(bytes)?;
    temp.persist(path)?;
    Ok(())
}

fn apply_plan(plan: Plan, backup_root: &Path) -> Result<String> {
    ensure!(!game_running(), "Close Fallout 76 before changing mods");
    for change in &plan.changes {
        ensure!(
            read_optional(&change.path)? == change.before,
            "{} changed after preview; preview again",
            change.path.display()
        );
    }
    let backup = backup_root.join(format!("fcm-{}", Uuid::new_v4()));
    fs::create_dir_all(&backup)?;
    let mut entries = Vec::new();
    for (index, change) in plan.changes.iter().enumerate() {
        let saved = if let Some(before) = &change.before {
            let name = index.to_string();
            fs::write(backup.join(&name), before)?;
            Some(name)
        } else {
            None
        };
        entries.push(BackupEntry {
            original: change.path.display().to_string(),
            saved,
        });
    }
    fs::write(
        backup.join("manifest.json"),
        serde_json::to_vec_pretty(&entries)?,
    )?;
    let mut applied = Vec::new();
    for change in &plan.changes {
        let result = if let Some(after) = &change.after {
            atomic_write(&change.path, after)
        } else if change.path.exists() {
            fs::remove_file(&change.path).map_err(Into::into)
        } else {
            Ok(())
        };
        if let Err(error) = result {
            for old in applied.into_iter().rev() {
                let old: &FileChange = old;
                if let Some(before) = &old.before {
                    let _ = atomic_write(&old.path, before);
                } else {
                    let _ = fs::remove_file(&old.path);
                }
            }
            return Err(error.context("FCM install failed; previous files were restored"));
        }
        applied.push(change);
    }
    Ok(backup.display().to_string())
}

#[tauri::command]
#[specta::specta]
pub fn fcm_apply(token: String, state: State<'_, FcmPlans>) -> CommandResult<String> {
    let plan = state
        .0
        .lock()?
        .remove(&token)
        .context("Preview expired; preview again")?;
    let backup_root = crate::utils::paths::get_config_path()
        .context("Cannot find app configuration directory")?
        .join("fcm-backups");
    let result = apply_plan(plan, &backup_root).map_err(super::errors::CommandError::from);
    if let Err(ref error) = result {
        log::error!("FCM install: {error}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_merge_preserves_unrelated_entries() -> Result<()> {
        let before = "[Archive]\r\nsResourceArchive2List=Other.ba2,FCMChatWidget.ba2,HUDModLoader.ba2\r\n[Display]\r\nbFoo=1\r\n";
        let after = archive_text(before, FcmAction::InstallBridge)?;
        assert!(after.contains("Other.ba2,HUDModLoader.ba2,FCMServerBridge.ba2"));
        assert!(after.contains("[Display]\r\nbFoo=1"));
        assert_eq!(archive_text(&after, FcmAction::InstallBridge)?, after);
        Ok(())
    }

    #[test]
    fn loader_switch_keeps_other_children() {
        let before = "ImprovedBars\nFCMChatWidget\n";
        assert_eq!(
            loader_text(before, FcmAction::InstallBridge),
            "ImprovedBars\nFCMServerBridge\n"
        );
    }

    #[test]
    fn loader_defaults_with_existing_fcm_lines_are_deduplicated() -> Result<()> {
        let before = "; HUDModLoader defaults\r\nImprovedBars\r\nFCMChatWidget\r\nFCMChatWidget.swf\r\nFCMServerBridge\r\n";
        assert_eq!(
            current_mode(before)?.as_deref(),
            Some("HUD and Server Bridge (conflict)")
        );
        assert_eq!(
            loader_text(before, FcmAction::InstallHud),
            "; HUDModLoader defaults\r\nImprovedBars\r\nFCMChatWidget\r\n"
        );
        assert_eq!(
            loader_text(before, FcmAction::InstallBridge),
            "; HUDModLoader defaults\r\nImprovedBars\r\nFCMServerBridge\r\n"
        );
        Ok(())
    }

    #[test]
    fn provider_detection_ignores_compatibility_names() -> Result<()> {
        let fixture = tempfile::tempdir()?;
        let game = fixture.path();
        fs::create_dir_all(game.join("Data"))?;
        fs::write(game.join("Fallout76.exe"), b"")?;
        fs::write(game.join("Data/HUDModLoader.ba2"), b"BTDX")?;
        fs::write(game.join("xscal.ini"), b"[Chat]\nenabled=true\n")?;
        fs::write(game.join("dxgi.dll"), b"XSCALCHATV1 GetZFERuntimeInfo")?;
        assert_eq!(provider(game)?, "xscal");
        fs::write(game.join("dxgi.dll"), b"zfe-chat-v1 xScal compatibility")?;
        assert_eq!(provider(game)?, "zfe");
        fs::write(game.join("dxgi.dll"), b"XSCALCHATV1 zfe-chat-v1")?;
        assert!(provider(game).is_err());
        Ok(())
    }

    #[test]
    fn published_unified_hud_layout_selects_one_provider() -> Result<()> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let data = [
            (
                "README.txt",
                "Version: 2.10.125\nPackage provider: unified\n",
            ),
            (
                "ZFE (Install for ZFE only)/Data (drag the contents into data folder)/FCMChatWidget.ba2",
                "BTDX zfe 2.10.125",
            ),
            (
                "ZFE (Install for ZFE only)/Data (drag the contents into data folder)/FCMChat.ini",
                "[FCMChat]\nopenKey=INSERT\n",
            ),
            (
                "ZFE (Install for ZFE only)/Data (drag the contents into data folder)/ZFE/TextChat/fragments/FCMChatWidget.ini",
                "[TextChat]\nenabled=true\n",
            ),
            (
                "xScal (Install for xScal only)/Data (drag the contents into data folder)/FCMChatWidget.ba2",
                "BTDX xscal 2.10.125",
            ),
            (
                "xScal (Install for xScal only)/Data (drag the contents into data folder)/FCMChat.ini",
                "[FCMChat]\nopenKey=INSERT\n",
            ),
            (
                "xScal (Install for xScal only)/xscal.ini",
                "[Chat]\nenabled=true\nrelayEndpoint=wss://falloutchatmod.com/relay\n",
            ),
        ];
        for (name, contents) in data {
            writer.start_file(name, options)?;
            writer.write_all(contents.as_bytes())?;
        }
        let bytes = writer.finish()?.into_inner();
        let info = FcmPackageInfo {
            version: "2.10.125".to_owned(),
            source: String::new(),
        };
        let zfe = package_files(FcmAction::InstallHud, &info, bytes.clone(), "zfe")?;
        let xscal = package_files(FcmAction::InstallHud, &info, bytes, "xscal")?;
        assert_eq!(zfe[0].1, b"BTDX zfe 2.10.125");
        assert_eq!(xscal[0].1, b"BTDX xscal 2.10.125");
        assert_eq!(zfe.len(), 3);
        assert_eq!(xscal.len(), 3);
        Ok(())
    }

    #[test]
    fn changed_file_blocks_apply() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.ini");
        fs::write(&path, "old")?;
        let plan = Plan {
            changes: vec![FileChange {
                path: path.clone(),
                before: Some(b"old".to_vec()),
                after: Some(b"new".to_vec()),
                description: "test".to_owned(),
            }],
        };
        fs::write(&path, "external edit")?;
        assert!(apply_plan(plan, dir.path()).is_err());
        assert_eq!(fs::read_to_string(path)?, "external edit");
        Ok(())
    }

    #[test]
    fn install_switch_remove_preserves_other_settings() -> Result<()> {
        let fixture = tempfile::tempdir()?;
        let game = fixture.path().join("game");
        let ini = fixture.path().join("ini");
        fs::create_dir_all(game.join("Data"))?;
        fs::create_dir_all(&ini)?;
        fs::write(game.join("Fallout76.exe"), b"")?;
        fs::write(game.join("dxgi.dll"), b"XSCALCHATV1")?;
        fs::write(
            game.join("xscal.ini"),
            b"[Chat]\nenabled=false\n[Other]\nkeep=yes\n",
        )?;
        fs::write(game.join("Data/HUDModLoader.ba2"), b"BTDX")?;
        fs::write(
            game.join("Data/hudmodloader.ini"),
            b"ImprovedBars\nFCMChatWidget\n",
        )?;
        fs::write(game.join("Data/FCMChatWidget.ba2"), b"BTDX old")?;
        let custom = ini.join("Fallout76Custom.ini");
        fs::write(&custom, b"[Archive]\nsResourceArchive2List=Other.ba2,HUDModLoader.ba2,FCMChatWidget.ba2\n[Other]\nkeep=yes\n")?;
        let info = FcmPackageInfo {
            version: "0.2.8".to_owned(),
            source: "test".to_owned(),
        };
        let (plan, preview) = make_plan(
            &game,
            &ini,
            "Fallout76",
            FcmAction::InstallBridge,
            Some(&info),
            vec![(
                PathBuf::from("Data/FCMServerBridge.ba2"),
                b"BTDX new".to_vec(),
            )],
        )?;
        assert_eq!(preview.installed.as_deref(), Some("HUD"));
        apply_plan(plan, fixture.path())?;
        assert!(!game.join("Data/FCMChatWidget.ba2").exists());
        assert_eq!(
            fs::read_to_string(game.join("Data/hudmodloader.ini"))?,
            "ImprovedBars\nFCMServerBridge\n"
        );
        assert!(
            fs::read_to_string(&custom)?.contains("Other.ba2,HUDModLoader.ba2,FCMServerBridge.ba2")
        );
        let (repeat, _) = make_plan(
            &game,
            &ini,
            "Fallout76",
            FcmAction::InstallBridge,
            Some(&info),
            vec![(
                PathBuf::from("Data/FCMServerBridge.ba2"),
                b"BTDX new".to_vec(),
            )],
        )?;
        assert!(repeat.changes.is_empty());
        fs::remove_file(game.join("dxgi.dll"))?;
        let (remove, _) = make_plan(
            &game,
            &ini,
            "Fallout76",
            FcmAction::Remove,
            None,
            Vec::new(),
        )?;
        apply_plan(remove, fixture.path())?;
        assert!(!game.join("Data/FCMServerBridge.ba2").exists());
        assert!(fs::read_to_string(&custom)?.contains("Other.ba2,HUDModLoader.ba2"));
        assert!(fs::read_to_string(&custom)?.contains("[Other]\nkeep=yes"));
        Ok(())
    }

    #[test]
    fn failed_write_restores_earlier_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let first = dir.path().join("first.ini");
        fs::write(&first, "before")?;
        let parent_file = dir.path().join("not-a-directory");
        fs::write(&parent_file, "blocking")?;
        let plan = Plan {
            changes: vec![
                FileChange {
                    path: first.clone(),
                    before: Some(b"before".to_vec()),
                    after: Some(b"after".to_vec()),
                    description: "first".to_owned(),
                },
                FileChange {
                    path: parent_file.join("child.ini"),
                    before: None,
                    after: Some(b"new".to_vec()),
                    description: "second".to_owned(),
                },
            ],
        };
        assert!(apply_plan(plan, dir.path()).is_err());
        assert_eq!(fs::read_to_string(first)?, "before");
        Ok(())
    }
}
