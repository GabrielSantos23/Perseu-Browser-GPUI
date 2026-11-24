<p align="center">
  <img src="crates/app/assets/icon.png" width="128" alt="Perseu logo">
</p>

<h1 align="center">Perseu</h1>

<p align="center">A sidebar-first web browser for Windows, drawn natively with GPUI and powered by Microsoft Edge WebView2.</p>

---

## Features

- **Vertical tabs in a sidebar**: pinned tabs, folders, a favorites bar, and essentials (a grid of sites shared by every space).
- **Spaces**, each with its own tabs, colours and texture. Idle tabs move to an archive automatically.
- **Split view**, **peek** (hold a key while clicking a link to preview it), and a **command bar** for tabs, history, commands and search.
- **History page** with search, day grouping, ranges and clearing.
- **Private windows**: a separate process running InPrivate, so nothing it does is saved.
- **Import** bookmarks and history from Chrome, Edge, Brave, Helium, Firefox, Zen, Vivaldi, Opera and more.
- **Chromium extensions**, hosted by WebView2. Install them from the Chrome Web Store with the "Add to Perseu" button.
- **Media player** in the sidebar, with **Identify**, which names the song that is playing using Shazam.
- **Downloads panel**, site permissions, and per-site zoom.
- **Auto-updates** from GitHub Releases.

## Requirements

- Windows 10 1809 or later (Windows 11 recommended; Identify needs Windows 11, which can capture one app's audio).
- The [Evergreen WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/). It ships with Windows 11; the installer checks for it.

## Install

Download `perseu-setup-<version>.exe` from the [latest release](https://github.com/GabrielSantos23/Perseu-Browser-GPUI/releases/latest) and run it. No administrator rights are needed. Perseu checks for a newer release each time it opens (or from **Settings → About → Check for updates**), downloads it in the background and installs it when you close the browser. Pick **Restart to update** to install it right away.

## Build from source

You need the Rust toolchain. `rust-toolchain.toml` pins the version, and rustup installs it on first build.

```powershell
cargo run -p app            # debug build, starts Perseu
cargo test                  # unit tests
cargo clippy --all-targets -- -D warnings
```

To build the installer locally, install [Inno Setup 6](https://jrsoftware.org/isinfo.php) and run:

```powershell
./installer/build.ps1       # release build + dist\perseu-setup-<version>.exe
```

Set `PERSEU_SIGN_THUMBPRINT` to a code-signing certificate's thumbprint to sign the binaries.

## Releasing

Releases are built by GitHub Actions (`.github/workflows/release.yml`):

1. Bump `version` in the root `Cargo.toml`.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. The workflow runs the tests, builds the installer and publishes a GitHub release with it attached. Installed copies find it on their next update check.

To sign releases, add the repository secrets `SIGN_CERT_PFX` (base64 of a `.pfx`) and `SIGN_CERT_PASSWORD`.

## Project layout

| Path | What it is |
| --- | --- |
| `crates/browser-core` | Browser model: tabs, spaces, history, sessions, settings, import, extensions. No UI; unit tested. |
| `crates/webview-host` | WebView2 views: creation, bounds, navigation, events, downloads, permissions. |
| `crates/app` | The `perseu` binary: window, sidebar, command bar, pages, menus, theme. |
| `installer` | Inno Setup script and build script. |
| `docs` | [Architecture](docs/ARCHITECTURE.md), [security decisions](docs/SECURITY.md) and design notes. |

## Data

Session, settings, history and site permissions live in `%APPDATA%\perseu`. The WebView2 profile (cookies, cache, saved passwords), extensions and Identify's Shazam package live in `%LOCALAPPDATA%\perseu`. Set `PERSEU_DATA_ROOT` to keep everything in another folder, for example for testing.

## Credits

- [GPUI](https://github.com/zed-industries/zed) and [gpui-component](https://github.com/longbridge/gpui-component) for the UI, and [Wry](https://github.com/tauri-apps/wry) for WebView2.
- Some extension, Web Store and import code is adapted from [Zephium](https://github.com/zephium-browser/Zephium), and the media notes animation from [Zen Browser](https://github.com/zen-browser/desktop). Both are MPL-2.0, and those files keep their MPL notices.
- Icons: [Uicons by Flaticon](https://www.flaticon.com/uicons). Browser logos belong to their owners (see `crates/app/assets/browsers/SOURCES.md`).
- Identify uses Shazam's public web extension service. Perseu is not affiliated with Shazam or Apple.
