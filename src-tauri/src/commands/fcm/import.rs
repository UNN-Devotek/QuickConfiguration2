use std::collections::HashSet;
use std::fs::{self, File};
#[cfg(test)]
use std::io::Write;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use regex::bytes::Regex;
use semver::Version;
use zip::ZipArchive;
#[cfg(test)]
use zip::{ZipWriter, write::SimpleFileOptions};

use super::{
    FcmAction, FcmPackageInfo, FcmPlans, FcmPreview, hex_digest, make_plan_with_prerequisites,
    probe_prerequisites,
};

const MAX_NESTED_ZIP_BYTES: u64 = 32 * 1024 * 1024;

enum ImportSource {
    Directory(PathBuf),
    ZipFile(PathBuf),
    ZipBytes(Vec<u8>),
}

struct ImportedPackage {
    action: FcmAction,
    info: FcmPackageInfo,
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

#[cfg(test)]
fn package_zip(entries: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer.start_file(name, SimpleFileOptions::default())?;
        writer.write_all(&bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

fn local_package_info(version: String) -> FcmPackageInfo {
    FcmPackageInfo {
        version,
        source: "Imported package".to_owned(),
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
    ensure!(ba2.starts_with(b"BTDX"), "Invalid bridge BA2");
    let expected = metadata["ba2Sha256"]
        .as_str()
        .context("Bridge BA2 checksum is missing")?;
    ensure!(hex_digest(&ba2) == expected, "Bridge BA2 checksum mismatch");
    let info = local_package_info(version.to_owned());
    let files = vec![(PathBuf::from("Data/FCMServerBridge.ba2"), ba2)];
    Ok(ImportedPackage {
        action: FcmAction::InstallBridge,
        info,
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
    ensure!(
        ba2.starts_with(b"BTDX")
            && ba2
                .windows(version.len())
                .any(|window| window == version.as_bytes()),
        "Invalid HUD BA2 or version stamp"
    );
    let info = local_package_info(version.clone());
    let use_zfe = package_provider == "zfe";
    let mut files = vec![
        (PathBuf::from("Data/FCMChatWidget.ba2"), ba2),
        (
            PathBuf::from("Data/FCMChat.ini"),
            source.read(&chat_name, 100_000)?,
        ),
    ];
    if use_zfe {
        files.push((
            PathBuf::from("Data/ZFE/TextChat/fragments/FCMChatWidget.ini"),
            source.read(&zfe_name, 100_000)?,
        ));
    } else {
        files.push((
            PathBuf::from("xscal.ini.example"),
            source.read(&xscal_name, 10_000)?,
        ));
    }
    Ok(ImportedPackage {
        action: FcmAction::InstallHud,
        info,
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
                let bytes = source.read(name, MAX_NESTED_ZIP_BYTES)?;
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
                let nested = source
                    .read(name, MAX_NESTED_ZIP_BYTES)
                    .and_then(|bytes| contains_marker(&ImportSource::ZipBytes(bytes), true));
                match nested {
                    Ok(true) => return Ok(true),
                    Err(error) if lower.contains("fcm") || lower.contains("bridge") => {
                        return Err(error);
                    }
                    Ok(false) | Err(_) => {}
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
    requested_provider: Option<&str>,
    provider_package: Option<&str>,
    loader_package: Option<&str>,
    state: &FcmPlans,
) -> Result<FcmPreview> {
    let game = PathBuf::from(game_path);
    let probe = probe_prerequisites(&game)?;
    let selected_provider = probe
        .provider
        .as_deref()
        .or(requested_provider)
        .context("Choose ZFE or xScal to install")?;
    ensure!(
        selected_provider == "zfe" || selected_provider == "xscal",
        "Unsupported provider"
    );
    ensure!(
        requested_provider.is_none_or(|requested| requested == selected_provider),
        "Selected provider conflicts with the installed provider"
    );
    let mut prerequisites = Vec::new();
    if probe.provider.is_none() {
        let source = prerequisite_source(
            provider_package.context("Choose a provider package to download")?,
        )?;
        let names = source.names()?;
        let dll_name = unique_basename(&names, "dxgi.dll")?;
        let dll = source.read(dll_name, 100 * 1024 * 1024)?;
        let marker = if selected_provider == "zfe" {
            b"zfe-chat-v1".as_slice()
        } else {
            b"xscalchatv1".as_slice()
        };
        let other = if selected_provider == "zfe" {
            b"xscalchatv1".as_slice()
        } else {
            b"zfe-chat-v1".as_slice()
        };
        ensure!(
            dll.windows(marker.len())
                .any(|part| part.eq_ignore_ascii_case(marker))
                && !dll
                    .windows(other.len())
                    .any(|part| part.eq_ignore_ascii_case(other)),
            "Downloaded provider DLL does not match the selected provider"
        );
        prerequisites.push((PathBuf::from("dxgi.dll"), dll));
        if selected_provider == "xscal" {
            let ini_name = unique_basename(&names, "xscal.ini")?;
            let ini = source.read(ini_name, 100_000)?;
            String::from_utf8(ini.clone()).context("Invalid xScal INI")?;
            if !game.join("xscal.ini").exists() {
                prerequisites.push((PathBuf::from("xscal.ini"), ini));
            }
        }
    }
    let mut loader_default = None;
    if !probe.hud_mod_loader {
        let source = prerequisite_source(
            loader_package.context("Choose a HUDModLoader package to download")?,
        )?;
        let names = source.names()?;
        let ba2 = source.read(
            unique_basename(&names, "HUDModLoader.ba2")?,
            30 * 1024 * 1024,
        )?;
        ensure!(ba2.starts_with(b"BTDX"), "Invalid HUDModLoader BA2");
        let ini =
            String::from_utf8(source.read(unique_basename(&names, "hudmodloader.ini")?, 100_000)?)?;
        loader_default = Some(ini);
        prerequisites.push((PathBuf::from("Data/HUDModLoader.ba2"), ba2));
    }
    let mut packages = Vec::new();
    for source in source_paths(paths)? {
        let label = source.label();
        for mut package in inspect_source(&source, selected_provider, false)? {
            package.info.source = format!("Local package: {label}");
            packages.push(package);
        }
    }
    ensure!(
        packages.len() == 1,
        "Select exactly one complete FCM HUD or Server Bridge package"
    );
    let package = packages.pop().context("No FCM package was found")?;
    let (plan, preview) = make_plan_with_prerequisites(
        &game,
        Path::new(&ini_path),
        ini_prefix,
        package.action,
        Some(&package.info),
        package.files,
        Some(selected_provider),
        prerequisites,
        loader_default,
    )?;
    state
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("FCM plan lock is unavailable"))?
        .insert(preview.token.clone(), plan);
    Ok(preview)
}

fn prerequisite_source(path: &str) -> Result<ImportSource> {
    ImportSource::from_path(PathBuf::from(path))?
        .context("Prerequisite package must be a ZIP or folder")
}

fn unique_basename<'a>(names: &'a [String], expected: &str) -> Result<&'a str> {
    let matches: Vec<&str> = names
        .iter()
        .filter(|name| {
            name.rsplit('/')
                .next()
                .is_some_and(|base| base.eq_ignore_ascii_case(expected))
        })
        .map(String::as_str)
        .collect();
    ensure!(
        matches.len() == 1,
        "Package must contain exactly one {expected}"
    );
    Ok(matches[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::fcm::hex_digest;

    #[test]
    #[ignore = "requires FCM_HUD_ZIP, FCM_LINUX_ZIP, and FCM_HUD_VERSION from the published release"]
    fn validates_current_published_packages() -> Result<()> {
        let hud = ImportSource::ZipFile(PathBuf::from(std::env::var("FCM_HUD_ZIP")?));
        let linux = ImportSource::ZipFile(PathBuf::from(std::env::var("FCM_LINUX_ZIP")?));
        let expected_hud_version = std::env::var("FCM_HUD_VERSION")?;
        for provider in ["zfe", "xscal"] {
            ensure!(
                contains_marker(&hud, false)?,
                "Published HUD marker is missing"
            );
            let packages = inspect_source(&hud, provider, false)?;
            ensure!(packages.len() == 1, "Expected one {provider} HUD package");
            ensure!(
                packages[0].action == FcmAction::InstallHud,
                "Wrong HUD action"
            );
            ensure!(
                packages[0].info.version == expected_hud_version,
                "HUD package version disagrees with the release feed"
            );
        }
        ensure!(
            contains_marker(&linux, false)?,
            "Published bridge marker is missing"
        );
        let packages = inspect_source(&linux, "zfe", false)?;
        ensure!(packages.len() == 1, "Expected one bridge in the Linux ZIP");
        ensure!(
            packages[0].action == FcmAction::InstallBridge,
            "Wrong bridge action"
        );
        Ok(())
    }

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
    fn bridge_package_requires_its_ba2_checksum() -> Result<()> {
        let mut entries = bridge_entries("");
        entries[0].1 = serde_json::to_vec(&serde_json::json!({
            "version": "0.2.8",
            "target": "prod",
        }))?;
        let source = ImportSource::ZipBytes(package_zip(entries)?);
        let names = source.names()?;
        let error = match import_bridge(&source, &names, "BUILD.json") {
            Ok(_) => anyhow::bail!("The bridge package should require a checksum"),
            Err(error) => error,
        };
        ensure!(error.to_string().contains("checksum is missing"));
        Ok(())
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
    fn unrelated_nested_hud_file_stays_in_the_normal_mod_flow() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("other-mod.zip");
        fs::write(
            &path,
            package_zip(vec![
                ("assets/hud-art.zip".to_owned(), b"not a ZIP".to_vec()),
                ("Data/OtherMod.ba2".to_owned(), b"BTDX".to_vec()),
            ])?,
        )?;
        assert!(!detect_import(&[path.display().to_string()])?);
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
        assert_eq!(packages[0].info.version, "2.10.125");
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
    fn full_hud_zip_installs_missing_xscal_and_loader_in_one_rollback_plan() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game = temp.path().join("game");
        let ini = temp.path().join("ini");
        fs::create_dir_all(game.join("Data"))?;
        fs::create_dir_all(&ini)?;
        fs::write(game.join("Fallout76.exe"), b"")?;
        fs::write(
            ini.join("Fallout76Custom.ini"),
            b"[Archive]\nsResourceArchive2List=Other.ba2\n[Other]\nkeep=yes\n",
        )?;
        let hud = package_zip(vec![
            ("ZFE (Install for ZFE only)/INSTALL.txt".to_owned(), b"Fallout Chat Mod HUD 2.10.125 - ZFE (PRODUCTION)".to_vec()),
            ("ZFE (Install for ZFE only)/Data (drag the contents into data folder)/FCMChatWidget.ba2".to_owned(), b"BTDX zfe 2.10.125".to_vec()),
            ("ZFE (Install for ZFE only)/Data (drag the contents into data folder)/FCMChat.ini".to_owned(), b"[FCMChat]\nopenKey=INSERT\n".to_vec()),
            ("ZFE (Install for ZFE only)/Data (drag the contents into data folder)/ZFE/TextChat/fragments/FCMChatWidget.ini".to_owned(), b"[TextChat]\nenabled=true\n".to_vec()),
            ("xScal (Install for xScal only)/INSTALL.txt".to_owned(), b"Fallout Chat Mod HUD 2.10.125 - xScal (PRODUCTION)".to_vec()),
            ("xScal (Install for xScal only)/Data (drag the contents into data folder)/FCMChatWidget.ba2".to_owned(), b"BTDX xscal 2.10.125".to_vec()),
            ("xScal (Install for xScal only)/Data (drag the contents into data folder)/FCMChat.ini".to_owned(), b"[FCMChat]\nopenKey=INSERT\n".to_vec()),
            ("xScal (Install for xScal only)/xscal.ini".to_owned(), b"[Chat]\nenabled=true\nrelayEndpoint=wss://falloutchatmod.com/relay\n".to_vec()),
        ])?;
        let provider = package_zip(vec![
            ("dxgi.dll".to_owned(), b"MZ XSCALCHATV1".to_vec()),
            (
                "xscal.ini".to_owned(),
                b"[Chat]\nenabled=false\n[Other]\nkeep=yes\n".to_vec(),
            ),
        ])?;
        let loader = package_zip(vec![
            ("Data/HUDModLoader.ba2".to_owned(), b"BTDX loader".to_vec()),
            (
                "Data/hudmodloader.ini".to_owned(),
                b"OtherChild\nFCMChatWidget\nFCMChatWidget.swf\n".to_vec(),
            ),
        ])?;
        let hud_path = temp.path().join("hud.zip");
        let provider_path = temp.path().join("provider.zip");
        let loader_path = temp.path().join("loader.zip");
        fs::write(&hud_path, hud)?;
        fs::write(&provider_path, provider)?;
        fs::write(&loader_path, loader)?;
        let state = FcmPlans::default();
        let preview = preview_import(
            game.to_str().unwrap(),
            ini.to_str().unwrap(),
            "Fallout76",
            &[hud_path.to_string_lossy().into_owned()],
            Some("xscal"),
            Some(provider_path.to_str().unwrap()),
            Some(loader_path.to_str().unwrap()),
            &state,
        )?;
        assert_eq!(preview.provider, "xscal");
        assert!(
            preview
                .changes
                .iter()
                .any(|change| change.path.ends_with("dxgi.dll"))
        );
        let plan = state.0.lock().unwrap().remove(&preview.token).unwrap();
        super::super::apply_plan(plan, temp.path())?;
        assert_eq!(super::super::provider(&game)?, "xscal");
        assert_eq!(
            fs::read_to_string(game.join("Data/hudmodloader.ini"))?,
            "OtherChild\nFCMChatWidget\n"
        );
        assert!(fs::read_to_string(game.join("xscal.ini"))?.contains("keep=yes"));
        assert!(
            fs::read_to_string(ini.join("Fallout76Custom.ini"))?
                .contains("Other.ba2,HUDModLoader.ba2,FCMChatWidget.ba2")
        );
        assert!(fs::read_to_string(ini.join("Fallout76Custom.ini"))?.contains("keep=yes"));
        let repeat = preview_import(
            game.to_str().unwrap(),
            ini.to_str().unwrap(),
            "Fallout76",
            &[hud_path.to_string_lossy().into_owned()],
            None,
            None,
            None,
            &state,
        )?;
        assert!(repeat.changes.is_empty());
        fs::remove_file(game.join("dxgi.dll"))?;
        let reinstall = preview_import(
            game.to_str().unwrap(),
            ini.to_str().unwrap(),
            "Fallout76",
            &[hud_path.to_string_lossy().into_owned()],
            Some("xscal"),
            Some(provider_path.to_str().unwrap()),
            None,
            &state,
        )?;
        assert!(
            reinstall
                .changes
                .iter()
                .any(|change| change.path.ends_with("dxgi.dll"))
        );
        assert!(
            reinstall
                .changes
                .iter()
                .all(|change| !change.path.ends_with("xscal.ini"))
        );
        Ok(())
    }

    #[test]
    fn prerequisite_import_rejects_unrecognized_proxy_and_wrong_provider() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game = temp.path().join("game");
        let ini = temp.path().join("ini");
        fs::create_dir_all(game.join("Data"))?;
        fs::create_dir_all(&ini)?;
        fs::write(game.join("Fallout76.exe"), b"")?;
        fs::write(game.join("dxgi.dll"), b"another proxy")?;
        assert!(probe_prerequisites(&game).is_err());
        fs::remove_file(game.join("dxgi.dll"))?;
        fs::write(game.join("Data/HUDModLoader.ba2"), b"BTDX")?;
        fs::write(ini.join("Fallout76Custom.ini"), b"[Archive]\n")?;
        let provider_path = temp.path().join("zfe.zip");
        fs::write(
            &provider_path,
            package_zip(vec![("dxgi.dll".to_owned(), b"MZ zfe-chat-v1".to_vec())])?,
        )?;
        let bridge_path = temp.path().join("bridge.zip");
        fs::write(&bridge_path, package_zip(bridge_entries(""))?)?;
        let result = preview_import(
            game.to_str().unwrap(),
            ini.to_str().unwrap(),
            "Fallout76",
            &[bridge_path.to_string_lossy().into_owned()],
            Some("xscal"),
            Some(provider_path.to_str().unwrap()),
            None,
            &FcmPlans::default(),
        );
        assert!(result.unwrap_err().to_string().contains("does not match"));
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
