use std::io::BufRead;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{fs, io};

use duplicate::duplicate_item;
use ini::Ini;
use serde::Serialize;
use specta::Type;
use tap::TapFallible;
use tauri::State;
use tauri::async_runtime::spawn_blocking;

use super::errors::{CommandError, CommandResult};
use crate::features::stores::ini::{IniFile, IniFiles};
use crate::utils::ini::IniAccessors;
use crate::utils::paths::get_resources_path;

fn merge_changed_keys(baseline: &[u8], desired: &Ini) -> CommandResult<Vec<u8>> {
    let original = String::from_utf8(baseline.to_vec()).map_err(|error| CommandError::String {
        message: format!("INI is not UTF-8: {error}"),
    })?;
    let previous = Ini::load_from_str(&original)?;
    let mut merged = original.clone();
    for (section, properties) in &previous {
        let Some(section) = section else { continue };
        for (key, value) in properties.iter() {
            if desired.get_from(Some(section), key) != Some(value) {
                merged = super::fcm::ini_update(
                    &merged,
                    section,
                    key,
                    desired.get_from(Some(section), key),
                )
                .map_err(CommandError::from)?;
            }
        }
    }
    for (section, properties) in desired {
        let Some(section) = section else { continue };
        for (key, value) in properties.iter() {
            if previous.get_from(Some(section), key).is_none() {
                merged = super::fcm::ini_update(&merged, section, key, Some(value))
                    .map_err(CommandError::from)?;
            }
        }
    }
    let previous_global: Vec<_> = previous
        .section(None::<&str>)
        .into_iter()
        .flat_map(|props| props.iter())
        .collect();
    let desired_global: Vec<_> = desired
        .section(None::<&str>)
        .into_iter()
        .flat_map(|props| props.iter())
        .collect();
    if previous_global != desired_global {
        return Err(CommandError::String {
            message: "Unsectioned INI changes need manual editing".to_owned(),
        });
    }
    Ok(merged.into_bytes())
}

#[duplicate_item(
    ini_get_TYPE        VALUE     RETURN_TYPE;
    [ini_get_string]    [string]  [String];
    [ini_get_int]       [i32]     [i32];
    [ini_get_float]     [f32]     [f32];
    [ini_get_boolean]   [bool]    [bool];
)]
#[tauri::command]
#[specta::specta]
pub fn ini_get_TYPE(
    ini_file: IniFile,
    section: Option<&str>,
    key: &str,
    state: State<IniFiles>,
) -> Option<RETURN_TYPE> {
    let ini = state
        .get_file(ini_file)
        .lock()
        .expect("couldn't lock ini file");
    (*ini).VALUE(section, key)
}

#[duplicate_item(
    ini_set_TYPE        set_VALUE     TYPE;
    [ini_set_string]    [set_string]  [&str];
    [ini_set_int]       [set_i32]     [i32];
    [ini_set_float]     [set_f32]     [f32];
    [ini_set_boolean]   [set_bool]    [bool];
)]
#[tauri::command]
#[specta::specta]
pub fn ini_set_TYPE(
    ini_file: IniFile,
    section: Option<&str>,
    key: &str,
    value: TYPE,
    state: State<IniFiles>,
) {
    let mut ini = state
        .get_file(ini_file)
        .lock()
        .expect("couldn't lock ini file");
    ini.set_VALUE(section, key, value);
}

#[tauri::command]
#[specta::specta]
pub fn ini_delete_key(ini_file: IniFile, section: Option<&str>, key: &str, state: State<IniFiles>) {
    let mut ini = state
        .get_file(ini_file)
        .lock()
        .expect("couldn't lock ini file");
    ini.delete_from(section, key);
}

#[tauri::command]
#[specta::specta]
pub fn ini_has_key(
    ini_file: IniFile,
    section: Option<&str>,
    key: &str,
    state: State<IniFiles>,
) -> bool {
    let ini = state
        .get_file(ini_file)
        .lock()
        .expect("couldn't lock ini file");
    ini.has(section, key)
}

#[tauri::command]
#[specta::specta]
pub async fn ini_load(
    ini_path: String,
    ini_prefix: String,
    state: State<'_, IniFiles>,
) -> CommandResult<()> {
    let main = Arc::clone(&state.main);
    let prefs = Arc::clone(&state.prefs);
    let custom = Arc::clone(&state.custom);
    let baselines = Arc::clone(&state.baselines);
    spawn_blocking(move || _ini_load(ini_path, ini_prefix, main, prefs, custom, baselines))
        .await
        .tap_err(|e| log::error!("Couldn't join handle in ini_load: {e}"))
        .map_err(CommandError::from)
        .flatten()
}

pub fn _ini_load(
    ini_path: String,
    ini_prefix: String,
    main: Arc<Mutex<Ini>>,
    prefs: Arc<Mutex<Ini>>,
    custom: Arc<Mutex<Ini>>,
    baselines: Arc<Mutex<std::collections::HashMap<std::path::PathBuf, Option<Vec<u8>>>>>,
) -> CommandResult<()> {
    log::trace!(
        "Loading ini files from '{}' with prefix '{}'",
        ini_path,
        ini_prefix
    );

    // Get paths based on directory path and prefix:
    let main_path = Path::new(&ini_path).join(format!("{}.ini", ini_prefix));
    let prefs_path = Path::new(&ini_path).join(format!("{}Prefs.ini", ini_prefix));
    let custom_path = Path::new(&ini_path).join(format!("{}Custom.ini", ini_prefix));

    // Lock state mutexes:
    let mut main_lock = main
        .lock()
        .tap_err(|err| log::error!("Couldn't lock mutex for {ini_prefix}.ini: {err}"))?;
    let mut prefs_lock = prefs
        .lock()
        .tap_err(|err| log::error!("Couldn't lock mutex for {ini_prefix}Prefs.ini: {err}"))?;
    let mut custom_lock = custom
        .lock()
        .tap_err(|err| log::error!("Couldn't lock mutex for {ini_prefix}Custom.ini: {err}"))?;

    // Read state from files
    *main_lock = Ini::load_from_file(&main_path)
        .tap_err(|err| log::error!("Couldn't load or parse {ini_prefix}.ini: {err}"))
        .map_err(|err| match err {
            ini::Error::Parse(err) => Into::<CommandError>::into(err)
                .with_file_name(main_path.file_name().unwrap().to_string_lossy().to_string()),
            _ => err.into(),
        })?;
    *prefs_lock = Ini::load_from_file(&prefs_path)
        .tap_err(|err| log::error!("Couldn't load or parse {ini_prefix}Prefs.ini: {err}"))
        .map_err(|err| match err {
            ini::Error::Parse(err) => Into::<CommandError>::into(err).with_file_name(
                prefs_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            ),
            _ => err.into(),
        })?;
    *custom_lock = Ini::load_from_file(&custom_path)
        .tap_err(|err| log::error!("Couldn't load or parse {ini_prefix}Custom.ini: {err}"))
        .or_else(|err| match err {
            ini::Error::Parse(err) => Err(Into::<CommandError>::into(err).with_file_name(
                custom_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            )),
            _ => Ok(Default::default()),
        })?;

    let mut snapshots = baselines.lock()?;
    for path in [&main_path, &prefs_path, &custom_path] {
        snapshots.insert(path.clone(), std::fs::read(path).ok());
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn ini_save(
    ini_path: String,
    ini_prefix: String,
    state: State<'_, IniFiles>,
) -> CommandResult<()> {
    let main = Arc::clone(&state.main);
    let prefs = Arc::clone(&state.prefs);
    let custom = Arc::clone(&state.custom);
    let baselines = Arc::clone(&state.baselines);
    spawn_blocking(move || _ini_save(ini_path, ini_prefix, main, prefs, custom, baselines))
        .await
        .tap_err(|e| log::error!("Couldn't join handle in ini_save: {e}"))
        .map_err(CommandError::from)
        .flatten()
}

pub fn _ini_save(
    ini_path: String,
    ini_prefix: String,
    main: Arc<Mutex<Ini>>,
    prefs: Arc<Mutex<Ini>>,
    custom: Arc<Mutex<Ini>>,
    baselines: Arc<Mutex<std::collections::HashMap<std::path::PathBuf, Option<Vec<u8>>>>>,
) -> CommandResult<()> {
    log::trace!(
        "Saving ini files to '{}' with prefix '{}'",
        ini_path,
        ini_prefix
    );

    // Get paths based on directory path and prefix:
    let main_path = Path::new(&ini_path).join(format!("{}.ini", ini_prefix));
    let prefs_path = Path::new(&ini_path).join(format!("{}Prefs.ini", ini_prefix));
    let custom_path = Path::new(&ini_path).join(format!("{}Custom.ini", ini_prefix));

    // Lock state mutexes:
    let main_lock = main
        .lock()
        .tap_err(|err| log::error!("Couldn't lock mutex for {ini_prefix}.ini: {err}"))?;
    let prefs_lock = prefs
        .lock()
        .tap_err(|err| log::error!("Couldn't lock mutex for {ini_prefix}Prefs.ini: {err}"))?;
    let custom_lock = custom
        .lock()
        .tap_err(|err| log::error!("Couldn't lock mutex for {ini_prefix}Custom.ini: {err}"))?;

    let mut snapshots = baselines.lock()?;
    for path in [&main_path, &prefs_path, &custom_path] {
        let expected = snapshots.get(path).ok_or_else(|| CommandError::String {
            message: format!(
                "{} was not loaded; reload INI files before saving",
                path.display()
            ),
        })?;
        if &std::fs::read(path).ok() != expected {
            return Err(CommandError::String {
                message: format!(
                    "{} changed outside Quick Configuration; reload before saving",
                    path.display()
                ),
            });
        }
    }

    for (path, desired) in [
        (&main_path, &*main_lock),
        (&prefs_path, &*prefs_lock),
        (&custom_path, &*custom_lock),
    ] {
        let baseline = snapshots
            .get(path)
            .and_then(Option::as_deref)
            .unwrap_or_default();
        let merged = merge_changed_keys(baseline, desired)?;
        if merged != baseline {
            let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| {
                CommandError::String {
                    message: format!("{} has no parent directory", path.display()),
                }
            })?)?;
            use std::io::Write;
            temp.write_all(&merged)?;
            temp.persist(path).map_err(|error| error.error)?;
        }
    }

    for path in [&main_path, &prefs_path, &custom_path] {
        snapshots.insert(path.clone(), std::fs::read(path).ok());
    }

    Ok(())
}

#[cfg(test)]
mod external_edit_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn stale_state_does_not_overwrite_external_custom_ini_edit() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("Fallout76.ini"), "[Display]\nfoo=1\n")?;
        std::fs::write(dir.path().join("Fallout76Prefs.ini"), "[Display]\nfoo=1\n")?;
        let custom = dir.path().join("Fallout76Custom.ini");
        std::fs::write(
            &custom,
            "[Archive]\nsResourceArchive2List=HUDModLoader.ba2\n",
        )?;
        let main = Arc::new(Mutex::new(Ini::new()));
        let prefs = Arc::new(Mutex::new(Ini::new()));
        let custom_state = Arc::new(Mutex::new(Ini::new()));
        let baselines = Arc::new(Mutex::new(HashMap::new()));
        let path = dir.path().display().to_string();
        _ini_load(
            path.clone(),
            "Fallout76".to_owned(),
            main.clone(),
            prefs.clone(),
            custom_state.clone(),
            baselines.clone(),
        )?;
        std::fs::write(
            &custom,
            "[Archive]\nsResourceArchive2List=HUDModLoader.ba2,FCMChatWidget.ba2\n",
        )?;
        assert!(
            _ini_save(
                path,
                "Fallout76".to_owned(),
                main,
                prefs,
                custom_state,
                baselines
            )
            .is_err()
        );
        assert!(std::fs::read_to_string(custom)?.contains("FCMChatWidget.ba2"));
        Ok(())
    }

    #[test]
    fn normal_save_preserves_unrelated_custom_ini_lines() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("Fallout76.ini"), "[Display]\nfoo=1\n")?;
        std::fs::write(dir.path().join("Fallout76Prefs.ini"), "[Display]\nfoo=1\n")?;
        let custom = dir.path().join("Fallout76Custom.ini");
        let before = "; player note\r\n[Archive]\r\nsResourceArchive2List=Other.ba2,FCMChatWidget.ba2\r\n[User]\r\nkeep=yes\r\n";
        std::fs::write(&custom, before)?;
        let main = Arc::new(Mutex::new(Ini::new()));
        let prefs = Arc::new(Mutex::new(Ini::new()));
        let custom_state = Arc::new(Mutex::new(Ini::new()));
        let baselines = Arc::new(Mutex::new(HashMap::new()));
        let path = dir.path().display().to_string();
        _ini_load(
            path.clone(),
            "Fallout76".to_owned(),
            main.clone(),
            prefs.clone(),
            custom_state.clone(),
            baselines.clone(),
        )?;
        custom_state
            .lock()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .set_to(Some("User"), "keep".to_owned(), "changed".to_owned());
        _ini_save(
            path,
            "Fallout76".to_owned(),
            main,
            prefs,
            custom_state,
            baselines,
        )?;
        let after = std::fs::read_to_string(custom)?;
        assert!(after.starts_with("; player note\r\n"));
        assert!(after.contains("sResourceArchive2List=Other.ba2,FCMChatWidget.ba2\r\n"));
        assert!(after.contains("keep=changed\r\n"));
        Ok(())
    }
}

/// Creates ini files from included templates.
#[tauri::command]
#[specta::specta]
pub fn ini_create_files(ini_path: String, ini_prefix: String) -> CommandResult<()> {
    let main_path = Path::new(&ini_path).join(format!("{}.ini", ini_prefix));
    let prefs_path = Path::new(&ini_path).join(format!("{}Prefs.ini", ini_prefix));

    let main_template_path = get_resources_path()
        .ok_or(anyhow::anyhow!("Couldn't get resources path"))?
        .join("IniFiles")
        .join("Fallout76.ini");
    let prefs_template_path = get_resources_path()
        .ok_or(anyhow::anyhow!("Couldn't get resources path"))?
        .join("IniFiles")
        .join("Fallout76Prefs.ini");

    fs::create_dir_all(&ini_path)?;

    fs::copy(&main_template_path, &main_path)?;
    fs::copy(&prefs_template_path, &prefs_path)?;

    Ok(())
}

#[derive(Debug, Serialize, Type)]
pub struct IniErrorContextLine {
    pub num: u32,
    pub code: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IniErrorContext {
    pub file_name: String,
    pub lines: Vec<IniErrorContextLine>,
}

#[tauri::command]
#[specta::specta]
pub fn ini_get_error_context(
    ini_path: String,
    file_name: String,
    line: u32,
    msg: String,
) -> CommandResult<IniErrorContext> {
    let path = Path::new(&ini_path).join(&file_name);
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut lines = Vec::new();
    let line = line as i32;
    for (line_num, line_str) in reader.lines().enumerate() {
        let line_num = line_num as i32 + 1;
        match line_num {
            n if (line - 2..=line - 1).contains(&n) || (line + 1..=line + 2).contains(&n) => {
                lines.push(IniErrorContextLine {
                    num: line_num as u32,
                    code: line_str?,
                    error: None,
                });
            }
            n if line == n => {
                lines.push(IniErrorContextLine {
                    num: line_num as u32,
                    code: line_str?,
                    error: Some(msg.clone()),
                });
            }
            _ => {}
        }
    }
    Ok(IniErrorContext { file_name, lines })
}
