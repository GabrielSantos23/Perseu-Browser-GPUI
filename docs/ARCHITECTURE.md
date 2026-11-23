# perseu: Architecture

## Crates

| Crate | Depends on | Owns |
|-------|-----------|------|
| `browser-core` | serde, serde_json, anyhow, rusqlite (bundled SQLite) | Tabs, spaces, essentials, history, session and settings model, plus their JSON/SQLite files. No GPUI, no Wry. Unit tested. |
| `webview-host` | wry, webview2-com, windows, raw-window-handle, anyhow, tracing | Creating, positioning, showing, hiding and navigating WebView2 child windows. |
| `app` (binary `perseu`) | gpui, gpui_platform, gpui-kit (component layer), gpui-ce-motion (math only), futures, raw-window-handle, windows, directories, browser-core, webview-host | Window, titlebar, sidebar, command bar, settings, theme. |

Versions are pinned in the root `Cargo.toml` under `[workspace.dependencies]`. A crate only lists a dependency once its code uses it; `unused_workspace_dependencies` is allowed so the pins can sit there in advance.

## Toolchain and GPUI version

- `rust-toolchain.toml` pins **Rust 1.100.0**. gpui-pre uses `slice_as_array` and `cfg_select!`, which Rust 1.91 does not have.
- **GPUI is `gpui-pre` 0.3.12**, imported under the name `gpui`. This is a crates.io snapshot of Zed's gpui (zed rev `d2a4fb6`). `gpui-component` 0.10.0 depends on exactly `gpui-pre =0.3.12`, so the official `gpui` 0.2.2 cannot be mixed in.
- gpui-pre moved platform backends into separate crates. `gpui_platform::application()` (crate `gpui-pre-platform`) picks the Windows backend (`gpui-pre-windows`).
- **gpui-component 0.10 has no `webview` feature**, so `webview-host` uses Wry directly. Wry 0.57 and gpui-pre-windows both use `windows` 0.62, so they share one copy.

## Window and titlebar (Phase 1)

- The window is created with `TitlebarOptions { appears_transparent: true }`. On Windows this sets `hide_title_bar`, so gpui does its own non-client hit testing.
- The titlebar is our own GPUI element (`app/src/title_bar.rs`). It does not use gpui-component's `TitleBar` because that one takes its colors from gpui-component's theme, and all our colors come from `theme.rs`.
- Each region declares a `WindowControlArea`:
  - `Drag` → `HTCAPTION`: drag to move, double-click to maximize, Aero Snap
  - `Min` → `HTMINBUTTON`
  - `Max` → `HTMAXBUTTON`, which makes Windows 11 show the **snap layouts flyout** when you hover it
  - `Close` → `HTCLOSE`
  
  Clicks on these regions are handled by Windows itself, so the buttons need no click handlers. A check with `WM_NCHITTEST` returned 8, 9, 20 and 2 for min, max, close and drag, and 1 (`HTCLIENT`) for the content area.
- The button glyphs come from the system icon font: **Segoe Fluent Icons** on Windows 11, falling back to **Segoe MDL2 Assets** on Windows 10. Both use the same code points (E921, E922, E923, E8BB). This needs no bundled icon assets.
- The view is wrapped in gpui-component's `Root`, so its dialogs, inputs and notifications work in later phases. Its theme is set to dark so its components match ours.
- The app quits when the last window closes.

## Shell layout and webview (Phase 2)

- **Layout.** A top strip holds back/forward/reload, a drag area, and our window controls. Below it are the sidebar (`sidebar_width`, 200–480px, dragged by a 6px handle) and the content frame (8px inset on the right and bottom, 10px radius, 1px border).
- **The resize handle sits entirely inside the sidebar.** The webview is a child window that takes every click inside its bounds, so a handle overlapping the frame edge cannot be grabbed. While dragging, gpui's `SetCapture` keeps mouse moves coming even when the pointer passes over the webview.
- **Keeping bounds in sync.** A `canvas` fills the frame. Its prepaint callback passes the frame's logical bounds plus `window.scale_factor()` to `BrowserView::set_bounds`. That runs every frame but does nothing when the bounds and scale are unchanged. Window resize, sidebar drag and DPI change all cause a re-layout, so all three go through this one path. Wry converts logical pixels to physical using the webview window's own DPI.
- **Rounded corners.** The webview's container window is clipped with `SetWindowRgn(CreateRoundRectRgn)` to the frame radius minus 1px. GDI regions are not anti-aliased, so the corner edge is slightly jagged. A smooth corner would need the WebView2 composition controller, which is a later polish item.
- **Events.** `webview-host` reports `WebViewEvent`s through an `EventSink` callback. Some come from wry (title changes); the rest come straight from WebView2 COM: `NavigationStarting`/`NavigationCompleted` drive loading, `SourceChanged` gives the committed URL, plus `HistoryChanged`, `FaviconChanged` (fetched as PNG with `GetFavicon`) and the controller's `GotFocus`. The app pushes each event into a `futures` channel and handles it in a `spawn_in` task. That way webview callbacks, which can fire in the middle of a webview call, never touch GPUI state re-entrantly.
- **Address field.** When not editing, the URL field is a pill showing `display_host(committed_url)`, the host without `www.`, which is never a value the page controls. Clicking it swaps in a gpui-component `Input` that is already focused with the full URL selected. Enter resolves the text with `browser_core::omnibox::resolve` (known schemes pass through, host-like text gets `https://` or `http://` for local hosts, everything else is a search), then moves focus into the page. Esc cancels. A `javascript:` input becomes a search, never a script.
- **Search engine.** `SearchEngine` is a URL template with `{query}`, with Google, DuckDuckGo, Bing and Brave built in. Google is the default until the Phase 9 settings page makes it a user setting.
- **Focus between GPUI and WebView2.**
  - At creation the webview is built with `with_focused(false)`, so the URL field keeps the keyboard.
  - Any left click on GPUI chrome calls `focus_parent()` (`SetFocus` on the main window).
  - When the webview reports `GotFocus`, GPUI focus is blurred, so the field goes back to the pill.
- The webview uses dark mode (`set_theme(Dark)`) and paints its background in the `surface` color so new pages don't flash white.
- **Known gap:** WebView2 currently uses its default user data folder next to the exe. Phase 9 moves it to `%APPDATA%/perseu/`.

## Tabs (Phase 3)

- **Model.** `browser_core::tabs::TabStrip` is one ordered `Vec<Tab>` with pinned tabs first, plus the active id and a reopen stack (at most 25 entries). Opening "after" a pinned opener lands at the top of the unpinned section. Closing the active tab selects its right neighbour, or else its left one. `move_before(id, target)` implements drop-on-row (the moved tab takes the target's section). `move_tab(id, section, index)` implements drop-on-divider and drop-at-end. All times are passed in (`Timestamp`, Unix ms), so tests are deterministic.
- **Runtime per tab.** In the app, `TabRuntime` holds the live `BrowserView`, the decoded favicon, loading and history state. The URL, title and favicon PNG bytes live in the core model.
- **One webview per tab, created lazily.** A tab gets its webview the first time it is activated with a non-blank URL, or when you navigate from its blank state. Inactive tabs stay unloaded until selected. That is the lazy-load mechanism; the user-facing setting arrives with session restore in Phase 9. Only the active view is visible (`set_visible`); hidden views keep running. Closing a tab drops its view, which destroys the WebView2.
- **Webviews are never created inside a GPUI update.** WebView2 creation runs a nested Win32 message loop while it waits. Inside an update, GPUI's own window messages then find the `App` already borrowed and get dropped (we saw "RefCell already borrowed" floods and frames lost). Instead, `ensure_view` takes the window's raw `HWND`, marks the tab `view_pending`, and creates the view in a `spawn_in` task between borrows. `attach_view` stores the result, or drops it if the tab was closed in the meantime. `BrowserView::new` takes a `RawWindowHandle` for this reason.
- **Keyboard shortcuts.**
  - A single table in `app/src/actions.rs` feeds both GPUI's keymap and the WebView2 shortcut filter, so the two cannot drift apart.
  - When a page has focus, WebView2's `AcceleratorKeyPressed` checks each Ctrl/Alt/F-key chord against that table. Only matching chords are marked handled and forwarded; everything else (Ctrl+C, Ctrl+F…) stays with the page. The app replays a forwarded chord with `window.dispatch_keystroke`, outside any entity update.
  - This part was planned for Phase 10. It was pulled forward because tab shortcuts are useless while the page has focus.
- **Keyboard focus.** GPUI focus stays inside `Shell` (`track_focus`, context `Browser`), so actions reach its handlers even while a page has OS focus. Starting a URL edit or switching tabs first calls `focus_parent()`, otherwise a hidden webview could keep the keyboard.
- **`window.open` / `target=_blank`.** These are denied at the WebView2 level and reopened as a normal tab right after the opener. *Limitation:* the new page has no `window.opener`, so popup-based sign-in flows that `postMessage` back to the opener will not complete. Phase 11 (popup policy) will revisit this with `NewWindowResponse::Create` on a shared environment.
- **Sidebar list.** Pinned rows, a divider (drop target: pin at end), a "New tab" row, then a scrolling unpinned list ending in a drop zone (unpin at end). Rows close via the hover close button or a middle click. Dragging uses GPUI `on_drag` / `drag_over` / `on_drop` with a `DraggedTab` preview.
- **Closing the last tab** opens a fresh blank tab instead of leaving the window empty.
- **Known gaps.**
  - Typed `data:` URLs load through wry's `NavigateToString`, so they report `about:blank` and show as "New tab".
  - Hover highlights on sidebar rows can stick after the pointer moves straight onto the page, because the webview takes mouse input and GPUI never sees the pointer leave.

## Spaces, essentials and archive (Phase 4)

- **Model.** `browser_core::browser::Browser` owns:
  - the spaces, each with a name, an emoji, a `SpaceTheme` (solid or gradient, as 0xRRGGBB) and its own `TabStrip`;
  - the shared essentials, at most 12;
  - the archive.
  
  Tab ids come from a global atomic counter, so they are unique across all spaces and the app's `TabId → TabRuntime` map spans every space.
- **What's on screen.** Each space remembers a `selected_essential`. The tab shown is that essential if set, otherwise the space's selected tab. Selecting a tab from another space switches to that space. Closing an essential resets it and falls back to the space's own tab.
- **Pinned tabs and essentials** store a `PinnedPage { url, title }`, captured when the tab is pinned. Closing one resets it to that page, drops its webview (`CloseOutcome::Unloaded`), and keeps the row with a muted title. A pinned tab that has navigated elsewhere shows a "/" before its title; clicking its favicon navigates back. A webview whose creation was still pending is discarded when it arrives, because `view_pending` is cleared on unload.
- **Drag and drop.**
  - Dropping a tab on the essentials grid makes it an essential. Dropping on a tile reorders.
  - Dragging a tile into the tab list turns it back into a pinned tab of the current space.
  - When there are no essentials, a dashed drop zone appears only while a drag is in progress.
- **Theming per space.** `Theme::space_palette(SpaceTheme)` derives the accent, the sidebar background (canvas tinted 12% toward the accent, or a two-stop linear gradient) and the content frame border (accent at 45%). Every accent use in the views goes through the palette, so the base theme keeps its single source of colors.
- **Switching.** Use the bottom-bar icons or Ctrl+Alt+1..9; this works from inside a page too, through the shared shortcut table. The header and tab list slide 24px and fade in over 200ms (ease-out quint), from the direction of the switch. Essentials and web content do not animate. Switching to an empty space opens a blank tab.
- **Editing a space.** Click the name to rename it inline (Enter or blur commits, Esc cancels). Clicking the emoji or the color dot steps through presets. A full editor comes with Settings (Phase 9).
- **Auto-archive.** A timer checks every `clamp(threshold / 4, 5s, 60s)`. It archives unpinned tabs in every space that have been idle longer than `archive_after_ms` (12h by default), except each space's selected tab. Their webviews are dropped. The archive list (newest first, at most 500) is the panel behind the archive button. Clicking an entry reopens it in the current space. `PERSEU_ARCHIVE_AFTER_SECS` overrides the threshold for testing until the Phase 9 settings page exists.
- **Known gaps.**
  - Ctrl+Alt+digit is AltGr+digit on many layouts (for example Brazilian ABNT2: AltGr+1 = ¹), so pages cannot receive those characters while the shortcut is bound. This will be configurable in Phase 9.
  - Space emoji render in monochrome at text size.
  - Delete space came in Phase 10: right-click a space in the bottom bar. Its tabs move to the archive, and pinned tabs come back unpinned when reopened.

## Command bar (Phase 5)

- **Separate window.** The bar is a separate GPUI window: `WindowKind::PopUp` (which gpui-pre gives `WS_EX_TOOLWINDOW | WS_EX_TOPMOST`), no titlebar, transparent background. DWM non-client rendering is disabled (`DWMWA_NCRENDERING_POLICY = DISABLED`), so Windows 11 draws no frame border or shadow around the transparent area; the panel draws its own. It is placed 56px below the top of the content frame, centered, at 60% of the frame width (420–680px). The frame bounds come from the same canvas that keeps webview bounds in sync. The bar's view is the window root (no gpui-component `Root`; `Input` does not need one).
- **Why a window.** A WebView2 always draws over GPUI content in the main window, so an in-window overlay would have to hide the page. A popup keeps the page visible underneath.
- **Opening and closing.**
  - Ctrl+T, the "+ New tab" row and empty spaces open it in *new tab* mode.
  - Ctrl+L and a click on the address pill open it in *current tab* mode, with the URL selected. This replaces the Phase 2 inline address editor, which has been deleted.
  - The bar closes on Enter, Esc, a click outside the panel, or losing activation.
  - In new-tab mode, a blank active tab is reused instead of opening another.
- **Close sequence.** This runs in a task between GPUI borrows: bring the main window to the front (raw `SetForegroundWindow`), wait 30ms for deactivation to settle, remove the popup, wait 30ms more, then run the chosen action. Calling `activate_window()` inside an update made GPUI drop the synchronous activation messages ("RefCell already borrowed"). Removing the popup before its deactivation arrived logged "window not found". Running an action that creates a webview before the popup was gone logged "invalid window handle". Actions run last, so a command that opens a new bar (New space) is not dismissed by the main window's activation.
- **Results.** Built from a snapshot taken when the bar opens:
  - open tabs from every space, most recent first, excluding the current tab;
  - pinned tabs and essentials;
  - commands (New space, Close tab, Reopen closed tab, View archive, Reload page);
  - history, minus URLs already open.
  
  As you type, an address entry (when the text resolves to something other than a search) goes on top, and a web search entry goes at the bottom. `browser_core::command_bar::rank` groups, scores (with `browser_core::fuzzy`, a subsequence matcher with prefix, word-start and run bonuses) and caps each group. Commands for compact mode, split view and settings will join the list in Phases 6, 7 and 9.
- **Keys.**
  - Typing filters the results.
  - Up/Down move the selection, Tab/Shift+Tab jump between groups, Enter confirms, Esc dismisses.
  - These are bound in the `CommandBar > Input` context. Later bindings win at equal depth, so they take precedence over the text input's own Up/Down/Tab/Escape.
- **History** is an in-memory `browser_core::history::History`, recorded from committed URLs (http, https and file only) with titles attached as they arrive, capped at 5000 entries. Phase 9 moves it to SQLite.
- **Known gaps.**
  - The popup is a fixed-height transparent window, so clicks in the empty area below the panel close the bar instead of reaching the page.
  - Moving or resizing the main window while the bar is open closes it, because the bar loses activation.
  - Results show group glyphs, not favicons.

## Compact mode (Phase 6)

- **Toggle.** Ctrl+S, or "Toggle compact mode" in the command bar. The shortcut lives in the shared table in `actions.rs`, so it can be rebound in one place; the Phase 9 settings page will expose it. Compact mode hides the sidebar, gives the content frame an 8px left inset (the hot zone, owned by GPUI), and shrinks the top strip's navigation area to just the buttons.
- **Hover detection by polling.** While compact mode is on, a 50ms task reads `GetCursorPos`. GPUI receives no mouse events while the pointer is over a WebView2, so it can never see the pointer leave the revealed sidebar onto the page. The reveal waits for 80ms of dwell in the hot zone (the window must be active); the hide waits for 400ms outside the sidebar. It stays open while a drag is active or the space is being renamed. The task only exists while compact mode is on.
- **Reveal strategy: prototyped and measured.** The test was the Rust article on Wikipedia, at a 180ms slide.

  | Strategy | Webview updates during slide | Host cost per update | Page reflow |
  |---|---|---|---|
  | Shrink the webview | 11 (move, resize and re-clip) | ~1.8 ms | Yes, every frame: Wikipedia switched to its narrow layout mid-animation |
  | **Clip the webview (kept)** | 14 (re-clip only) | ~0.65 ms | None |
  | Topmost popup window | not built | — | — |

  **Kept: clip.** The sidebar is drawn as a floating GPUI overlay inside the main window (rounded, with a shadow, sliding in from the left). The webview's window region (`SetWindowRgn`, already used for the rounded corners) gets a matching rounded cutout, `ContentBounds::cutout_width`, so GPUI shows through. The page never resizes. When only the cutout changes, `BrowserView::set_bounds` skips `SetWindowPos`.

  **Popup: not prototyped, by analysis.** Every Shell handler receives the `Window` it runs in. Rendered in a popup, a click on a tab would call `ensure_view` with the popup's HWND and parent new webviews to the popup. It would also need cross-window drag and drop (essentials, pinning) and activation juggling like the command bar's. Making it work means a separate event-routing layer, and the clip approach already gives an overlay with no reflow and no extra window.
- **Animation.** Progress is computed per frame from an `Instant`, with ease-out quint and `request_animation_frame` while moving. The overlay position and the webview cutout follow the same value. The hide animates too, and reversing mid-slide continues from the current position.
- **Known gaps.**
  - The overlay is not keyboard-reachable on its own: Ctrl+L, Ctrl+T and the shortcuts still work.
  - The hot zone is the left 8–12px of the window, including the edge of the screen when maximized.

## Split view (Phase 7)

- **Model.** `TabStrip` keeps `Split { first, second, orientation, ratio }` records, two panes per split as in the DESIGN_NOTES MVP. Invariants:
  - The two tabs are always adjacent, `first` then `second`, in `first`'s section. `keep_splits_together` runs after every insert and move, and dropping a tab onto the second pane of a split inserts it before the first instead.
  - Removing or closing either tab, including unloading a pinned one, dissolves the split.
  - Split tabs are never auto-archived.
  - Ratio is clamped to 0.15–0.85.
  
  Making a tab an essential removes it from the strip, which dissolves its split.
- **Creating a split.** Ctrl+click toggles tabs in a selection (at most 2, oldest dropped). "Split view" in the command bar pairs, in order of preference:
  1. the two selected tabs;
  2. the active tab with one selected tab;
  3. the active tab with the most recently used other tab in the space.
  
  "Unsplit view" and "Rotate split" appear only while the active tab is in a split; "Split view" appears only when a pair is possible. The tab context menu entry is deferred to Phase 10, where context menus get the popup-window treatment so they can extend over the page.
- **Content.** The active tab and its partner are both visible; every other webview is hidden. Each pane is its own rounded frame with its own canvas syncing its webview's bounds. The active pane gets a full-accent border. The 8px gap between panes is the divider: dragging it sets the ratio from the pointer position within the content bounds, and GPUI's mouse capture keeps the drag alive over the webviews. Vertical splits stack the panes and use a row-resize cursor. In compact mode, the overlay cutout applies to the panes that touch the left edge.
- **Focus.** Clicking into a pane makes WebView2 report `GotFocus`. If that tab is the active tab's partner, it becomes the active tab, so the address pill, back/forward/reload and Ctrl+L follow the pane you're using.
- **Sidebar.** A split renders as one grouped row: two half-rows (each with its own click, middle-click close, close button and drag) plus an unsplit button. Dragging either half moves the pair.

## Peek (Phase 8)

- **Trigger (since Phase 11).** Shift+click and Ctrl+click on a link make Chromium request a new window. The host's own `NewWindowRequested` handler reads `IsUserInitiated`, checks which modifiers are physically held (`GetAsyncKeyState`) and, when exactly the "Peek key" (Shift or Ctrl; Shift by default) is held on an http(s) URL, reports `PeekRequested` instead of `NewWindowRequested`. No page script or IPC is involved. Alt+click is a download in Chromium, so Alt is no longer offered; a saved `alt` reads as Shift. Phase 8 originally used an injected script plus wry IPC, which web pages could reach; that bridge was removed (see docs/SECURITY.md).
- **Window.** A borderless, transparent `WindowKind::PopUp` window covers the main window. Its backdrop is the theme `scrim` and closes the peek when clicked. A card sits over the content area, inset by `peek_margin` (at most 1/8 of the content). It has a header (title, host, "Open as tab", close) and a rounded frame whose canvas positions the peek's own webview, a child of the popup.
- **Model.** The peek owns a `Tab` that is created up front, so its id is known before the webview exists. Events for that id update the peek's tab, runtime and header instead of the strip. History records peek visits. New-window requests and peek requests inside the peek navigate the peek itself.
- **Closing.** Esc closes the peek, both from the page and from the header:
  - from the page, the peek webview's shortcut filter intercepts only a bare Esc while it is a peek;
  - from the header, the `Peek` key context binds Escape.
  
  Clicking the backdrop, or the popup losing activation (clicking the main window or another app), also closes it. Closing drops the webview first, then uses the command bar's sequenced removal: foreground main, 30ms, remove the popup, refocus the active page.
- **Open as tab.** `BrowserView::reparent` moves the live webview into the main window, so the page keeps its state and doesn't reload. It clears the cached bounds so the next layout places it. The shortcut filter flips to the browser shortcuts. `Browser::adopt` inserts the peek's tab after the active tab. A promoted tab peeks like any other.
- **Known.**
  - GPUI's `window.bounds()` for the main window sits about 4px above `GetWindowRect`, so the popup and card are offset by that much. The command bar has the same offset.
  - While peeking, the page cannot see Esc.

## Persistence and settings (Phase 9)

- **Where.**

  | Data | Location |
  |------|----------|
  | Session (`session.json`), settings (`settings.json`) and history (`history.sqlite`) | `%APPDATA%\perseu\` |
  | WebView2 user data folder (cookies, cache, storage) | `%LOCALAPPDATA%\perseu\WebView2`, one folder for every view, including peeks |

  `app/src/paths.rs` resolves these with `directories`. Every `BrowserView` gets its own wry `WebContext` pointing at that folder. The contexts are cheap and not shared, so overlapping view creations, which can nest inside each other's message loop, never contend for one context.
- **Files.** `browser_core::store` reads and writes JSON:
  - Writes go to `<file>.tmp` first and are then renamed over the file.
  - A file that fails to parse is renamed to `<file>.broken` and the app starts from defaults, so a later save cannot destroy it.
- **Session.**
  - `Browser::to_session` and `from_session` convert to plain serde structs in `browser_core::session`.
  - Tabs and spaces are saved by position, and ids are reassigned on restore. Indexes that point nowhere are dropped.
  - The session covers spaces (name, icon, colors, tabs, selected tab, shown essential, splits with orientation and ratio), essentials, the archive, and layout (compact mode, sidebar width).
  - Favicons are not saved; a restored tab shows the globe until its page loads.
- **Saving.**
  - `observe_self` on the shell schedules a save on every `notify`. At most one save is pending: it runs 1s after the first change and skips the write when the snapshot equals the last one written.
  - `on_app_quit` writes one final time.
  - Writes are synchronous on the UI thread; the file is a few KB.
- **Restore.** If the session has an active tab, startup shows it (only shown tabs create webviews) and skips the startup command bar.
- **History.** `History::open` creates a `visits` table (url primary key, title, visit count, last visit; WAL mode) and loads the 5,000 most recent entries into memory for the command bar. Each visit and title change is written right away. If a write fails, the error is logged and the in-memory entry is kept. If the database cannot open, history stays in memory for the session.
- **Settings.** `browser_core::settings::Settings` (serde with field defaults) holds:
  - search engine (by name);
  - accent (0xRRGGBB);
  - archive time (seconds, 0 = never);
  - peek key;
  - downloads folder (`None` = the system Downloads folder);
  - shortcut overrides (default keys → new keys).

  `Shell::update_settings` saves the file and applies only what changed:
  - **Accent:** the `Theme` global and gpui-component's selection color.
  - **Archive time:** `Browser::archive_after_ms`. `PERSEU_ARCHIVE_AFTER_SECS` still overrides it for testing.
  - **Downloads folder:** `ICoreWebView2Profile::SetDefaultDownloadFolderPath` on the live views. It is also applied when each view is created.
  - **Shortcuts:** rebound.

  The peek key is baked into each page's init script, so it applies to pages created after the change.
- **Accent.** Spaces keep their own colors. The app accent colors text selection, the command bar's selected row and the selected controls on the settings page. The UI stays dark (global rule), so there is no light theme.
- **Shortcuts.** The `SHORTCUTS` table stays the single source.
  - Each entry is keyed by its default keys. The active keys live in a static that both the gpui keymap and the WebView2 accelerator filter read.
  - Rebinding binds the new keys and binds `NoAction` to keys that are no longer used. Bindings added later take precedence in gpui's keymap, so this works without clearing other components' bindings.
  - `normalize_shortcut` accepts typed keys (`Ctrl+Shift+K`, `ctrl-shift-k`) and requires Ctrl or Alt unless the key is F1–F24, so typing in pages keeps working. Only keys the page filter can see are accepted: letters, digits, Tab and F-keys.
  - `edit_shortcut` refuses duplicates.
- **Settings page.** It renders in the content frame, and every webview is hidden while it is open. Open it with the gear in the bottom bar or "Settings" in the command bar. Close it with the gear, the close button, Esc while focus is on the page, or by selecting any tab. Its sections are:
  - choice chips: search engine, archive time, peek key;
  - accent swatches;
  - downloads folder: path, "Change…" (the system folder picker), "Use Downloads";
  - shortcuts: one text field per shortcut, applied on Enter or blur, with the error shown above the list.

## Downloads, menus, system integration and release (Phase 10)

- **Downloads.**
  - `webview-host/src/downloads.rs` subscribes to `DownloadStarting` on every view and sets `Handled`, which hides WebView2's own download popup.
  - It reports `DownloadStarted`, `DownloadProgress` (at most every 250ms) and `DownloadFinished` (completed, cancelled or failed). Ids come from a process-wide counter. Running operations live in a thread-local registry until they finish, which also serves `cancel_download(id)`.
  - `browser_core::downloads` keeps the list for the run (newest first, 100 entries) and is not saved. Events that arrive after a download has finished are ignored.
- **Downloads panel.**
  - It replaces the tab list, like the archive. The bottom-bar button turns the accent color while something is downloading, and a new download opens the panel.
  - Each row shows name, size or progress (a bar, dim when the size is unknown) and state. A finished row opens the file on click, has a folder button that runs `explorer /select`, and can be removed. A running row has a cancel button. "Clear" drops every finished row.
- **Download-only tabs.**
  - A tab that only led to a download leaves the list: it is unpinned, has no title, its URL is the download URL, and it never had a page to go back to. This matches common browser behavior.
  - Its webview is kept hidden in `download_hosts` until that download finishes. Closing the webview stalls the transfer, as observed when testing.
  - It is removed without entering the reopen-closed stack, so reopening it cannot download again.
- **Tab context menu.**
  - `context_menu.rs` is a generic popup-window menu (`ContextMenu<T>`). It supports item and danger entries and separators, hover and Up/Down selection, Enter to choose, and Esc or losing activation to dismiss.
  - Right-click on a tab row, half of a split row, or an essential opens the tab menu at the cursor. Right-click on a space button opens Rename space / Delete space (shown only when another space exists). The menu moves up when it would pass the window's bottom edge.
  - Entries shown depend on the tab: Reload, Duplicate and Copy link (not on blank tabs); Pin/Unpin tab; Add to or Remove from essentials; Split with current tab or Unsplit view; Close tab.
  - The command bar, peek and menu all close through `Shell::dismiss_popup`: foreground the main window, wait 30ms, remove the popup, wait 30ms, then run the follow-up.
- **Focus and shortcut forwarding.**
  - Alt+Left / Alt+Right (back/forward) join the shortcut table, and the WebView2 key filter now maps the arrow keys.
  - `page_had_focus` is set when a page reports `GotFocus` and cleared whenever the chrome takes the keyboard. When the main window is activated again (Alt+Tab, closing a popup) and the page had focus, the page gets it back.
- **Command line and single instance.**
  - `instance::accepted_target` keeps only `http(s)` URLs and existing files, which become `file:///` URLs.
  - The first process holds the `Local\perseu.instance` mutex and a message-only window (`perseu.ipc`). A later launch sends its targets with `WM_COPYDATA` (tagged, newline-separated UTF-16) after `AllowSetForegroundWindow`, then exits. It retries for 2s while the first instance starts.
  - The receiver validates the targets again, because any local process can send that message. It restores and foregrounds the window and opens each target, reusing a blank active tab.
  - On a first launch with targets, the startup command bar is skipped.
- **Default browser.**
  - Windows 10/11 do not let apps make themselves the default. `default_browser::register` writes the per-user (HKCU) registration:
    - `Software\Clients\StartMenuInternet\perseu` with its `Capabilities` (http, https, .htm, .html → `perseuHTML`);
    - `RegisteredApplications`;
    - the `perseuHTML` ProgID, whose open command is `"exe" "%1"`.
  - It then opens `ms-settings:defaultapps?registeredAppUser=perseu` with `ShellExecuteW`. Explorer misreads that URI, so it isn't used for this.
  - The settings page shows "Make default…" until `UrlAssociations\https\UserChoice\ProgId` is `perseuHTML`.
  - The installer writes the same keys, and its uninstaller removes them.
- **Icon and version.**
  - `crates/app/assets/icon.ico` (16–256px) and `icon.png` are an original design: a dark rounded square with an accent sidebar strip and a light content panel.
  - `crates/app/build.rs` compiles `app.rc` with `embed-resource`: icon resource 1, which gpui loads for its windows, plus `VERSIONINFO` built from the package version. It reads the app name from `browser_core::APP_NAME`.
- **Release.**
  - `[profile.release]` uses `opt-level = 3`, thin LTO, one codegen unit and `strip`.
  - The binary is always a GUI subsystem app.
  - Logging goes to stderr in debug builds and to `%LOCALAPPDATA%\perseu\perseu.log` (truncated each launch) in release builds. The default filter is `warn`, and `RUST_LOG` overrides it.
- **Installer.**
  - `installer/perseu.iss` (Inno Setup 6) installs per user (`PrivilegesRequired=lowest`) into `%LOCALAPPDATA%\Programs\perseu`, x64, Windows 10 1809+. It adds a Start menu entry, an optional desktop icon and the browser registration, and offers to launch the app.
  - It warns when the WebView2 Runtime is missing.
  - Uninstall removes the program and the registration but keeps user data in `%APPDATA%` and `%LOCALAPPDATA%`.
  - `installer/build.ps1` runs `cargo build --release` and then `ISCC`, writing `dist\perseu-setup-<version>.exe`. The version comes from the exe's `ProductVersion`.

## Security hardening (Phase 11)

Every decision, its test and the threat model are in [docs/SECURITY.md](SECURITY.md). In short:
- **No page bridge.** Web pages have no route into the app: no IPC, no host objects, no injected scripts, no custom scheme. Source-scanning tests enforce this.
  - *One scoped exception:* an extension's toolbar popup shows in a view of its own, so its `tabs.query({active: true})` would find the popup itself. `extensions::adapt` adds a script file to the installed extension's own folder and loads it first in its popup page. It answers that query with the most recently used tab that is not an extension page. It runs only inside that extension, sends nothing to the app, and web pages never get it. (A DevTools `Page.addScriptToEvaluateOnNewDocument` script was tried first: WebView2 never runs it on `chrome-extension:` pages.)
  - *Second exception:* on Chrome Web Store listings only, `store_button` runs Zephium's `store_page.js` through DevTools `Runtime.evaluate` every 400 ms to show "Add to perseu"; a run returns only the clicked listing's id, and the app installs it only if it matches the tab's URL.
  - Extension windows (`windows.create`, `window.open` from an extension page) open as a tab whose view takes over the pending window, as Zephium does, instead of a card.
- **WebView2 setup.**
  - Explicit browser arguments keep SmartScreen on (wry's defaults turned it off) and keep Chromium's autoplay policy.
  - The Evergreen runtime is checked at startup.
- **Prompts in the sidebar.** Popups, permissions and external protocols each get a GPUI notice under the address field:
  - script-only popups are blocked, with an "Open once" option, and each page may open at most 5 windows;
  - permission requests wait for an answer through a deferral, decisions are stored per origin, and dismissing a prompt means deny;
  - external protocols are confirmed before launch, abused schemes are refused, and only user-initiated launches are offered.
- **Address field.** It shows a padlock or warning from Chromium's own security state (the DevTools `Security` domain) and highlights the registrable domain (public suffix list). The host is the committed URL's punycode form.
- **Downloads.** Each row shows its source site. Executable types are flagged and need "Open anyway".
- **Settings.** Site permissions (with Reset) and Clear browsing data, which wipes the WebView2 profile, history and permissions.
- **Phase 10 gaps closed.** The tab menu gets "Move to <space>", and the download button gets a progress bar.

## Renaming the app

The codename lives in:
- `browser_core::APP_NAME`, which also feeds `build.rs`, the data folders, the instance mutex and window class, the registry keys and ProgID, and the log file;
- the `[[bin]] name` in `crates/app/Cargo.toml`;
- `#define AppName` in `installer/perseu.iss`.

The installer's registry entries mirror `default_browser.rs` by hand.

## Theme

`app/src/theme.rs` defines `Theme { colors, metrics, fonts }`, stored as a GPUI `Global` and read with `Theme::of(cx)`. Views never hardcode colors or sizes. The values come from the palette in `DESIGN_NOTES.md` §8. `sidebar_tint` is computed as the canvas color with the space accent blended in at 12%.

Contrast, checked by hand:
- `text_muted` on `sidebar_tint` is about 5.4:1 (passes AA).
- `text` on `surface` is about 14:1.
- `text_faint` is about 2.9:1. It is reserved for disabled or unloaded items, which WCAG exempts. Do not use it for text the user needs to read.

## Webview overlay decisions

A WebView2 is a native child window and always draws on top of GPUI content inside its bounds. Plan per feature (to be confirmed in each phase):

| Feature | Decision |
|---------|----------|
| Sidebar, titlebar, bottom bar | Outside the webview bounds. No conflict. |
| Command bar (Phase 5) | **Done:** borderless transparent topmost GPUI popup over the content frame (see the Phase 5 section). |
| Context menus over content (Phase 10) | **Done:** borderless transparent popup GPUI window sized to the menu at the cursor (see the Phase 10 section). |
| Compact-mode sidebar reveal (Phase 6) | **Done:** in-window GPUI overlay with a matching cutout in the webview region (measured against shrinking; see the Phase 6 section). |
| Peek (Phase 8) | **Done:** full-window transparent popup with a scrim backdrop and its own child webview; "Open as tab" reparents that webview into the main window (see the Phase 8 section). |
| Toasts and permission prompts (Phases 10–11) | Shown in sidebar space, or in a small topmost window when they must point at content. |
| Hidden title bar reveal | **Done:** with Settings → Hide title bar on, the title bar leaves the layout and slides down as an in-window GPUI overlay when the pointer reaches the top edge. Each webview gets a rectangular top cutout, `ContentBounds::cutout_top`, as tall as the part of the bar over it, the same technique as the compact sidebar. Compact mode and the title bar share one pointer poll. |
| Site menu, toasts, tooltips | **Done:** the site menu (from the title bar's sliders button) and toasts are borderless popup windows like the context menus. Tooltips are in-window GPUI, so buttons near the page open theirs away from it. |

## UI kit and motion

- **GPUI Kit.** The app depends on `gpui-kit =0.10.0` with only the `component` feature. Its component layer is the same `gpui-component` 0.10.0 used before, on the same `gpui-pre =0.3.12`, so imports read `gpui_kit::component::…`. `theme::apply_to_components` copies our `Theme` colors into Kit's theme (primary = space accent, text, borders, surfaces) every time the accent or the light/dark scheme changes. That way Kit's `Switch`, `Button` and `Input` match the chrome. Settings uses Kit's `Switch` and `Button` for its toggles and option chips, and the site menu uses `Button`.
- **What stays hand-built.** Anything that must draw over the page stays a separate popup window: tooltips, menus, the site menu, the theme picker and toasts. Kit's in-window `Tooltip`, `Popover` and `Menu` would be covered by the WebView2 child window. The sidebar rows, the title bar and the Zen theme picker keep their own drawing because their look is the design.
- **Rake was not adopted.** `rake-app` 0.1.0-alpha.5 pins `gpui-pre =0.3.11` and `gpui-kit` 0.9.0, which cannot share a build with our `=0.3.12` pins. It is also an alpha application framework (`view!` macro, routing) that would replace the whole shell.
- **Motion.** gpui-motion's GPUI adapters target other GPUI builds: `gpui-motion-kit` 0.5 pins `gpui-kit =0.7.3` (gpui-pre 0.3.9), and `gpui-ce-motion`'s `gpui` feature targets `gpui-ce`. We use `gpui-ce-motion =0.4.0` with default features off, which is pure math with no GPUI. `app/src/motion.rs` wraps it:
  - `Toggle` is a `MotionValue` on a bounce-free spring, so reversing it mid-way keeps its velocity. It drives the compact sidebar and the hidden title bar. Render samples it once per frame and requests another frame while it moves.
  - `curve(Easing)` turns a gpui-motion tween into the 0..1 curve that gpui's `with_animation` expects. It is used for the space slide, the media card expanding, the media note bob and the loading sweep.
  - **Reduced motion.** When Windows' "Animation effects" setting is off (`SPI_GETCLIENTAREAANIMATION`), the springs and curves jump to their end, the note stops bobbing and the loading bar is a static strip. The setting is read once at startup.
- **Sidebar row motion** (`shell/row_motion.rs`). Rows slide to their new place with FLIP-style layout animation. Each tab row remembers its slot (its top within its list: unpinned, loose pinned, or one folder), and when the slot changes it starts from where it was last drawn and springs back to 0. Rows that are new in a list that was already showing fade in; rows on first show or after a space switch don't. While a tab is dragged, its row leaves its list and an empty gap follows the pointer (`on_drag_move` on the list, slot = pointer y / row height). The list takes the drop, and the dropped row glides in from the pointer. Rows inside folders keep per-row drop targets. The divider shows "Drop to pin" during a drag.
- **Menu fade.** Context menus fade in on open. On close they fade out for `MENU_FADE` while the chosen action already runs; the popup window is removed after the fade (`Shell::fade_out_popup`).
