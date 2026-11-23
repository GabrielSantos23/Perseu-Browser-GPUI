# perseu: Design Notes (Phase 0)

These notes cover the interaction design of **Arc** (The Browser Company) and **Zen Browser** (a Firefox fork). They are the reference for building `perseu`. We copy *behaviors* only. We do not copy names, logos, icons, artwork or assets.

**Source confidence.** Each claim is tagged:
- **[official]**: the vendor's own help center, docs or release notes.
- **[3rd]**: reviews, guides or press.
- **[inferred]**: our own reading of screenshots or videos, or a decision we made where no source exists.

**Context (Nov 2026).** Arc went into maintenance mode in May 2025. Atlassian bought The Browser Company (announced Sep 2025, closed Oct 2025), and Arc's sunset is reported for **22 Dec 2026**, with users moved to Dia [3rd: [TechCrunch](https://techcrunch.com/2025/11/03/dias-ai-browser-starts-adding-arcs-greatest-hits-to-its-feature-set), [AlternativeTo](https://alternativeto.net/news/2026/10/the-browser-company-confirms-arc-browser-sunset-directs-users-to-ai-powered-successor-dia/), [The Register](https://www.theregister.com/2025/05/27/arc_browser_development_ends/)]. Arc's help center may go offline. Zen is still under active development, and its docs are the more stable reference.

---

## 1. Sidebar anatomy

Arc's sidebar, top to bottom [official + 3rd: [Arc Favorites](https://resources.arc.net/hc/en-us/articles/19230755904151), [allthings.how pinned tabs](https://allthings.how/how-do-pinned-tabs-work-in-arc-browser/), [TidBITS](https://tidbits.com/2023/05/01/arc-will-change-the-way-you-work-on-the-web/)]:

| # | Element | Notes |
|---|---------|-------|
| 1 | Window controls row | Traffic lights on macOS. On Windows, Arc puts its own min/max/close in the sidebar's top row, plus back, forward and reload icons on the same row [inferred from screenshots]. |
| 2 | URL / command field | A pill that shows the **domain only**, not the full URL. Clicking it opens the command bar, not an inline editor [inferred]. |
| 3 | Favorites grid | Square favicon tiles in rows of 3 or 4. They are shared across all Spaces. Arc allows up to 12 [official]. |
| 4 | Space name | Small label with the Space icon. In Arc it sits near the pinned section. In Zen the workspace name sits above the pinned tabs [inferred]. |
| 5 | Pinned tabs | Scoped to the Space. They can be put in folders, and folders can nest [3rd]. |
| 6 | Divider | A thin line with a "Clear" action on hover, which archives all unpinned tabs [inferred]. Dragging a tab above the line pins it [3rd]. |
| 7 | "+ New Tab" row, then unpinned ("Today") tabs | Newest at the top in Arc [inferred]. |
| 8 | Bottom bar | Left: library and downloads. Center: one icon per Space (dots or emoji). Right: "+" for a new space, folder or tab [3rd: [allthings.how folders](https://allthings.how/?p=8915)]. |

Zen uses the same layout. Essentials sit at the top, then workspace tabs, and workspace icons sit at the bottom of the sidebar [official: [Zen Workspaces](https://docs.zen-browser.app/user-manual/workspaces)].

**Our order:** titlebar row (our min/max/close plus nav buttons) → URL pill → essentials grid → space header → pinned → divider with "Clear" → "+ New tab" → unpinned → bottom bar (downloads · space dots · "+").

## 2. Spaces / workspaces

- **Switching**
  - Arc: click a Space icon in the bottom bar, two-finger swipe across the sidebar, or keyboard. Guides list Cmd+Option+→/← for next and previous Space on macOS, and either Ctrl+1..9 or Alt+1..9 for Space number on Windows [3rd: [allthings.how Spaces on Windows](https://allthings.how/how-to-manage-spaces-in-arc-browser-on-windows), [Hongkiat](https://www.hongkiat.com/blog/arc-browser-keyboard-shortcuts/)]. Sources disagree on the exact keys.
  - Zen: icons at the bottom of the sidebar. Alt+Ctrl+Q and Alt+Ctrl+E go backward and forward. "Switch to workspace 1..10" exists but has no default binding [official: [Zen Shortcuts](https://docs.zen-browser.app/user-manual/shortcuts)].
- **Per-space look**
  - Arc: each Space has its own theme (color or gradient plus grain), edited from the command bar with "Theme" [official: [Arc release notes Dec 2022](https://resources.arc.net/hc/en-us/articles/20498417809815)].
  - Zen: right-click the sidebar → "Change Theme Colors" sets the gradient, noise and opacity for each workspace [official: [Welcome to Zen](https://docs.zen-browser.app/welcome-to-zen); 3rd: [GitHub discussion #8940](https://github.com/zen-browser/desktop/discussions/8940)].
- **Content:** each Space has its own pinned and unpinned tabs. Favorites/Essentials are global [official + 3rd: [DeepWiki Zen workspaces](https://deepwiki.com/zen-browser/desktop/3.1-workspaces-system)].
- **Switch animation:** the sidebar content slides horizontally, in the direction of the swipe, and the theme color crossfades [inferred from videos]. The web content does not animate. It just swaps to the target Space's last active tab.

**Our plan:** Ctrl+Alt+1..9 jumps to a space and Ctrl+Alt+←/→ steps through them. The sidebar content slides about 24px and fades over 200ms, and the accent crossfades.

## 3. Tab behavior

- **Pinned vs unpinned.** Pinned tabs live for the Space, like bookmarks that hold a live page. Unpinned tabs are short-lived [3rd].
- **Auto-archive.** Unpinned tabs move to the Archive after **12h** of inactivity by default. The timer can be set from 12h to 30 days and cannot be turned off. Pinned tabs are never archived. Archived tabs are found with Ctrl+T → "View Archive" and can be restored [official: [Arc Archive](https://resources.arc.net/hc/en-us/articles/19228855311127)].
- **Pinned URL reset.** A pinned tab or Favorite always goes back to its original URL. If you browse away from it, a **"/" appears next to the tab name**, and clicking the favicon resets it. "Edit Pinned Page → Replace Pinned URL with Current" changes the saved URL [official: [Arc slash](https://resources.arc.net/hc/en-us/articles/25625148480279-Why-is-There-a-Slash-Next-to-My-Pinned-Tab-s-Name), [Arc reverting](https://resources.arc.net/hc/en-us/articles/25541939922199-Why-Are-My-Pinned-Tabs-and-Favorites-Reverting-Back-to-Their-Original-URL-in-Arc-for-Desktop)]. Closing a pinned tab unloads it: the row stays, dimmed [inferred]. Zen does the same, with a reset button; holding Ctrl changes how the reset works [3rd: [DeepWiki Zen pinned](https://deepwiki.com/zen-browser/desktop/3.3.1-pinned-tabs-and-essential-tabs)].
- **Folders.** They exist only in the pinned section, can nest, and collapse. Tabs from either section can be dragged into a folder [3rd: [allthings.how](https://allthings.how/?p=8915)].
- **Drag and drop.** You can reorder within a section, drag across the divider to pin or unpin (Ctrl+D also pins), drag onto the Favorites grid, and drag onto a Space icon to move the tab [3rd + inferred].

**Our plan:** pinned tabs store `pinned_url`. Closing one unloads it and resets it. The "/" drift marker and favicon-click reset are in. Folders come later.

## 4. Command bar

- **Arc:** Ctrl+T on Windows opens a centered overlay instead of a blank tab [official: [Arc Shortcuts](https://resources.arc.net/hc/en-us/articles/20595231349911-Keyboard-Shortcuts)]. It searches open tabs ("Switch to tab"), history, pinned tabs and Favorites, Spaces, and **commands** such as "Add Right Split", "View Archive" and "Theme". When nothing matches, it falls back to the search engine [official: [Arc Split View](https://resources.arc.net/hc/en-us/articles/19335393146775-Split-View-View-Multiple-Tabs-at-Once); 3rd: [Raycast Arc extension](https://www.raycast.com/the-browser-company/arc)]. Ctrl+L also opens it on Windows [3rd: [Windows Report](https://windowsreport.com/arc-browser-commands)], with the current URL pre-filled and selected [inferred].
- **Zen:** the URL bar works as the command bar. From 1.16 it can switch workspaces, open extensions, and run actions such as toggling compact mode. A floating URL bar replaces the new-tab page (`zen.urlbar.replace-newtab`) [official: [Zen URL bar](https://docs.zen-browser.app/user-manual/urlbar); 3rd: [AlternativeTo 1.16b](https://alternativeto.net/news/2025/9/zen-browser-1-16b-brings-enhanced-url-bar-command-bar-actions-and-updated-firefox-143-base)].
- **Keyboard flow** [inferred]: type → results update live → ↑/↓ move through them → Enter runs the selection (Alt+Enter opens in a new tab) → Esc closes. Results are grouped (Tabs, Commands, History, Search). The top hit is preselected.

**Our plan:** the groups are Tabs → Pinned/Essentials → Commands → History → "Search the web for …", which is always last. Tab moves to the next group. The bar is a borderless topmost GPUI window (see ARCHITECTURE.md, Phase 5).

## 5. Compact / hidden sidebar

- **Arc:** Ctrl+S toggles the sidebar [official: [Arc Hide Sidebar](https://resources.arc.net/hc/en-us/articles/25619487530519-How-Do-You-Hide-the-Sidebar), [Find focus](https://start.arc.net/find-focus)]. Hovering the left edge reveals it as a floating panel over the page, with rounded corners and a shadow [3rd: [Orion feedback](https://orionfeedback.org/d/684-sidebar-openclose-on-hover/83); inferred].
- **Zen:** Alt+Ctrl+C or the toolbar context menu. In single-toolbar mode it hides the sidebar, which comes back when you hover the side edge [official: [Zen Compact Mode](https://docs.zen-browser.app/user-manual/compact-mode)]. Its flags [official: [Zen about:config](https://docs.zen-browser.app/guides/about-config-flags)]:
  - `zen.view.compact.show-sidebar-and-toolbar-on-hover` (on)
  - `zen.view.compact.toolbar-hide-after-hover.duration` = **1000ms** hide delay
  - `zen.view.compact.animate-sidebar` (on): the page animates in and out
  - `zen.view.compact.toolbar-flash-popup`: briefly shows the sidebar when a new tab opens (800ms)
  - `zen.view.compact.hover-hack-delay` = 0, an anti-flicker delay [3rd: [DeepWiki UI modes](https://deepwiki.com/zen-browser/desktop/3.6-ui-modes-and-layouts)]
- **Hit area** [inferred]: a strip about 4–8px wide at the window's left edge. The panel slides in from the left over about 150–200ms with ease-out.

**Our plan:** Ctrl+S toggles (configurable). The hot zone is 6px. Show delay 80ms, hide delay 400ms. Zen's 1000ms feels slow, so we will make it configurable. Slide-in takes 180ms. The overlay strategy will be prototyped in Phase 6.

## 6. Split view

- **Arc:** up to **4 tabs**, side by side or stacked [3rd: [TechCrunch](https://techcrunch.com/2023/10/27/startup-browser-maker-arc-now-lets-you-share-spaces-folders-and-split-views-with-non-arc-users/), [Zapier](https://www.zapier.com/blog/arc-browser-review)]. You create one from the command bar ("Add Right/Left/Top/Bottom Split"), with Ctrl+Shift+= to add and Ctrl+Shift+- to remove, by dragging a tab onto the page, or by selecting several tabs and using the context menu [official: [Arc Split View](https://resources.arc.net/hc/en-us/articles/19335393146775-Split-View-View-Multiple-Tabs-at-Once)]. Dividers can be dragged on macOS; one guide says Windows could not resize them at one point [3rd: [allthings.how](https://allthings.how/how-to-open-tabs-in-split-view-in-arc-browser-on-windows)]. A split shows as **one grouped row** in the sidebar.
- **Zen:** up to **4 tabs**. The layouts are horizontal (Alt+Ctrl+H), vertical (Alt+Ctrl+V) and grid (Alt+Ctrl+G). Alt+Ctrl+U unsplits. The active pane gets a top overlay with a drag handle and an unsplit button [official: [Zen Split View](https://docs.zen-browser.app/user-manual/split-view)].
- **Visuals** [inferred]: each pane is its own rounded frame, with an 8px gap acting as the divider. The focused pane gets an accent outline.

**Our plan:** the MVP supports 2 panes, horizontal or vertical, with a draggable gap divider. A grid of 4 comes later.

## 7. Peek / Glance

- **Arc Peek:** Shift+click a link, or click an external link from a pinned tab or Favorite, and it opens in a floating card over the current page. The expand button (or Ctrl+O) promotes it to a tab, and there is also a Split button. Clicking outside, ×, or Ctrl+W closes it [official: [Arc Peek](https://resources.arc.net/hc/en-us/articles/19335302900887-Peek-Preview-Sites-From-Pinned-Tabs)].
- **Zen Glance:** Alt+click by default (can be changed to Ctrl+click). External links from Essentials and pinned tabs open in Glance automatically. Buttons: close, expand to a new tab, split [official: [Zen Glance](https://docs.zen-browser.app/user-manual/glance)].
- **Visuals** [inferred]: a card about 80% of the content area's size, centered, with a large radius and a deep shadow. The page behind it is dimmed. The card scales up from the clicked point over about 200ms.

**Our plan:** Shift+click (configurable) opens it. A popup window holds a webview with a slim header (domain, ×, expand, split). Esc or a click outside closes it.

## 8. Visual language

There are no official specs. Everything here is **[inferred]** from screenshots, videos and the sources cited in §1–7, then normalized into our tokens.

- **Layout.** The sidebar sits on a tinted "canvas" that fills the window. The web page sits on top in an **inset rounded frame**: about 8px gap on the top, right and bottom, with the sidebar as the gap on the left. The frame has about a 10px corner radius and a subtle 1px border/shadow. The canvas color *is* the Space theme, so switching Spaces changes the color around the page.
- **Spacing scale (ours):** 2, 4, 6, 8, 12, 16, 20, 24, 32px. Tab rows are 32px high with 8px horizontal padding and a 6px gap between favicon and title. Favorites tiles are about 52×44px.
- **Radius (ours):** 4 for small chips, 8 for tab rows and tiles, 10 for the content frame, 12 for popovers and the command bar, 14 for Peek cards.
- **Shadows:** the content frame has almost none (just a 1px border). Floating surfaces (command bar, Peek, compact sidebar) have a soft, large shadow (0 12px 32px, about 40% black in dark mode).
- **Typography:** the system UI font (Segoe UI Variable on Windows). Tab titles 13px regular. Section labels 11px, medium weight, muted. Command bar input 16px. There are no bold headings in the chrome.
- **Theme picker:**
  - Arc has a 2D color field or wheel, with 1–3 draggable color stops forming a gradient, a grain/noise slider, and a light/dark/auto toggle; the (−) button removes stops [official: [Arc default theme](https://resources.arc.net/hc/en-us/articles/25625261733143-How-Do-I-Restore-the-Default-Theme-for-Arc-for-Desktop); 3rd: [Inverse design interview](https://www.inverse.com/input/design/the-browser-company-arc-design-interview)].
  - Zen has a similar picker with gradient stops, noise and opacity [official: [Welcome to Zen](https://docs.zen-browser.app/welcome-to-zen)].
- **Motion:** quick and springy, never slow.
  - Hover feedback: 80–120ms
  - Space slide: about 200ms
  - Popovers: about 150ms (scale 0.98 → 1 plus fade)
  - Sidebar reveal: about 180ms ease-out
  - Peek open: about 220ms

### Our dark palette (source of truth for the Phase 1 theme struct)

| Token | Value | Use |
|-------|-------|-----|
| `canvas` | `#17171C` | Window background behind sidebar and frame |
| `sidebar_tint` | accent at 12% over `canvas` | Space-tinted sidebar background |
| `surface` | `#1F1F26` | Content frame backing, cards |
| `surface_raised` | `#272730` | Command bar, menus, Peek header |
| `surface_hover` | `#FFFFFF0F` | Row hover (white at 6%) |
| `surface_active` | `#FFFFFF1F` | Selected tab row (white at 12%) |
| `border` | `#FFFFFF14` | 1px hairlines, frame border |
| `text` | `#ECECF1` | Primary text |
| `text_muted` | `#9A9AA6` | Section labels, URL pill |
| `text_faint` | `#6B6B76` | Unloaded pinned tabs, placeholders |
| `accent` | `#7C8CFF` (default space) | Focus rings, active pane outline, progress |
| `danger` | `#FF6B6B` | Close-button hover, dangerous downloads |
| `warning` | `#F5B84B` | Insecure page indicator |
| `success` | `#4CC38A` | Download complete |
| `shadow` | `#00000066` | Floating surface shadow |

Default space accents to choose from: `#7C8CFF` indigo, `#4CC3B5` teal, `#F28C6B` coral, `#C58CFF` violet, `#6BBF59` green, `#E8B04B` amber.

## 9. What we will build

**MVP (Phases 1–9)**
1. Frameless window with our own titlebar, sidebar and inset content frame
2. One WebView2 per tab, with show/hide and lazy restore
3. URL pill with search fallback, back/forward/reload, loading state, title and favicon
4. Vertical tabs: pinned and unpinned, drag reorder, drag to pin, close on hover and middle-click
5. Pinned URL reset, "/" drift marker, close unloads
6. Spaces with accent color, a bottom switcher and Ctrl+Alt+1..9
7. Essentials grid (global)
8. Auto-archive (12h by default) and an Archived list
9. Command bar: tabs, history, pinned, commands, web search
10. Compact mode with an edge hover reveal
11. Split view with 2 panes
12. Peek on Shift+click, with promote to tab
13. JSON session plus SQLite history, and a settings page

**Later**
- Folders in the pinned section (nested, collapsible)
- Split with 3–4 panes and a grid layout
- Gradient themes with 2–3 stops plus grain, and a full theme picker
- Swipe gestures to switch spaces
- Auto-Peek for external links from pinned tabs and essentials
- Per-space profiles (separate cookie jars)
- Downloads panel and per-tab context menu (Phase 10)

**Out of scope**
- AI features (Arc Max, Dia-style assistants)
- Boosts / page CSS injection, Easels, Notes, Little Arc
- Sync, sharing Spaces, mobile companion apps
- Extensions (see "Later" in the brief, ask first)
- macOS / Linux

---

## All sources

**Arc (official)**
- https://resources.arc.net/hc/en-us/articles/19230755904151 (Favorites)
- https://resources.arc.net/hc/en-us/articles/19228855311127 (Archive)
- https://resources.arc.net/hc/en-us/articles/25625148480279-Why-is-There-a-Slash-Next-to-My-Pinned-Tab-s-Name
- https://resources.arc.net/hc/en-us/articles/25541939922199-Why-Are-My-Pinned-Tabs-and-Favorites-Reverting-Back-to-Their-Original-URL-in-Arc-for-Desktop
- https://resources.arc.net/hc/en-us/articles/20595231349911-Keyboard-Shortcuts
- https://resources.arc.net/hc/en-us/articles/19335393146775-Split-View-View-Multiple-Tabs-at-Once
- https://resources.arc.net/hc/en-us/articles/19335302900887-Peek-Preview-Sites-From-Pinned-Tabs
- https://resources.arc.net/hc/en-us/articles/25619487530519-How-Do-You-Hide-the-Sidebar
- https://resources.arc.net/hc/en-us/articles/25625261733143-How-Do-I-Restore-the-Default-Theme-for-Arc-for-Desktop
- https://resources.arc.net/hc/en-us/articles/20498417809815 (Release notes 2022)
- https://resources.arc.net/hc/en-us/articles/20498377604887-Arc-for-macOS-2023-Release-Notes
- https://resources.arc.net/hc/en-us/articles/20498463803799 (Release notes Dec 2021)
- https://start.arc.net/find-focus
- https://start.arc.net/master-multitasking

**Zen (official)**
- https://docs.zen-browser.app/user-manual/workspaces
- https://docs.zen-browser.app/user-manual/compact-mode
- https://docs.zen-browser.app/user-manual/split-view
- https://docs.zen-browser.app/user-manual/glance
- https://docs.zen-browser.app/user-manual/shortcuts
- https://docs.zen-browser.app/user-manual/urlbar
- https://docs.zen-browser.app/guides/about-config-flags
- https://docs.zen-browser.app/welcome-to-zen

**Third party**
- https://tidbits.com/2023/05/01/arc-will-change-the-way-you-work-on-the-web/
- https://allthings.how/how-do-pinned-tabs-work-in-arc-browser/
- https://allthings.how/?p=8915 (folders)
- https://allthings.how/how-to-manage-spaces-in-arc-browser-on-windows
- https://allthings.how/how-to-open-tabs-in-split-view-in-arc-browser-on-windows
- https://www.hongkiat.com/blog/arc-browser-keyboard-shortcuts/
- https://windowsreport.com/arc-browser-commands
- https://www.zapier.com/blog/arc-browser-review
- https://techcrunch.com/2023/10/27/startup-browser-maker-arc-now-lets-you-share-spaces-folders-and-split-views-with-non-arc-users/
- https://techcrunch.com/2025/11/03/dias-ai-browser-starts-adding-arcs-greatest-hits-to-its-feature-set
- https://alternativeto.net/news/2026/10/the-browser-company-confirms-arc-browser-sunset-directs-users-to-ai-powered-successor-dia/
- https://alternativeto.net/news/2025/9/zen-browser-1-16b-brings-enhanced-url-bar-command-bar-actions-and-updated-firefox-143-base
- https://www.theregister.com/2025/05/27/arc_browser_development_ends/
- https://www.raycast.com/the-browser-company/arc
- https://www.inverse.com/input/design/the-browser-company-arc-design-interview
- https://orionfeedback.org/d/684-sidebar-openclose-on-hover/83
- https://github.com/zen-browser/desktop/discussions/8940
- https://deepwiki.com/zen-browser/desktop/3.1-workspaces-system
- https://deepwiki.com/zen-browser/desktop/3.3.1-pinned-tabs-and-essential-tabs
- https://deepwiki.com/zen-browser/desktop/3.6-ui-modes-and-layouts
