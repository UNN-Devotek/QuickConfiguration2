use tap::TapFallible;

use crate::{
    commands::errors::{CommandError, CommandResult},
    utils::{open::open_path, paths::get_current_log_file_path},
};

#[tauri::command]
#[specta::specta]
pub fn open_log_file() -> CommandResult<()> {
    let path = get_current_log_file_path()
        .ok_or_else(|| CommandError::from("Log path could not be determined"))?;
    let path: camino::Utf8PathBuf = path.try_into()?;
    if !path.exists() {
        return Err(CommandError::from(format!(
            "Log file does not exist: {path}"
        )));
    }
    open_path(path.as_ref())
        .tap_err(|error| log::error!("Could not open log file: {error}"))
        .map_err(CommandError::from)
}
