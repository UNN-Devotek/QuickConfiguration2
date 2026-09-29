use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use tauri::State;
use uuid::Uuid;

use super::errors::CommandResult;
use super::ini::ini_update;

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
    selected_provider: Option<String>,
    provider_package: Option<String>,
    loader_package: Option<String>,
    state: State<'_, FcmPlans>,
) -> CommandResult<FcmPreview> {
    Ok(import::preview_import(
        &game_path,
        &ini_path,
        &ini_prefix,
        &paths,
        selected_provider.as_deref(),
        provider_package.as_deref(),
        loader_package.as_deref(),
        &state,
    )?)
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FcmPrerequisites {
    pub provider: Option<String>,
    pub hud_mod_loader: bool,
}

fn probe_prerequisites(game: &Path) -> Result<FcmPrerequisites> {
    ensure!(
        game.join("Fallout76.exe").is_file(),
        "Select the Fallout 76 game directory"
    );
    let dll_path = game.join("dxgi.dll");
    let provider = if dll_path.exists() {
        ensure!(
            fs::metadata(&dll_path)?.len() <= 100 * 1024 * 1024,
            "Provider DLL is unexpectedly large"
        );
        let dll = fs::read(&dll_path)?;
        let xscal = dll
            .windows(b"xscalchatv1".len())
            .any(|part| part.eq_ignore_ascii_case(b"xscalchatv1"));
        let zfe = dll
            .windows(b"zfe-chat-v1".len())
            .any(|part| part.eq_ignore_ascii_case(b"zfe-chat-v1"));
        match (xscal, zfe) {
            (true, false) => {
                ensure!(
                    game.join("xscal.ini").is_file(),
                    "xScal requires xscal.ini beside the game"
                );
                Some("xscal".to_owned())
            }
            (false, true) => Some("zfe".to_owned()),
            _ => bail!(
                "dxgi.dll is already present but is not exactly one supported provider; resolve it before installing FCM"
            ),
        }
    } else {
        None
    };
    Ok(FcmPrerequisites {
        provider,
        hud_mod_loader: game.join("Data/HUDModLoader.ba2").is_file()
            && game.join("Data/hudmodloader.ini").is_file(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn fcm_probe_prerequisites(game_path: String) -> CommandResult<FcmPrerequisites> {
    Ok(probe_prerequisites(Path::new(&game_path))?)
}

#[tauri::command]
#[specta::specta]
pub async fn fcm_prerequisite_download_links(
    api_key: String,
    mod_id: u32,
    file_id: u32,
) -> CommandResult<Vec<crate::features::nexusmods::models::json::DownloadLink>> {
    if ![4065, 4183, 3144].contains(&mod_id) {
        return Err(anyhow::anyhow!("Unsupported prerequisite source").into());
    }
    Ok(crate::features::nexusmods::api::NexusModsAPI::new(api_key)
        .request_download_links(
            "fallout76".to_owned(),
            mod_id.into(),
            file_id.into(),
            None,
            None,
        )
        .await?)
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
                if cmdline_contains_game(&cmd) {
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

#[cfg(any(target_os = "linux", test))]
fn cmdline_contains_game(cmdline: &[u8]) -> bool {
    cmdline.split(|byte| *byte == 0).any(|part| {
        part.rsplit(|byte| *byte == b'/' || *byte == b'\\')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(b"Fallout76.exe"))
    })
}

fn provider(game: &Path) -> Result<String> {
    let probe = probe_prerequisites(game)?;
    ensure!(probe.hud_mod_loader, "Install HUDModLoader first");
    probe.provider.context("Install ZFE or xScal first")
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    ensure!(
        !path.is_symlink() || path.exists(),
        "{} is a broken symbolic link",
        path.display()
    );
    if path.exists() {
        Ok(Some(fs::read(path)?))
    } else {
        Ok(None)
    }
}

fn is_linked(path: &Path) -> Result<bool> {
    if path.is_symlink() {
        return Ok(true);
    }
    if !path.exists() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path)?;
        Ok(metadata.nlink() > 1)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };

        let file = fs::File::open(path)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe {
            GetFileInformationByHandle(HANDLE(file.as_raw_handle() as isize), &mut info)?;
        }
        Ok(info.nNumberOfLinks > 1)
    }
}

fn add_change(
    changes: &mut Vec<FileChange>,
    path: PathBuf,
    after: Option<Vec<u8>>,
    description: &str,
) -> Result<()> {
    ensure!(
        !is_linked(&path)?
            || path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ini")),
        "{} is linked to another file; remove its managed mod through the normal mod list first",
        path.display()
    );
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
    if action == FcmAction::Remove
        && !input.lines().any(|line| {
            is_fcm_loader_entry(line, "FCMChatWidget")
                || is_fcm_loader_entry(line, "FCMServerBridge")
        })
    {
        return input.to_owned();
    }
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
    if action == FcmAction::Remove
        && !current.split(',').any(|item| {
            ["FCMChatWidget.ba2", "FCMServerBridge.ba2"]
                .iter()
                .any(|name| item.trim().eq_ignore_ascii_case(name))
        })
    {
        return Ok(input.to_owned());
    }
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

fn current_install(game: &Path) -> Result<Option<String>> {
    let loader = read_optional(&game.join("Data/hudmodloader.ini"))?
        .map(String::from_utf8)
        .transpose()?
        .unwrap_or_default();
    if let Some(mode) = current_mode(&loader)? {
        return Ok(Some(mode));
    }
    let hud = game.join("Data/FCMChatWidget.ba2").is_file();
    let bridge = game.join("Data/FCMServerBridge.ba2").is_file();
    Ok(match (hud, bridge) {
        (true, true) => Some("HUD and Server Bridge (conflict)".to_owned()),
        (true, false) => Some("HUD".to_owned()),
        (false, true) => Some("Server Bridge".to_owned()),
        (false, false) => None,
    })
}

#[tauri::command]
#[specta::specta]
pub fn fcm_current_install(game_path: String) -> CommandResult<Option<String>> {
    Ok(current_install(Path::new(&game_path))?)
}

fn make_plan(
    game: &Path,
    ini_dir: &Path,
    prefix: &str,
    action: FcmAction,
    info: Option<&FcmPackageInfo>,
    package: Vec<(PathBuf, Vec<u8>)>,
) -> Result<(Plan, FcmPreview)> {
    make_plan_with_prerequisites(
        game,
        ini_dir,
        prefix,
        action,
        info,
        package,
        None,
        Vec::new(),
        None,
    )
}

fn make_plan_with_prerequisites(
    game: &Path,
    ini_dir: &Path,
    prefix: &str,
    action: FcmAction,
    info: Option<&FcmPackageInfo>,
    package: Vec<(PathBuf, Vec<u8>)>,
    requested_provider: Option<&str>,
    prerequisites: Vec<(PathBuf, Vec<u8>)>,
    loader_default: Option<String>,
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
    } else if let Some(requested) = requested_provider {
        ensure!(
            requested == "zfe" || requested == "xscal",
            "Unsupported provider"
        );
        let probe = probe_prerequisites(game)?;
        ensure!(
            probe
                .provider
                .as_deref()
                .is_none_or(|existing| existing == requested),
            "The installed provider differs from the selected provider"
        );
        ensure!(
            probe.provider.is_some()
                || prerequisites
                    .iter()
                    .any(|(path, _)| path == Path::new("dxgi.dll")),
            "The selected provider package is missing"
        );
        ensure!(
            probe.hud_mod_loader
                || prerequisites
                    .iter()
                    .any(|(path, _)| path == Path::new("Data/HUDModLoader.ba2")),
            "The HUDModLoader package is missing"
        );
        requested.to_owned()
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
    let loader_base = if loader.exists() {
        loader_before.clone()
    } else {
        loader_default.unwrap_or_default()
    };
    let installed = current_mode(&loader_before)?;
    let mut changes = Vec::new();
    for (relative, bytes) in prerequisites {
        ensure!(
            matches!(
                relative.to_str(),
                Some("dxgi.dll" | "xscal.ini" | "Data/HUDModLoader.ba2")
            ),
            "Unexpected prerequisite file"
        );
        let path = game.join(&relative);
        if relative != Path::new("Data/HUDModLoader.ba2") {
            ensure!(
                !path.exists(),
                "Prerequisite changed since detection: {}",
                path.display()
            );
        }
        add_change(
            &mut changes,
            path,
            Some(bytes),
            "Install selected prerequisite",
        )?;
    }
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
            let before = if let Some(change) = changes.iter().find(|change| change.path == path) {
                String::from_utf8(
                    change
                        .after
                        .clone()
                        .context("Missing planned xScal settings")?,
                )?
            } else {
                fs::read_to_string(&path)?
            };
            let example = String::from_utf8(bytes)?;
            let enabled = ini_value(&example, "Chat", "enabled")?
                .context("HUD package has no xScal Chat enabled value")?;
            let endpoint = ini_value(&example, "Chat", "relayEndpoint")?
                .context("HUD package has no xScal relay endpoint")?;
            let merged = ini_update(&before, "Chat", "enabled", Some(&enabled))?;
            let merged = ini_update(&merged, "Chat", "relayEndpoint", Some(&endpoint))?;
            if let Some(change) = changes.iter_mut().find(|change| change.path == path) {
                change.after = Some(merged.into_bytes());
            } else {
                add_change(
                    &mut changes,
                    path,
                    Some(merged.into_bytes()),
                    "Merge xScal chat settings",
                )?;
            }
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
    let loader_after = loader_text(&loader_base, action);
    if action != FcmAction::Remove || loader.exists() {
        add_change(
            &mut changes,
            loader,
            Some(loader_after.into_bytes()),
            "Select one FCM loader child",
        )?;
    }
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

#[tauri::command]
#[specta::specta]
pub fn fcm_discard(token: String, state: State<'_, FcmPlans>) -> CommandResult<()> {
    state.0.lock()?.remove(&token);
    Ok(())
}

#[derive(Serialize)]
struct BackupEntry {
    original: String,
    saved: Option<String>,
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if is_linked(path)? {
        ensure!(
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ini")),
            "{} is linked to another file",
            path.display()
        );
        fs::write(path, bytes)?;
        return Ok(());
    }
    fs::create_dir_all(path.parent().context("File has no parent")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("File has no parent")?)?;
    temp.write_all(bytes)?;
    if path.exists() {
        temp.as_file()
            .set_permissions(fs::metadata(path)?.permissions())?;
    }
    temp.persist(path)?;
    Ok(())
}

fn apply_plan(plan: Plan, backup_root: &Path) -> Result<String> {
    ensure!(!game_running(), "Close Fallout 76 before changing mods");
    for change in &plan.changes {
        ensure!(
            !is_linked(&change.path)?
                || change
                    .path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ini")),
            "{} is linked to another file; preview again",
            change.path.display()
        );
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
            let mut restore_errors = Vec::new();
            for old in applied.into_iter().chain(std::iter::once(change)).rev() {
                let old: &FileChange = old;
                let restored = if let Some(before) = &old.before {
                    atomic_write(&old.path, before)
                } else if old.path.exists() {
                    fs::remove_file(&old.path).map_err(Into::into)
                } else {
                    Ok(())
                };
                if let Err(restore_error) = restored {
                    restore_errors.push(format!("{}: {restore_error}", old.path.display()));
                }
            }
            if restore_errors.is_empty() {
                return Err(error.context("FCM install failed; previous files were restored"));
            }
            return Err(error.context(format!(
                "FCM install failed; rollback incomplete: {}; backup: {}",
                restore_errors.join("; "),
                backup.display()
            )));
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
    fn linux_game_detection_accepts_proton_path_separators() {
        assert!(cmdline_contains_game(
            b"Z:\\steamapps\\Fallout76.exe\0--foo\0"
        ));
        assert!(cmdline_contains_game(b"/games/Fallout76.exe\0"));
        assert!(!cmdline_contains_game(b"/games/Other.exe\0"));
    }

    #[test]
    fn failed_rollback_reports_backup_and_restore_error() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let original = dir.path().join("original.ini");
        fs::write(&original, b"before")?;
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, b"not a directory")?;
        let plan = Plan {
            changes: vec![
                FileChange {
                    path: original.clone(),
                    before: Some(b"before".to_vec()),
                    after: None,
                    description: "remove".to_owned(),
                },
                FileChange {
                    path: original.join("child"),
                    before: None,
                    after: Some(b"child".to_vec()),
                    description: "create nested file".to_owned(),
                },
                FileChange {
                    path: blocker.join("child"),
                    before: None,
                    after: Some(b"fail".to_vec()),
                    description: "fail".to_owned(),
                },
            ],
        };
        let error = match apply_plan(plan, dir.path()) {
            Ok(_) => bail!("The planned write should fail"),
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains("rollback incomplete"));
        assert!(error.contains("backup:"));
        assert!(error.contains(&original.display().to_string()));
        Ok(())
    }

    #[test]
    fn current_install_finds_loader_entries_and_orphan_ba2() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let data = dir.path().join("Data");
        fs::create_dir_all(&data)?;
        assert_eq!(current_install(dir.path())?, None);
        fs::write(data.join("FCMChatWidget.ba2"), b"BTDX")?;
        assert_eq!(current_install(dir.path())?.as_deref(), Some("HUD"));
        fs::write(data.join("hudmodloader.ini"), b"FCMServerBridge\n")?;
        assert_eq!(
            current_install(dir.path())?.as_deref(),
            Some("Server Bridge")
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn replacing_an_fcm_file_preserves_its_permissions() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir()?;
        let path = dir.path().join("hudmodloader.ini");
        fs::write(&path, "before")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        atomic_write(&path, b"after")?;
        assert_eq!(fs::read_to_string(&path)?, "after");
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o644);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn linked_ini_is_updated_without_replacing_its_link() -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source.ini");
        let hardlink = dir.path().join("hudmodloader.ini");
        let symlink = dir.path().join("Fallout76Custom.ini");
        fs::write(&source, "before")?;
        fs::hard_link(&source, &hardlink)?;
        std::os::unix::fs::symlink(&source, &symlink)?;
        atomic_write(&hardlink, b"after")?;
        atomic_write(&symlink, b"final")?;
        assert_eq!(fs::metadata(&source)?.ino(), fs::metadata(&hardlink)?.ino());
        assert!(symlink.is_symlink());
        assert_eq!(fs::read(source)?, b"final");
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn linked_ba2_is_rejected_before_removal() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let game = dir.path().join("game");
        let ini = dir.path().join("ini");
        fs::create_dir_all(game.join("Data"))?;
        fs::create_dir_all(&ini)?;
        fs::write(game.join("Fallout76.exe"), b"")?;
        fs::write(ini.join("Fallout76Custom.ini"), b"[Archive]\n")?;
        let source = dir.path().join("managed.ba2");
        fs::write(&source, b"BTDX")?;
        fs::hard_link(&source, game.join("Data/FCMChatWidget.ba2"))?;
        assert!(
            make_plan(
                &game,
                &ini,
                "Fallout76",
                FcmAction::Remove,
                None,
                Vec::new()
            )
            .is_err()
        );
        assert_eq!(fs::read(source)?, b"BTDX");
        Ok(())
    }

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
    fn removing_orphan_ba2_does_not_create_or_rewrite_ini_files() -> Result<()> {
        let fixture = tempfile::tempdir()?;
        let game = fixture.path().join("game");
        let ini = fixture.path().join("ini");
        fs::create_dir_all(game.join("Data"))?;
        fs::create_dir_all(&ini)?;
        fs::write(game.join("Fallout76.exe"), b"")?;
        fs::write(game.join("Data/FCMChatWidget.ba2"), b"BTDX")?;
        let custom = ini.join("Fallout76Custom.ini");
        let original = b"; keep this exact layout\r\n[Archive]\r\nsResourceArchive2List=Other.ba2";
        fs::write(&custom, original)?;
        let (plan, _) = make_plan(
            &game,
            &ini,
            "Fallout76",
            FcmAction::Remove,
            None,
            Vec::new(),
        )?;
        assert_eq!(plan.changes.len(), 1);
        apply_plan(plan, fixture.path())?;
        assert!(!game.join("Data/FCMChatWidget.ba2").exists());
        assert!(!game.join("Data/hudmodloader.ini").exists());
        assert_eq!(fs::read(custom)?, original);
        Ok(())
    }

    #[test]
    fn provider_detection_ignores_compatibility_names() -> Result<()> {
        let fixture = tempfile::tempdir()?;
        let game = fixture.path();
        fs::create_dir_all(game.join("Data"))?;
        fs::write(game.join("Fallout76.exe"), b"")?;
        fs::write(game.join("Data/HUDModLoader.ba2"), b"BTDX")?;
        fs::write(game.join("Data/hudmodloader.ini"), b"OtherChild\n")?;
        fs::write(game.join("xscal.ini"), b"[Chat]\nenabled=true\n")?;
        fs::write(game.join("dxgi.dll"), b"XSCALCHATV1 GetZFERuntimeInfo")?;
        assert_eq!(provider(game)?, "xscal");
        fs::remove_file(game.join("Data/hudmodloader.ini"))?;
        assert!(!probe_prerequisites(game)?.hud_mod_loader);
        fs::write(game.join("Data/hudmodloader.ini"), b"OtherChild\n")?;
        fs::write(game.join("dxgi.dll"), b"zfe-chat-v1 xScal compatibility")?;
        assert_eq!(provider(game)?, "zfe");
        fs::write(game.join("dxgi.dll"), b"XSCALCHATV1 zfe-chat-v1")?;
        assert!(provider(game).is_err());
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
