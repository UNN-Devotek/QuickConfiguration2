# Build the focused FCM fork

Install Node.js, pnpm, Rust, and the Tauri 2 platform prerequisites. On Linux, WebKitGTK 4.1 and the packages listed in [Tauri's Linux prerequisites](https://v2.tauri.app/start/prerequisites/) are required.

```bash
pnpm install --frozen-lockfile
pnpm ui:lint
pnpm ui:vitest
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

To build only the AppImage, run `pnpm tauri build --bundles appimage`. If `linuxdeploy` cannot strip a system library, set `NO_STRIP=1`. The AppImage launcher script at `scripts/launch-appimage-linux.sh` handles the known NVIDIA WebKit renderer workaround and clears inherited AppImage environment variables.

The generated Tauri command bindings are in `src/commands/bindings.ts`. After changing the Rust command list, regenerate them with `cargo run --manifest-path src-tauri/Cargo.toml -- --export-bindings` and commit the result. The opt-in production package acceptance commands are documented in [FCM-INSTALL.md](FCM-INSTALL.md).
