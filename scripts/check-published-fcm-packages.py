#!/usr/bin/env python3
"""Run the fork importer against the current production FCM packages."""

import json
import os
import subprocess
import tempfile
import urllib.parse
from pathlib import Path


BASE = "https://falloutchatmod.com"
MAX_DOWNLOAD_BYTES = 250 * 1024 * 1024


def download(url: str, destination: Path) -> None:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme != "https" or parsed.netloc != "falloutchatmod.com":
        raise ValueError(f"Unexpected FCM download host: {url}")
    if not parsed.path.startswith("/downloads/electron/"):
        raise ValueError(f"Unexpected FCM download path: {url}")
    subprocess.run(
        [
            "curl",
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "300",
            "--max-filesize",
            str(MAX_DOWNLOAD_BYTES),
            "--output",
            str(destination),
            url,
        ],
        check=True,
    )
    if destination.stat().st_size < 1_000:
        raise ValueError(f"FCM package is unexpectedly small: {url}")
    if destination.stat().st_size > MAX_DOWNLOAD_BYTES:
        raise ValueError(f"FCM package exceeds {MAX_DOWNLOAD_BYTES} bytes: {url}")


def main() -> None:
    feed = subprocess.run(
        ["curl", "--fail", "--location", "--silent", "--show-error", "--max-time", "30", f"{BASE}/api/releases"],
        check=True,
        capture_output=True,
        text=True,
    )
    releases = json.loads(feed.stdout)["data"]
    if not releases:
        raise ValueError("The production release feed is empty")
    hud_release = next(
        (
            item
            for item in releases
            if item.get("hudModUrl") and item.get("hudModVersion")
        ),
        None,
    )
    if hud_release is None:
        raise ValueError("The production release feed has no HUD package")
    version = releases[0]["version"]
    linux_name = f"Fallout Chat Mod-{version}.AppImage (Linux).zip"
    linux_url = f"{BASE}/downloads/electron/{urllib.parse.quote(linux_name)}"
    with tempfile.TemporaryDirectory(prefix="qc2-fcm-release-") as directory:
        root = Path(directory)
        hud_zip = root / "hud.zip"
        linux_zip = root / "linux.zip"
        download(hud_release["hudModUrl"], hud_zip)
        download(linux_url, linux_zip)
        print(f"Checking production overlay {version} and HUD {hud_release['hudModVersion']}", flush=True)
        env = os.environ.copy()
        env.update(
            FCM_HUD_ZIP=str(hud_zip),
            FCM_LINUX_ZIP=str(linux_zip),
            FCM_HUD_VERSION=hud_release["hudModVersion"],
        )
        subprocess.run(
            [
                "cargo",
                "test",
                "--manifest-path",
                "src-tauri/Cargo.toml",
                "validates_current_published_packages",
                "--",
                "--ignored",
                "--nocapture",
            ],
            check=True,
            env=env,
        )


if __name__ == "__main__":
    main()
