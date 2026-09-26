use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use regex::bytes::Regex;
use semver::Version;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::{
    FcmAction, FcmPlans, FcmPreview, FcmRelease, MAX_DOWNLOAD, make_plan, package_files, provider,
};

enum ImportSource {
    Directory(PathBuf),
    ZipFile(PathBuf),
    ZipBytes(Vec<u8>),
}

struct ImportedPackage {
    action: FcmAction,
    release: FcmRelease,
    files: Vec<(PathBuf, Vec<u8>)>,
}

fn safe_name(name: &str) -> bool {
    !name.contains('\\')
        && !name.starts_with('/')
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn zip_names<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Result<Vec<String>> {
    ensure!(zip.len() <= 20_000, "Archive has too many entries");
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        let name = entry.name();
        ensure!(safe_name(name), "Archive contains an unsafe path");
        if entry.is_file() {
            ensure!(
                seen.insert(name.to_owned()),
                "Archive contains duplicate paths"
            );
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn zip_read<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let entry = zip.by_name(name)?;
    ensure!(
        entry.is_file() && entry.size() <= limit,
        "Invalid {name} size"
    );
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "Invalid {name} size");
    Ok(bytes)
}

impl ImportSource {
    fn label(&self) -> String {
        match self {
            Self::Directory(path) | Self::ZipFile(path) => path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            Self::ZipBytes(_) => "nested ZIP".to_owned(),
        }
    }

    fn from_path(path: PathBuf) -> Result<Option<Self>> {
        if path.is_dir() {
            return Ok(Some(Self::Directory(path)));
        }
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            return Ok(Some(Self::ZipFile(path)));
        }
        Ok(None)
    }

    fn names(&self) -> Result<Vec<String>> {
        match self {
            Self::ZipFile(path) => zip_names(&mut ZipArchive::new(File::open(path)?)?),
            Self::ZipBytes(bytes) => {
                zip_names(&mut ZipArchive::new(Cursor::new(bytes.as_slice()))?)
            }
            Self::Directory(root) => {
                let mut names = Vec::new();
                let mut stack = vec![root.clone()];
                while let Some(dir) = stack.pop() {
                    for entry in fs::read_dir(dir)? {
                        let entry = entry?;
                        let kind = entry.file_type()?;
                        if kind.is_symlink() {
                            continue;
                        }
                        if kind.is_dir() {
                            stack.push(entry.path());
                        } else if kind.is_file() {
                            let relative = entry.path().strip_prefix(root)?.to_path_buf();
                            let name = relative.to_string_lossy().replace('\\', "/");
                            if safe_name(&name) {
                                names.push(name);
                            }
                        }
                        ensure!(
                            names.len() + stack.len() <= 20_000,
                            "Folder is too large to inspect"
                        );
                    }
                }
                if root
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("Data (drag"))
                    && root
                        .parent()
                        .is_some_and(|parent| parent.join("xscal.ini").is_file())
                {
                    names.push("xscal.ini".to_owned());
                }
                Ok(names)
            }
        }
    }

    fn read(&self, name: &str, limit: u64) -> Result<Vec<u8>> {
        ensure!(safe_name(name), "Unsafe package path");
        match self {
            Self::ZipFile(path) => zip_read(&mut ZipArchive::new(File::open(path)?)?, name, limit),
            Self::ZipBytes(bytes) => zip_read(
                &mut ZipArchive::new(Cursor::new(bytes.as_slice()))?,
                name,
                limit,
            ),
            Self::Directory(root) => {
                let path = if name == "xscal.ini"
                    && root
                        .file_name()
                        .is_some_and(|value| value.to_string_lossy().starts_with("Data (drag"))
                    && !root.join(name).exists()
                {
                    root.parent().context("Missing provider folder")?.join(name)
                } else {
                    root.join(name)
                };
                ensure!(
                    path.is_file() && fs::metadata(&path)?.len() <= limit,
                    "Invalid {name} size"
                );
                Ok(fs::read(path)?)
            }
        }
    }
}

fn package_zip(entries: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer.start_file(name, SimpleFileOptions::default())?;
        writer.write_all(&bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

fn local_release(version: String) -> FcmRelease {
    FcmRelease {
        version,
        url: String::new(),
        source: "Imported package".to_owned(),
        digest: None,
    }
}

fn import_bridge(
    source: &ImportSource,
    names: &[String],
    build_name: &str,
) -> Result<ImportedPackage> {
    let prefix = build_name
        .strip_suffix("BUILD.json")
        .context("Invalid bridge manifest path")?;
    let ba2_name = format!("{prefix}Data/FCMServerBridge.ba2");
    ensure!(
        names.iter().any(|name| name == &ba2_name),
        "Bridge package is missing its BA2"
    );
    let build = source.read(build_name, 10_000)?;
    let metadata: serde_json::Value = serde_json::from_slice(&build)?;
    let version = metadata["version"]
        .as_str()
        .context("Bridge version is missing")?;
    Version::parse(version)?;
    ensure!(
        metadata["target"] == "prod",
        "Only production bridge packages are supported"
    );
    let ba2 = source.read(&ba2_name, 2 * 1024 * 1024)?;
    let release = local_release(version.to_owned());
    let bytes = package_zip(vec![
        ("BUILD.json".to_owned(), build),
        ("Data/FCMServerBridge.ba2".to_owned(), ba2),
    ])?;
    let files = package_files(FcmAction::InstallBridge, &release, bytes, "zfe")?;
    Ok(ImportedPackage {
        action: FcmAction::InstallBridge,
        release,
        files,
    })
}

fn hud_version_from_ba2(ba2: &[u8]) -> Result<String> {
    let matcher = Regex::new(r"(?-u)(?:^|[^0-9])([0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3})(?:[^0-9]|$)")?;
    matcher
        .captures_iter(ba2)
        .filter_map(|capture| {
            let value = std::str::from_utf8(capture.get(1)?.as_bytes()).ok()?;
            let version = Version::parse(value).ok()?;
            (version.major <= 10).then_some(version)
        })
        .max()
        .map(|version| version.to_string())
        .context("Cannot identify the HUD version from the selected folder")
}

fn hud_version(source: &ImportSource, names: &[String], root: &str, ba2: &[u8]) -> Result<String> {
    let version_name = format!("{root}FCMChatWidget.version.txt");
    if names.iter().any(|name| name == &version_name) {
        let install_name = format!("{root}INSTALL.txt");
        let install = String::from_utf8(source.read(&install_name, 200_000)?)?;
        ensure!(
            install
                .lines()
                .next()
                .is_some_and(|line| line.contains("PRODUCTION")),
            "Only production HUD packages are supported"
        );
        let version = String::from_utf8(source.read(&version_name, 100)?)?;
        return Ok(Version::parse(version.trim())?.to_string());
    }
    let install_name = format!("{root}INSTALL.txt");
    if names.iter().any(|name| name == &install_name) {
        let install = String::from_utf8(source.read(&install_name, 200_000)?)?;
        ensure!(
            install
                .lines()
                .next()
                .is_some_and(|line| line.contains("PRODUCTION")),
            "Only production HUD packages are supported"
        );
        return install
            .split_whitespace()
            .find_map(|word| Version::parse(word).ok())
            .map(|version| version.to_string())
            .context("HUD package has no version");
    }
    hud_version_from_ba2(ba2)
}

fn import_hud(
    source: &ImportSource,
    names: &[String],
    ba2_name: &str,
    selected_provider: &str,
) -> Result<ImportedPackage> {
    let data_prefix = ba2_name
        .strip_suffix("FCMChatWidget.ba2")
        .context("Invalid HUD BA2 path")?;
    let root = data_prefix
        .strip_suffix("Data (drag the contents into data folder)/")
        .or_else(|| data_prefix.strip_suffix("Data/"))
        .unwrap_or(data_prefix);
    let chat_name = format!("{data_prefix}FCMChat.ini");
    ensure!(
        names.iter().any(|name| name == &chat_name),
        "HUD package is missing FCMChat.ini"
    );
    let zfe_name = format!("{data_prefix}ZFE/TextChat/fragments/FCMChatWidget.ini");
    let xscal_name = format!("{root}xscal.ini");
    let legacy = names
        .iter()
        .any(|name| name == &format!("{root}FCMChatWidget.version.txt"));
    let (zfe_name, xscal_name) = if legacy {
        (
            format!("{root}examples/ZFE/FCMChatWidget.ini.example"),
            format!("{root}xscal.ini.example"),
        )
    } else {
        (zfe_name, xscal_name)
    };
    let has_zfe = names.iter().any(|name| name == &zfe_name);
    let has_xscal = names.iter().any(|name| name == &xscal_name);
    ensure!(
        if legacy {
            has_zfe && has_xscal
        } else {
            has_zfe != has_xscal
        },
        "Select a complete ZFE or xScal HUD folder"
    );
    let package_provider = if legacy {
        selected_provider
    } else if has_zfe {
        "zfe"
    } else {
        "xscal"
    };
    ensure!(
        package_provider == selected_provider,
        "HUD package provider does not match the installed provider"
    );
    let ba2 = source.read(ba2_name, 20 * 1024 * 1024)?;
    let version = hud_version(source, names, root, &ba2)?;
    let release = local_release(version.clone());
    let use_zfe = package_provider == "zfe";
    let folder = if use_zfe {
        "ZFE (Install for ZFE only)"
    } else {
        "xScal (Install for xScal only)"
    };
    let data = format!("{folder}/Data (drag the contents into data folder)");
    let mut entries = vec![
        (
            "README.txt".to_owned(),
            format!("Version: {version}\nPackage provider: unified\n").into_bytes(),
        ),
        (format!("{data}/FCMChatWidget.ba2"), ba2),
        (
            format!("{data}/FCMChat.ini"),
            source.read(&chat_name, 100_000)?,
        ),
    ];
    if use_zfe {
        entries.push((
            format!("{data}/ZFE/TextChat/fragments/FCMChatWidget.ini"),
            source.read(&zfe_name, 100_000)?,
        ));
    } else {
        entries.push((
            format!("{folder}/xscal.ini"),
            source.read(&xscal_name, 10_000)?,
        ));
    }
    let files = package_files(
        FcmAction::InstallHud,
        &release,
        package_zip(entries)?,
        selected_provider,
    )?;
    Ok(ImportedPackage {
        action: FcmAction::InstallHud,
        release,
        files,
    })
}

fn inspect_source(
    source: &ImportSource,
    selected_provider: &str,
    nested: bool,
) -> Result<Vec<ImportedPackage>> {
    let names = source.names()?;
    let mut packages = Vec::new();
    for build in names.iter().filter(|name| name.ends_with("BUILD.json")) {
        let prefix = build
            .strip_suffix("BUILD.json")
            .context("Invalid bridge path")?;
        if names
            .iter()
            .any(|name| name == &format!("{prefix}Data/FCMServerBridge.ba2"))
        {
            packages.push(import_bridge(source, &names, build)?);
        }
    }
    let hud_ba2s: Vec<&String> = names
        .iter()
        .filter(|name| name.ends_with("FCMChatWidget.ba2"))
        .collect();
    if !hud_ba2s.is_empty() {
        let provider_ba2s: Vec<&&String> = hud_ba2s
            .iter()
            .filter(|name| {
                let lower = name.to_ascii_lowercase();
                !lower.contains("install for ") || lower.contains(selected_provider)
            })
            .collect();
        ensure!(
            provider_ba2s.len() == 1,
            "Select exactly one matching HUD provider folder"
        );
        packages.push(import_hud(
            source,
            &names,
            provider_ba2s[0],
            selected_provider,
        )?);
    }
    if !nested && packages.is_empty() {
        for name in names
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(".zip"))
        {
            let lower = name.to_ascii_lowercase();
            if lower.contains("fcm") || lower.contains("bridge") || lower.contains("hud") {
                let bytes = source.read(name, MAX_DOWNLOAD as u64)?;
                packages.extend(inspect_source(
                    &ImportSource::ZipBytes(bytes),
                    selected_provider,
                    true,
                )?);
            }
        }
    }
    Ok(packages)
}

fn contains_marker(source: &ImportSource, nested: bool) -> Result<bool> {
    let names = source.names()?;
    if names
        .iter()
        .any(|name| name.ends_with("FCMChatWidget.ba2") || name.ends_with("FCMServerBridge.ba2"))
    {
        return Ok(true);
    }
    if !nested {
        for name in names
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(".zip"))
        {
            let lower = name.to_ascii_lowercase();
            if lower.contains("fcm") || lower.contains("bridge") || lower.contains("hud") {
                let bytes = source.read(name, MAX_DOWNLOAD as u64)?;
                if contains_marker(&ImportSource::ZipBytes(bytes), true)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn source_paths(paths: &[String]) -> Result<Vec<ImportSource>> {
    let mut sources = Vec::new();
    for raw in paths {
        let path = PathBuf::from(raw);
        ensure!(
            path.exists(),
            "Import path does not exist: {}",
            path.display()
        );
        if let Some(source) = ImportSource::from_path(path)? {
            sources.push(source);
        }
    }
    Ok(sources)
}

pub(super) fn detect_import(paths: &[String]) -> Result<bool> {
    for raw in paths {
        let path = Path::new(raw);
        if path.is_file()
            && path.file_name().is_some_and(|name| {
                name.eq_ignore_ascii_case("FCMChatWidget.ba2")
                    || name.eq_ignore_ascii_case("FCMServerBridge.ba2")
            })
        {
            return Ok(true);
        }
    }
    for source in source_paths(paths)? {
        if contains_marker(&source, false)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn preview_import(
    game_path: &str,
    ini_path: &str,
    ini_prefix: &str,
    paths: &[String],
    state: &FcmPlans,
) -> Result<FcmPreview> {
    let game = PathBuf::from(game_path);
    let selected_provider = provider(&game)?;
    let mut packages = Vec::new();
    for source in source_paths(paths)? {
        let label = source.label();
        for mut package in inspect_source(&source, &selected_provider, false)? {
            package.release.source = format!("Local package: {label}");
            packages.push(package);
        }
    }
    ensure!(
        packages.len() == 1,
        "Select exactly one complete FCM HUD or Server Bridge package"
    );
    let package = packages.pop().context("No FCM package was found")?;
    let (plan, preview) = make_plan(
        &game,
        Path::new(&ini_path),
        ini_prefix,
        package.action,
        Some(&package.release),
        package.files,
    )?;
    state
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("FCM plan lock is unavailable"))?
        .insert(preview.token.clone(), plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::fcm::hex_digest;

    fn bridge_entries(prefix: &str) -> Vec<(String, Vec<u8>)> {
        let ba2 = b"BTDX bridge contents".to_vec();
        let build = serde_json::json!({
            "version": "0.2.8",
            "target": "prod",
            "ba2Sha256": hex_digest(&ba2),
        });
        vec![
            (
                format!("{prefix}BUILD.json"),
                serde_json::to_vec(&build).unwrap(),
            ),
            (format!("{prefix}Data/FCMServerBridge.ba2"), ba2),
        ]
    }

    #[test]
    fn recognizes_bridge_inside_windows_overlay_zip_without_importing_executable() -> Result<()> {
        let mut entries = bridge_entries("Optional FCM Bridge/");
        entries.push(("Fallout Chat Mod Setup.exe".to_owned(), b"MZ".to_vec()));
        let bytes = package_zip(entries)?;
        let packages = inspect_source(&ImportSource::ZipBytes(bytes), "zfe", false)?;
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].action, FcmAction::InstallBridge);
        assert_eq!(
            packages[0].files[0].0,
            PathBuf::from("Data/FCMServerBridge.ba2")
        );
        Ok(())
    }

    #[test]
    fn recognizes_bridge_inside_linux_overlay_zip() -> Result<()> {
        let bridge = package_zip(bridge_entries(""))?;
        let outer = package_zip(vec![
            ("Fallout Chat Mod.AppImage".to_owned(), b"appimage".to_vec()),
            ("Optional FCM Bridge/bridge-prod.zip".to_owned(), bridge),
        ])?;
        let packages = inspect_source(&ImportSource::ZipBytes(outer), "xscal", false)?;
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].action, FcmAction::InstallBridge);
        Ok(())
    }

    #[test]
    fn unified_hud_zip_selects_only_active_provider() -> Result<()> {
        let zfe = "ZFE (Install for ZFE only)/Data (drag the contents into data folder)";
        let xscal = "xScal (Install for xScal only)/Data (drag the contents into data folder)";
        let entries = vec![
            (
                format!("ZFE (Install for ZFE only)/INSTALL.txt"),
                b"Fallout Chat Mod HUD 2.10.125 - ZFE (PRODUCTION)".to_vec(),
            ),
            (
                format!("{zfe}/FCMChatWidget.ba2"),
                b"BTDX 2.10.125".to_vec(),
            ),
            (
                format!("{zfe}/FCMChat.ini"),
                b"[FCMChat]\nopenKey=INSERT\n".to_vec(),
            ),
            (
                format!("{zfe}/ZFE/TextChat/fragments/FCMChatWidget.ini"),
                b"[TextChat]\nOpenChatKey=INSERT\n".to_vec(),
            ),
            (
                format!("{xscal}/FCMChatWidget.ba2"),
                b"BTDX 2.10.125".to_vec(),
            ),
            (
                format!("{xscal}/FCMChat.ini"),
                b"[FCMChat]\nopenKey=INSERT\n".to_vec(),
            ),
            (
                "xScal (Install for xScal only)/xscal.ini".to_owned(),
                b"[Chat]\nenabled=true\nrelayEndpoint=wss://falloutchatmod.com/relay\n".to_vec(),
            ),
        ];
        let packages =
            inspect_source(&ImportSource::ZipBytes(package_zip(entries)?), "zfe", false)?;
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].files.len(), 3);
        assert_eq!(packages[0].release.version, "2.10.125");
        Ok(())
    }

    #[test]
    fn xscal_data_subfolder_uses_adjacent_ini_and_preserves_complete_package() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let data = temp
            .path()
            .join("Data (drag the contents into data folder)");
        fs::create_dir_all(&data)?;
        fs::write(data.join("FCMChatWidget.ba2"), b"BTDX 2.10.125")?;
        fs::write(data.join("FCMChat.ini"), b"[FCMChat]\nopenKey=INSERT\n")?;
        fs::write(
            temp.path().join("xscal.ini"),
            b"[Chat]\nenabled=true\nrelayEndpoint=wss://falloutchatmod.com/relay\n",
        )?;
        let packages = inspect_source(&ImportSource::Directory(data), "xscal", false)?;
        assert_eq!(packages.len(), 1);
        assert!(
            packages[0]
                .files
                .iter()
                .any(|(path, _)| path == Path::new("xscal.ini.example"))
        );
        Ok(())
    }

    #[test]
    fn incomplete_fcm_package_fails_instead_of_entering_generic_mod_flow() -> Result<()> {
        let bytes = package_zip(vec![(
            "FCMChatWidget.ba2".to_owned(),
            b"BTDX 2.10.125".to_vec(),
        )])?;
        let source = ImportSource::ZipBytes(bytes);
        assert!(contains_marker(&source, false)?);
        assert!(inspect_source(&source, "zfe", false).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "set FCM_HUD_ZIP, FCM_UNIFIED_HUD_ZIP, FCM_BRIDGE_ZIP, FCM_WINDOWS_ZIP, and FCM_LINUX_ZIP to local production packages"]
    fn validates_downloaded_production_packages() -> Result<()> {
        for (variable, expected_action) in [
            ("FCM_BRIDGE_ZIP", FcmAction::InstallBridge),
            ("FCM_WINDOWS_ZIP", FcmAction::InstallBridge),
            ("FCM_LINUX_ZIP", FcmAction::InstallBridge),
            ("FCM_HUD_ZIP", FcmAction::InstallHud),
            ("FCM_UNIFIED_HUD_ZIP", FcmAction::InstallHud),
        ] {
            let source = ImportSource::ZipFile(PathBuf::from(std::env::var(variable)?));
            assert!(contains_marker(&source, false)?);
            let packages = inspect_source(&source, "zfe", false)?;
            assert_eq!(packages.len(), 1, "{variable}");
            assert_eq!(packages[0].action, expected_action, "{variable}");
            if expected_action == FcmAction::InstallHud {
                let xscal = inspect_source(&source, "xscal", false)?;
                assert_eq!(xscal.len(), 1, "{variable} xScal");
                assert_eq!(xscal[0].action, FcmAction::InstallHud);
            }
        }
        Ok(())
    }
}
