# perseu: Security

WebView2 (Chromium) owns process sandboxing, site isolation, TLS, Safe Browsing/SmartScreen verdicts and engine patches. This document covers what the host app owns: every decision, where it lives, and the test or manual check that confirms it.

Automated tests run with `cargo test`. Source-scanning tests in `crates/webview-host/src/hardening_tests.rs` fail the build if someone reintroduces a forbidden API. Manual checks were last performed on 2026-11-22 (Windows 11, Evergreen WebView2 Runtime).

## Decisions

### Runtime

- **Decision.** Only the Evergreen WebView2 Runtime is used; no fixed-version runtime is bundled or referenced.
  - At startup, `webview_host::runtime_version()` asks the WebView2 loader for the installed version.
  - If none is found, a native dialog offers Microsoft's download page and the app exits.
  - The installer warns as well.
- **Code.** `webview-host/src/lib.rs` (`runtime_version`), `app/src/lib.rs` (`run`), `app/src/native.rs` (`offer_runtime_install`), `installer/perseu.iss` (`InitializeSetup`).
- **Tests.**
  - `hardening_tests::evergreen_runtime_only` (no browser-executable-folder override).
  - Manual: the release build logs `WebView2 Runtime <version>` and starts. The missing-runtime dialog was **not** exercised; this machine has the runtime.

### JS ↔ Rust bridge

- **Decision.** Web pages have no route into the app:
  - no IPC handler, `WebMessageReceived` listener or injected script;
  - no host object (`AddHostObjectToScript`);
  - no custom protocol.
- **How events reach the host.** Only through engine events: navigation, title, favicon, downloads, permissions, new windows, external schemes and the DevTools `Security` domain. Each is read on the host side from WebView2's own event arguments, never from page-supplied JSON.
- **Phase 8 bridge removed.**
  - Peek (Phase 8) originally injected a click listener and posted links through wry IPC, which any page script could call.
  - It now uses `NewWindowRequested`. Chromium raises it for Shift/Ctrl+click; the host checks `IsUserInitiated`, the physically held modifiers and the URL scheme.
- **Web Store button.** The one script the host runs in a web page. While a tab shows a `chromewebstore.google.com` (or old `chrome.google.com/webstore`) listing, the app runs `store_page.js` (from Zephium, MPL-2.0) every 400 ms through DevTools `Runtime.evaluate`; nothing is registered on the page and no other site gets it. It hides the store's "Add to Chrome" and "switch to Chrome" prompts and adds an "Add to perseu" button in a closed shadow root. Each run returns only an extension id whose button got a trusted click; the app installs it only when it equals the listing id it reads from the tab's own URL, so the page can at most offer the extension it is showing. Nothing installs until the user presses Add on the card in the sidebar, which lists the access the manifest asks for (`extensions::review`). Once installed, the button removes that extension instead. Test `the_store_button_is_offered_for_its_listing`.
- **Extension popups.** No script is injected by the host. `extensions::adapt` writes `perseu-popup-tab.js` into the installed (unpacked) extension's folder and adds it as the first script of the popup page named by the manifest; a popup path that leaves the folder is refused. The script replaces `chrome.tabs.query` for `{active: true}` with the most recently used tab that is not an extension page, so the popup sees the page instead of itself. It runs with only that extension's own permissions, posts nothing, and the app reads nothing from it. Web pages never get it. The same step (`extensions::adapt`) puts `windows_create.js` first in the MV3 service worker named by the manifest: WebView2's `windows.create` opens the window but resolves with nothing, so the script finds the tab the call opened and answers with its window. It wraps only that one API, inside that extension. Next to it goes `tabs_create.js` (from Zephium, MPL-2.0), which does the same for `tabs.create` by matching the tab `tabs.onCreated` reports. Extension windows open as a tab whose view is handed over (`NewWindowRequested` deferral and `SetNewWindow`), only for `chrome-extension:` URLs; every view shares one WebView2 environment, which the handover requires. Tests `popup_pages_load_the_tab_script_first_once`, `service_workers_get_the_window_script_once`, `popups_outside_the_folder_are_refused`.
- **Internal pages.** The spec's internal-pages-on-a-custom-scheme option is not needed: settings, history and new tab are native GPUI views (a private tab's start page is a fixed HTML string shown with `NavigateToString`, with no script from outside), so there is nothing to protect with a CSP and no scheme to reach.
- **Code.** `webview-host/src/guard.rs`, `webview-host/src/lib.rs`.
- **Tests.**
  - `hardening_tests::web_pages_get_no_bridge_into_the_app` (scans for `with_ipc_handler`, `add_WebMessageReceived`, `AddHostObjectToScript`, initialization scripts, `AddScriptToExecuteOnDocumentCreated`, custom protocols).
  - `guard::tests::peek_needs_exactly_the_chosen_modifier`, `guard::tests::only_web_urls_can_be_peeked`, `settings::tests::unknown_peek_keys_fall_back_to_shift`.
  - Manual: Shift+click on example.com's link opens a peek.

### Internal pages

- **Decision.** No internal URL scheme exists, and the address bar no longer passes `perseu:` through, so typing it becomes a search. Web pages cannot navigate to `edge://` pages; Chromium blocks that.
- **Code.** `browser-core/src/omnibox.rs`.
- **Tests.** `omnibox::tests::internal_scheme_is_not_navigable`, `omnibox::tests::javascript_urls_are_searched_not_run`.

### Address bar integrity

- **What the field shows.**
  - It shows the host of the main frame's committed URL (`SourceChanged`), never the title or any page-provided text.
  - URLs without a host (`data:`, `file:`) are shown as written.
  - Hosts arrive from Chromium in punycode (`xn--…`) and are shown that way. This is stricter than "punycode only when suspicious": no IDN is ever rendered in Unicode, so no homograph can be displayed.
- **Registrable domain.** It is highlighted using the public suffix list (`psl`): `evil.github.io` highlights all of `evil.github.io`, not `github.io`. Subdomains are muted.
- **Padlock and warning.**
  - Driven by Chromium's own visible security state (DevTools `Security.visibleSecurityStateChanged`).
  - Plain `http:` always gets the warning.
  - The state is cleared at each navigation start, so a stale padlock never carries over.
- **Code.** `app/src/shell/sidebar.rs` (`render_url_field`), `browser-core/src/security.rs` (`registrable_domain`), `webview-host/src/guard.rs` (`watch_security_state`).
- **Tests.**
  - `security::tests::registrable_domains_follow_the_public_suffix_list`, `guard::tests::security_state_comes_from_the_devtools_event`.
  - Manual:
    - example.com shows a padlock, neverssl.com a warning, expired.badssl.com a warning;
    - a non-ASCII host displays as `xn--…`;
    - for badssl.com, `expired.` is muted and `badssl.com` highlighted.

### Certificate errors

- **Decision.** The app never handles `ServerCertificateErrorDetected` and never passes `--ignore-certificate-errors`. WebView2 keeps its default full-page interstitial; Chromium remembers a "proceed" only for the session.
- **Tests.**
  - `hardening_tests::certificate_errors_are_never_accepted_by_the_app`.
  - Manual: https://expired.badssl.com shows "Your connection isn't private" (NET::ERR_CERT_DATE_INVALID).

### Permissions

- **Requests wait for the app.** `PermissionRequested` is taken with a deferral (`SavesInProfile = false`, so WebView2 does not remember answers itself) and reported to the app.
- **Decisions.**
  - The app answers from its per-origin store (`permissions.json`), or shows a sidebar prompt: "<host> wants to <use your camera>" with Allow / Block.
  - Allow and Block are remembered per origin and permission.
  - Dismissing the prompt, navigating away, closing the tab or moving it to another space denies without remembering.
  - Pages without a web origin (`data:`, `file:`) and peeks are always denied.
- **Review.** Settings → Site permissions lists every decision with "Reset". Clear browsing data empties the store.
- **Code.** `webview-host/src/guard.rs` (`on_permission`, `answer_permission`), `app/src/shell/site_guard.rs`, `browser-core/src/permissions.rs`, `app/src/shell/settings_page.rs`.
- **Tests.**
  - `permissions::tests::*`, `security::tests::origins_drop_paths_credentials_and_case`.
  - Manual on permission.site:
    - Location → prompt → Block; a second click is blocked without a prompt;
    - Camera → prompt → dismissed → denied and not stored;
    - Settings lists `https://permission.site · geolocation — Blocked`, and Reset removes it.

### Popups and new windows

- **Decision.** Every new-window request is handled by the host; WebView2 never opens its own window.
  - Requests without a user gesture are blocked, and the sidebar shows "Pop-up blocked: <host>" with "Open once".
  - With a gesture, a page may open at most 5 windows, counted per page and reset on navigation. Further requests are blocked the same way.
  - An allowed window opens as a tab whose view the opener gets back, so `window.opener` works. If the opener closes first, its waiting requests are declined before its view goes away (finishing them later crashes WebView2), and the tab loads the URL by itself.
- **Code.** `browser-core/src/security.rs` (`may_open_window`, `MAX_WINDOWS_PER_PAGE`), `app/src/shell/site_guard.rs`, `webview-host/src/guard.rs` (`decline_windows_of`).
- **Tests.**
  - `security::tests::windows_need_a_gesture_and_stay_under_the_limit`.
  - Manual: a page calling `window.open` from a timer shows the blocked notice, and "Open once" opens it.

### External protocols and files

- **External protocols.**
  - `LaunchingExternalUriScheme` is always cancelled.
  - User-initiated requests are offered in a sidebar prompt ("<host> wants to open a mailto: link in another app" / Open). Only on Open does the app call `ShellExecuteW`.
  - Requests without a gesture are dropped.
  - Schemes the browser handles itself, and schemes known to be abused to run code, are refused outright: `ms-msdt`, `search-ms`, `ms-officecmd`, `ms-appinstaller`, `shell`, `hcp`, `javascript`, `vbscript`, `file`, and others.
- **Files.**
  - `file://` navigation from web pages is blocked by Chromium.
  - Downloaded files are never opened automatically.
  - SmartScreen stays on: wry's default browser arguments disable it (`msSmartScreenProtection`), so the app passes its own (`BROWSER_ARGS`) and sets `IsReputationCheckingRequired = true` explicitly.
- **Code.** `webview-host/src/guard.rs`, `webview-host/src/lib.rs` (`BROWSER_ARGS`, `require_reputation_checks`), `browser-core/src/security.rs` (`external_scheme`), `app/src/native.rs` (`launch_uri`).
- **Tests.**
  - `security::tests::external_schemes_exclude_web_and_abused_handlers`, `hardening_tests::browser_arguments_keep_smartscreen_and_gesture_gated_autoplay`.
  - Manual:
    - a `mailto:` link prompts instead of launching;
    - a `file:///C:/Windows/win.ini` link does nothing;
    - https://demo.smartscreen.msft.net/phishingdemo.html shows the Microsoft Defender SmartScreen block page (before Phase 11 it loaded).

### Downloads

- **Decision.**
  - Every download row shows its source host ("1.6 MB · the.earth.li").
  - Files that can run code (`.exe`, `.msi`, `.bat`, `.ps1`, `.js`, `.lnk`, `.hta`, macro-enabled Office files and more) are flagged in amber with "This type of file can run programs on your computer."
  - Opening a flagged file requires "Open anyway".
  - "Show in folder" never executes the file.
- **Code.** `browser-core/src/security.rs` (`is_dangerous_file`), `app/src/shell/downloads_panel.rs`.
- **Tests.**
  - `security::tests::executables_and_scripts_are_dangerous`, `downloads_panel::tests::status_lines_describe_each_state`.
  - Manual: putty.exe shows the warning, and clicking it shows "Open anyway / Cancel".

### Local data

- **Decision.** Session, settings, history (SQLite) and site permissions are stored **unencrypted** in `%APPDATA%\perseu\`. They are protected only by the Windows profile's ACLs, like other browsers' history databases.
  - The settings page says so next to "Clear browsing data".
  - DPAPI was not used: the SQLite history would need an encrypting SQLite build, and DPAPI only helps against attackers who are not already running as the user.
  - WebView2's own profile (cookies, passwords) is in `%LOCALAPPDATA%\perseu\WebView2` and is managed by Chromium, which encrypts saved passwords and cookies with a DPAPI-protected key (Chromium behavior, not verified here).
- **Clear browsing data.** Settings → Privacy → Clear browsing data… → Clear everything. It calls `ICoreWebView2Profile2::ClearBrowsingData(ALL_PROFILE)`, clears the history database (`DELETE` + `VACUUM`), empties site permissions and drops finished downloads from the list. If no page is open, the profile wipe runs as soon as the next page loads.
- **Private window.** A separate process (`perseu --private`) that never claims or forwards to the running instance, restores no session, keeps history in memory only and saves no session or site permissions. Its pages run InPrivate (`IsInPrivateModeEnabled` on each view's controller), so cookies, cache and storage live only until the last private view closes. Private tabs are kept outside the saved spaces (`Browser::private`): never in the session file, never archived, never in history, and gone without a trace when the private window closes. Extensions and "Clear browsing data" never use a private view (`any_view`). Test `browser::tests::the_private_window_is_never_saved_and_closes_without_a_trace`.
- **Identify (Shazam).** Pressing Identify records only this app's own audio: WASAPI process loopback of the WebView2 browser process and its children (`ICoreWebView2::BrowserProcessId`; Windows does not treat them as part of the app's own process tree), never other apps or the microphone, for at most 12 seconds and only while the button's try runs. The audio stays in memory; it is turned into a Shazam signature by the Shazam extension's own `sigx.wasm` in a Web Worker of a hidden page that loads no URL and gets no web content. Only that signature (plus a random session id and the extension's anonymous install id) is sent to `https://www.shazam.com/services/webrec/match_extensionv2`, as the extension does. The extension package is downloaded once from the Chrome Web Store into `%LOCALAPPDATA%\perseu\Shazam` and is not shipped with the app. Code `app/src/loopback.rs`, `app/src/shell/identify.rs`. Tests `identify::tests::*`, `loopback::tests::this_process_can_be_recorded` (ignored; needs an audio device).
- **Updates.** Release builds made by `.github/workflows/release.yml` carry the repository name (`PERSEU_UPDATE_REPO`); other builds never check. The app asks `api.github.com` for the latest release each time the browser opens (after 3 s), every 6 hours, and when the user presses "Check for updates" in Settings, offers it only when its version is higher and its installer is a `perseu-setup-*.exe` under that repository's own `github.com/<repo>/releases/download/` path, downloads it to the temp folder in the background, and runs the installer silently when the app closes (`/NORELAUNCH`, so it does not reopen) or at once when the user picks "Restart to update". Without code signing (the workflow signs when the `SIGN_CERT_PFX` secret is set) the update is trusted on HTTPS and GitHub alone. Code `app/src/updater.rs`. Tests `updater::tests::*`.
- **Importing from other browsers.** Settings → General → Import reads other browsers' files under `%LOCALAPPDATA%` and `%APPDATA%` only: Chromium's `Local State`, `Bookmarks` and `History`, Firefox's `profiles.ini` and `places.sqlite`. Profile folders named in `Local State` that contain a path separator are skipped. Databases are copied (with their `-wal` and `-journal` files) to a temp folder that is deleted after reading, so the other browser is never written to. Only `http`, `https` and `file` links are kept. Nothing leaves the computer. Tests `import::tests::*`, `history::tests::imported_visits_merge_with_our_own`.
- **Code.** `webview-host/src/lib.rs` (`clear_browsing_data`), `browser-core/src/history.rs` (`clear`), `app/src/shell/settings_page.rs`.
- **Tests.**
  - `history::tests::clearing_removes_stored_visits`, `store::tests::broken_files_are_set_aside`.
  - Manual: after clearing, Site permissions is empty, `permissions.json` is `{"sites":{}}`, and no error is logged.

### Private window

- **Not implemented** (optional in the spec). No in-memory profile is offered.

### Default browser registration and command line

- **Registration.**
  - The app writes its HKCU registration only when the user clicks "Make default…", and the installer writes the same keys. Windows itself requires the user's final confirmation.
  - The registered command is `"<exe>" "%1"`, with both parts quoted.
- **Command line.**
  - The app accepts no command-line flags at all. Each argument is either an http(s) URL or an existing local file (turned into a `file:///` URL); anything else, including `--anything`, is ignored.
  - URLs handed over by a second instance (`WM_COPYDATA`) are validated again, because any local process can send that message.
  - Nothing from the command line ever reaches WebView2's browser arguments.
- **Code.** `app/src/instance.rs`, `app/src/default_browser.rs`, `installer/perseu.iss`.
- **Tests.**
  - `instance::tests::crafted_arguments_cannot_become_flags`, `instance::tests::web_urls_pass_and_other_schemes_are_dropped`, `instance::tests::existing_files_become_file_urls`.
  - Manual (Phase 10): registration and Default apps.

### Supply chain and updates

- **CI.** `.github/workflows/ci.yml` runs fmt, clippy (`-D warnings`) and tests, then `cargo audit --deny warnings` and `cargo deny --locked check`. Any new advisory fails CI until it is fixed or reviewed.
- **Reviewed exceptions** (`.cargo/audit.toml`) are unmaintained-crate warnings with no known vulnerability:
  - from gpui and gpui-component: chrono, instant, paste, proc-macro-error, rustls-pemfile, rustybuzz, ttf-parser;
  - Linux-only crates never compiled for Windows: zbus_names, zvariant, zvariant_derive, and glib's unsound `VariantStrIter`.
  
  `deny.toml` (Windows target) needs no exceptions. It allows only common permissive licenses, rejects wildcard versions, and allows crates.io only, with no git sources.
- **Pinning.** Every direct dependency is pinned to an exact version where the spec asked (gpui, gpui-component, wry, webview2-com, rusqlite, psl, embed-resource), and `Cargo.lock` is used with `--locked`.
- **Signing.** `installer/build.ps1` signs the exe, installer and uninstaller (SHA-256, RFC 3161 timestamp) when `PERSEU_SIGN_THUMBPRINT` names a code-signing certificate. Without one it warns and builds unsigned. **No certificate is available yet, so current builds are unsigned.**
- **Updates.** The updater (above) trusts HTTPS and GitHub; it does not yet verify the installer. Once releases are signed it should verify the update's Authenticode signature (same publisher) before running it.
- **Checks.** `cargo audit --deny warnings` and `cargo deny --locked check` both pass locally (2026-11-22). CI has not run yet: the project is not in a git repository.

## Threat model

| Threat | What they control | What stops them |
|--------|-------------------|-----------------|
| **Malicious page** | Any script, links, popups, permission and protocol requests in the page | No host bridge (no IPC, host objects or custom scheme). Popups need a gesture and are capped. Permissions and external protocols need an explicit choice; dismiss means deny. Abused schemes are refused. The padlock comes from Chromium, not the page. Punycode hosts and the highlighted registrable domain beat look-alike domains. `file://` is blocked by Chromium. Certificate errors keep the interstitial. SmartScreen is on. |
| **Malicious download** | File name, type and contents | Never auto-opened. Executable types are flagged and need "Open anyway". The source host is shown. SmartScreen download reputation is on. Mark of the Web is Chromium's default on Windows; not separately verified here. |
| **Crafted URL on the command line or from another app** | Argument text, `WM_COPYDATA` payloads | No flags are parsed. Only http(s) URLs and existing files are accepted, and they are re-validated on the receiving side. The registered command quotes `%1`. Browser arguments are a compile-time constant. |
| **Local attacker reading `%APPDATA%`** | Files in the user's profile | Out of scope for encryption: history, session and permissions are plaintext (documented in the UI). Chromium encrypts cookies and passwords with a DPAPI-protected key (Chromium behavior, not verified here). "Clear browsing data" removes everything. An attacker running as the user can read the user's data in any browser. |

## Checklist

| Item | Covered by |
|------|-----------|
| Evergreen runtime only, startup check | Test `evergreen_runtime_only`; manual start (missing-runtime dialog not exercised) |
| No page-reachable bridge | Test `web_pages_get_no_bridge_into_the_app` + peek tests |
| Internal pages not navigable | Test `internal_scheme_is_not_navigable` |
| Address bar: committed URL, punycode, registrable domain, real security state | Tests `registrable_domains_follow_the_public_suffix_list`, `security_state_comes_from_the_devtools_event`; manual badssl/neverssl/example.com/IDN |
| Certificate errors not auto-accepted | Test `certificate_errors_are_never_accepted_by_the_app`; manual expired.badssl.com |
| Permissions prompt, store, review, default deny | Tests `permissions::*`, `origins_drop_paths_credentials_and_case`; manual permission.site |
| Popups need a gesture, capped, allow once | Test `windows_need_a_gesture_and_stay_under_the_limit`; manual timer popup |
| External protocols confirmed, abused schemes refused | Test `external_schemes_exclude_web_and_abused_handlers`; manual mailto |
| `file://` from web blocked, no auto-open, SmartScreen on | Test `browser_arguments_keep_smartscreen_and_gesture_gated_autoplay`; manual file link, SmartScreen demo |
| Downloads show origin, flag dangerous types, confirm | Tests `executables_and_scripts_are_dangerous`, `status_lines_describe_each_state`; manual putty.exe |
| Local data documented as plaintext; clear browsing data | Test `clearing_removes_stored_visits`; manual clear |
| Private window | Not implemented (optional) |
| Registration only on user action; argument validation | Test `crafted_arguments_cannot_become_flags` + instance tests; manual (Phase 10) |
| `cargo audit` / `cargo deny` in CI, signing, update verification | CI workflow, `audit.toml`, `deny.toml`, `build.ps1`; local runs pass; builds unsigned until a certificate exists; updater does not verify signatures yet |
