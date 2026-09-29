use super::*;

const BRIDGE_SHA256: &str = "9ae03b9e047c5fea221d10a8c805b0fe07cfcadb79af9667c766a4c627b54e44";

struct SavedFile {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

fn affected_files(game: &Path, ini: &Path) -> Vec<PathBuf> {
    vec![
        game.join("Data/hudmodloader.ini"),
        game.join("Data/FCMChat.ini"),
        game.join("Data/FCMChatWidget.ba2"),
        game.join("Data/FCMServerBridge.ba2"),
        game.join("xscal.ini"),
        ini.join("Fallout76Custom.ini"),
    ]
}

fn snapshot(paths: Vec<PathBuf>) -> Result<Vec<SavedFile>> {
    paths
        .into_iter()
        .map(|path| {
            let bytes = read_optional(&path)?;
            Ok(SavedFile { path, bytes })
        })
        .collect()
}

fn restore(saved: &[SavedFile]) -> Result<()> {
    ensure!(
        !game_running()?,
        "Fallout 76 started; restore the saved files after closing it"
    );
    for file in saved {
        match &file.bytes {
            Some(bytes) => atomic_write(&file.path, bytes)?,
            None if file.path.exists() => fs::remove_file(&file.path)?,
            None => {}
        }
    }
    for file in saved {
        ensure!(
            read_optional(&file.path)? == file.bytes,
            "Restoration mismatch: {}",
            file.path.display()
        );
    }
    Ok(())
}

fn make_fixture(source_game: &Path, source_ini: &Path, root: &Path) -> Result<(PathBuf, PathBuf)> {
    let game = root.join("game");
    let ini = root.join("ini");
    fs::create_dir_all(game.join("Data"))?;
    fs::create_dir_all(&ini)?;
    for relative in ["Fallout76.exe", "Data/HUDModLoader.ba2", "dxgi.dll"] {
        fs::copy(source_game.join(relative), game.join(relative))?;
    }
    for (source, target) in affected_files(source_game, source_ini)
        .into_iter()
        .zip(affected_files(&game, &ini))
    {
        if source.is_file() {
            fs::copy(source, target)?;
        }
    }
    Ok((game, ini))
}

fn import_preview(game: &Path, ini: &Path, zip: &Path, state: &FcmPlans) -> Result<FcmPreview> {
    let path = zip.display().to_string();
    ensure!(
        import::detect_import(std::slice::from_ref(&path))?,
        "FCM package not detected"
    );
    let preview = import::preview_import(
        &game.display().to_string(),
        &ini.display().to_string(),
        "Fallout76",
        &[path],
        None,
        None,
        None,
        state,
    )?;
    println!("{}", serde_json::to_string_pretty(&preview)?);
    Ok(preview)
}

fn apply_preview(preview: &FcmPreview, state: &FcmPlans, backups: &Path) -> Result<String> {
    let plan = state
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("FCM plan lock is unavailable"))?
        .remove(&preview.token)
        .context("Missing preview plan")?;
    let backup = apply_plan(plan, backups)?;
    ensure!(
        Path::new(&backup).join("manifest.json").is_file(),
        "Missing backup manifest"
    );
    println!(
        "Applied {} with backup {backup}",
        preview.package.as_ref().map_or("removal", |p| &p.version)
    );
    Ok(backup)
}

fn verify_mode(game: &Path, ini: &Path, action: FcmAction, sha256: &str) -> Result<()> {
    let loader = fs::read_to_string(game.join("Data/hudmodloader.ini"))?;
    let archive = fs::read_to_string(ini.join("Fallout76Custom.ini"))?;
    let entries: Vec<_> = ini_value(&archive, "Archive", "sResourceArchive2List")?
        .context("Missing archive list")?
        .split(',')
        .map(|item| item.trim().to_ascii_lowercase())
        .collect();
    let (chosen_loader, other_loader, chosen_ba2, other_ba2) = match action {
        FcmAction::InstallBridge => (
            "FCMServerBridge",
            "FCMChatWidget",
            "FCMServerBridge.ba2",
            "FCMChatWidget.ba2",
        ),
        FcmAction::InstallHud => (
            "FCMChatWidget",
            "FCMServerBridge",
            "FCMChatWidget.ba2",
            "FCMServerBridge.ba2",
        ),
        FcmAction::Remove => bail!("A removed mode has no BA2"),
    };
    ensure!(
        loader
            .lines()
            .filter(|line| is_fcm_loader_entry(line, chosen_loader))
            .count()
            == 1,
        "Chosen loader entry is not unique"
    );
    ensure!(
        loader
            .lines()
            .all(|line| !is_fcm_loader_entry(line, other_loader)),
        "Other loader entry remains"
    );
    ensure!(
        entries
            .iter()
            .filter(|item| item.as_str() == chosen_ba2.to_ascii_lowercase())
            .count()
            == 1,
        "Chosen archive entry is not unique"
    );
    ensure!(
        entries
            .iter()
            .all(|item| item != &other_ba2.to_ascii_lowercase()),
        "Other archive entry remains"
    );
    ensure!(
        !game.join("Data").join(other_ba2).exists(),
        "Other FCM BA2 remains"
    );
    ensure!(
        hex_digest(&fs::read(game.join("Data").join(chosen_ba2))?) == sha256,
        "Installed BA2 checksum mismatch"
    );
    Ok(())
}

fn unrelated_ini_lines(input: &str, excluded: &[&str]) -> Vec<String> {
    input
        .lines()
        .filter(|line| {
            !excluded.iter().any(|key| {
                line.split_once('=')
                    .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(key))
            })
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn unrelated_loader_lines(input: &str) -> Vec<String> {
    input
        .lines()
        .filter(|line| {
            !is_fcm_loader_entry(line, "FCMChatWidget")
                && !is_fcm_loader_entry(line, "FCMServerBridge")
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn exercise(
    game: &Path,
    ini: &Path,
    bridge: &Path,
    hud: &Path,
    backups: &Path,
    saved: &[SavedFile],
) -> Result<()> {
    let state = FcmPlans::default();
    let original_loader = fs::read_to_string(game.join("Data/hudmodloader.ini"))?;
    let original_custom = fs::read_to_string(ini.join("Fallout76Custom.ini"))?;
    let original_chat = read_optional(&game.join("Data/FCMChat.ini"))?;
    let original_xscal = fs::read_to_string(game.join("xscal.ini"))?;
    ensure!(
        current_mode(&original_loader)?.as_deref() == Some("HUD"),
        "Expected an existing visible HUD installation"
    );

    let bridge_preview = import_preview(game, ini, bridge, &state)?;
    ensure!(
        bridge_preview.action == FcmAction::InstallBridge,
        "Linux ZIP did not select the bridge"
    );
    ensure!(
        bridge_preview.provider == "xscal",
        "Expected the xScal provider"
    );
    ensure!(
        bridge_preview
            .package
            .as_ref()
            .is_some_and(|p| p.version == "0.2.9"),
        "Unexpected bridge version"
    );
    apply_preview(&bridge_preview, &state, backups)?;
    verify_mode(game, ini, FcmAction::InstallBridge, BRIDGE_SHA256)?;
    ensure!(
        read_optional(&game.join("Data/FCMChat.ini"))? == original_chat,
        "Bridge changed FCMChat.ini"
    );
    ensure!(
        fs::read_to_string(game.join("xscal.ini"))? == original_xscal,
        "Bridge changed xscal.ini"
    );
    ensure!(
        unrelated_loader_lines(&fs::read_to_string(game.join("Data/hudmodloader.ini"))?)
            == unrelated_loader_lines(&original_loader),
        "Bridge changed unrelated loader lines"
    );
    ensure!(
        unrelated_ini_lines(
            &fs::read_to_string(ini.join("Fallout76Custom.ini"))?,
            &["sResourceArchive2List"]
        ) == unrelated_ini_lines(&original_custom, &["sResourceArchive2List"]),
        "Bridge changed unrelated Custom.ini lines"
    );

    let repeat = import_preview(game, ini, bridge, &state)?;
    ensure!(
        repeat.changes.is_empty(),
        "Repeating the bridge install is not idempotent"
    );
    apply_preview(&repeat, &state, backups)?;

    let hud_preview = import_preview(game, ini, hud, &state)?;
    ensure!(
        hud_preview.action == FcmAction::InstallHud,
        "HUD ZIP did not select the HUD"
    );
    ensure!(
        hud_preview
            .package
            .as_ref()
            .is_some_and(|p| p.version == "2.10.134"),
        "Unexpected HUD version"
    );
    let source_sha = {
        let plan = state
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("FCM plan lock is unavailable"))?;
        let change = plan
            .get(&hud_preview.token)
            .context("Missing HUD plan")?
            .changes
            .iter()
            .find(|change| change.path.ends_with("FCMChatWidget.ba2"))
            .context("HUD plan has no BA2")?;
        hex_digest(change.after.as_deref().context("HUD plan removes BA2")?)
    };
    apply_preview(&hud_preview, &state, backups)?;
    verify_mode(game, ini, FcmAction::InstallHud, &source_sha)?;
    ensure!(
        read_optional(&game.join("Data/FCMChat.ini"))? == original_chat,
        "HUD changed customized FCMChat.ini"
    );
    ensure!(
        unrelated_ini_lines(
            &fs::read_to_string(game.join("xscal.ini"))?,
            &["enabled", "relayEndpoint"]
        ) == unrelated_ini_lines(&original_xscal, &["enabled", "relayEndpoint"]),
        "HUD changed unrelated xScal settings"
    );
    ensure!(
        unrelated_ini_lines(
            &fs::read_to_string(ini.join("Fallout76Custom.ini"))?,
            &["sResourceArchive2List"]
        ) == unrelated_ini_lines(&original_custom, &["sResourceArchive2List"]),
        "HUD changed unrelated Custom.ini lines"
    );
    ensure!(
        unrelated_loader_lines(&fs::read_to_string(game.join("Data/hudmodloader.ini"))?)
            == unrelated_loader_lines(&original_loader),
        "HUD changed unrelated loader lines"
    );
    ensure!(
        saved
            .iter()
            .all(|file| file.path.exists() || file.bytes.is_none()),
        "Unexpected missing file"
    );
    Ok(())
}

#[test]
#[ignore = "requires FCM_LINUX_ZIP, FCM_HUD_ZIP, FCM_GAME_DIR, and FCM_INI_DIR; live runs also require FCM_TEST_LIVE=1 and FCM_BACKUP_DIR"]
fn headless_production_linux_install() -> Result<()> {
    let bridge = PathBuf::from(std::env::var("FCM_LINUX_ZIP")?);
    let hud = PathBuf::from(std::env::var("FCM_HUD_ZIP")?);
    let source_game = PathBuf::from(std::env::var("FCM_GAME_DIR")?);
    let source_ini = PathBuf::from(std::env::var("FCM_INI_DIR")?);
    ensure!(
        bridge.is_file() && hud.is_file(),
        "Production ZIP is missing"
    );
    ensure!(
        !game_running()?,
        "Close Fallout 76 before the acceptance test"
    );
    let fixture = tempfile::tempdir()?;
    let live = std::env::var("FCM_TEST_LIVE").as_deref() == Ok("1");
    let (game, ini) = if live {
        (source_game, source_ini)
    } else {
        make_fixture(&source_game, &source_ini, fixture.path())?
    };
    let backup_root = if live {
        PathBuf::from(std::env::var("FCM_BACKUP_DIR")?)
    } else {
        fixture.path().join("backups")
    };
    println!(
        "Acceptance target: {}",
        if live {
            "live Proton profile"
        } else {
            "fixture"
        }
    );
    let saved = snapshot(affected_files(&game, &ini))?;
    let result = exercise(&game, &ini, &bridge, &hud, &backup_root, &saved);
    let restored = restore(&saved);
    if let Err(error) = restored {
        bail!("Restore failed after acceptance test ({result:?}): {error}");
    }
    result?;
    println!(
        "Original {} files restored and hash-equivalent",
        saved.len()
    );
    Ok(())
}
