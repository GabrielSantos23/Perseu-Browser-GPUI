//! "Identify": names the song this browser is playing, the way Shazam's own
//! extension does. It records the pages' audio, turns 3, 6, 9 and 12
//! seconds of it into a Shazam signature with the extension's `sigx.wasm`
//! (run in a Web Worker of a hidden page) and asks Shazam's web matcher.
//! The extension package is downloaded from the Chrome Web Store on first
//! use, not shipped with the app.

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gpui::{AsyncWindowContext, Context, WeakEntity, Window};
use raw_window_handle::HasWindowHandle;
use webview_host::{BrowserView, BrowserViewOptions, ShortcutFilter};

use super::Shell;
use crate::loopback;

/// The Shazam extension on the Chrome Web Store.
const SHAZAM_EXTENSION: &str = "mmioliijnhnoblpgimnlajmefafdfilb";
const MATCH_URL: &str = "https://www.shazam.com/services/webrec/match_extensionv2";
/// The installation id the extension sends before it has its own.
const ANONYMOUS_INID: &str = "0a0a0a0a-1b1b-2c2c-3d3d-4c4c4c4c4c4c";
/// What the extension records before each attempt, in seconds.
const ATTEMPTS_SECS: [u64; 4] = [3, 6, 9, 12];
/// Shazam signs 16 kHz audio; recording is 48 kHz, so every third sample.
const SIGN_RATE: u32 = 16_000;
const SIGN_PAGE: &str = include_str!("identify_page.html");
/// The signer page needs a real origin: on a page loaded from a string the
/// worker starts but never signs. The Shazam folder is served here.
const SIGNER_HOST: &str = "shazam-signer.perseu";
const SIGNER_PAGE_FILE: &str = "perseu-identify.html";
/// No step may leave the button on "Listening…" for good.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);
const PAGE_READY_POLL: Duration = Duration::from_millis(100);
const PAGE_READY_TRIES: usize = 50;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MATCH_TIMEOUT_SECS: &str = "20";
/// How long a found song stays on the card before the player comes back.
pub(super) const FOUND_HOLD: Duration = Duration::from_secs(20);
/// How long "No match" shows on the button.
pub(super) const MISSED_HOLD: Duration = Duration::from_secs(4);

/// A song Shazam recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Song {
    pub(super) title: String,
    pub(super) artist: String,
    /// Shazam's page for the track.
    pub(super) url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Identify {
    #[default]
    Idle,
    Listening,
    Found(Song),
    Missed,
}

impl Shell {
    /// Starts listening for the song that plays; does nothing while a try
    /// is already running.
    pub(super) fn identify_song(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.media.identify == Identify::Listening {
            return;
        }
        self.media.identify = Identify::Listening;
        cx.notify();
        let package = self.paths.shazam();
        let data_directory = self.paths.webview_data();
        let parent = window.window_handle().ok().map(|handle| handle.as_raw());
        let engine = self.media.signer.clone();
        cx.spawn_in(window, async move |shell, cx| {
            let found = listen(&shell, engine, parent, &package, &data_directory, cx).await;
            let outcome = match found {
                Ok(Some(song)) => Identify::Found(song),
                Ok(None) => Identify::Missed,
                Err(error) => {
                    tracing::warn!("could not identify the song: {error:#}");
                    Identify::Missed
                }
            };
            let hold = if matches!(outcome, Identify::Found(_)) {
                FOUND_HOLD
            } else {
                MISSED_HOLD
            };
            let shown = outcome.clone();
            let updated = shell.update(cx, |shell, cx| {
                shell.media.identify = outcome;
                cx.notify();
            });
            if updated.is_err() {
                return;
            }
            cx.background_executor().timer(hold).await;
            shell
                .update(cx, |shell, cx| {
                    // Only if nothing newer replaced it meanwhile.
                    if shell.media.identify == shown {
                        shell.media.identify = Identify::Idle;
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Puts the player back after a found song.
    pub(super) fn dismiss_song(&mut self, cx: &mut Context<Self>) {
        self.media.identify = Identify::Idle;
        cx.notify();
    }
}

async fn listen(
    shell: &WeakEntity<Shell>,
    engine: Option<Rc<BrowserView>>,
    parent: Option<raw_window_handle::RawWindowHandle>,
    package: &Path,
    data_directory: &Path,
    cx: &mut AsyncWindowContext,
) -> Result<Option<Song>> {
    let folder = package.to_path_buf();
    let (glue, wasm) = cx
        .background_executor()
        .spawn(async move { shazam_package(&folder) })
        .await?;
    let view = match engine {
        Some(view) => view,
        None => {
            let parent = parent.context("no window to host the signer")?;
            let view = Rc::new(signer_view(parent, data_directory, package)?);
            let load = format!(
                "perseuLoad({}, {})",
                serde_json::to_string(&glue)?,
                serde_json::to_string(&base64(&wasm))?
            );
            wait_for_page(&view, cx).await?;
            let loaded = evaluate(&view, &load, cx).await;
            anyhow::ensure!(loaded == "ready", "the signer did not start: {loaded}");
            shell.update(cx, |shell, _| shell.media.signer = Some(view.clone()))?;
            view
        }
    };

    let process = view
        .browser_process_id()
        .context("WebView2 did not name its process")?;
    let (snapshots, mut recorded) = mpsc::unbounded::<Vec<f32>>();
    let marks: Vec<Duration> = ATTEMPTS_SECS.map(Duration::from_secs).to_vec();
    std::thread::Builder::new()
        .name("identify-audio".into())
        .spawn(move || {
            let result = loopback::record(process, &marks, |samples| {
                snapshots.unbounded_send(samples.to_vec()).is_ok()
            });
            if let Err(error) = result {
                tracing::warn!("could not record the page audio: {error:#}");
            }
        })?;
    while let Some(samples) = recorded.next().await {
        let pcm = downsample(&samples);
        if pcm.iter().all(|sample| *sample == 0.0) {
            continue;
        }
        let started = Instant::now();
        let signature = evaluate(
            &view,
            &format!(
                "perseuSign({}, {SIGN_RATE})",
                serde_json::to_string(&base64(&f32_bytes(&pcm)))?
            ),
            cx,
        )
        .await;
        if signature.is_empty() {
            continue;
        }
        tracing::debug!("signed {} samples in {:?}", pcm.len(), started.elapsed());
        let reply = cx
            .background_executor()
            .spawn(async move { ask_shazam(&signature) })
            .await?;
        if let Some(song) = parse_match(&reply) {
            return Ok(Some(song));
        }
    }
    Ok(None)
}

/// The extension's signer glue and wasm, downloading the extension once.
fn shazam_package(folder: &Path) -> Result<(String, Vec<u8>)> {
    let wasm_path = folder.join("sigx.wasm");
    if !wasm_path.is_file() {
        let version = webview_host::runtime_version().context("WebView2 is missing")?;
        let url = browser_core::webstore::crx_url(SHAZAM_EXTENSION, &version);
        super::extensions::download_and_unpack(&url, folder)
            .context("downloading the Shazam extension")?;
    }
    let wasm = std::fs::read(&wasm_path).context("reading Shazam's signer")?;
    let glue = std::fs::read_dir(folder)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "js"))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .find(|text| text.contains("_onSignatureReadyImpl"))
        .context("Shazam's signer script is missing")?;
    Ok((glue, wasm))
}

/// A view that never shows: it only hosts the signing worker.
fn signer_view(
    parent: raw_window_handle::RawWindowHandle,
    data_directory: &Path,
    package: &Path,
) -> Result<BrowserView> {
    std::fs::write(package.join(SIGNER_PAGE_FILE), SIGN_PAGE).context("writing the signer page")?;
    let intercept: ShortcutFilter = Rc::new(|_| false);
    let view = BrowserView::new(
        parent,
        BrowserViewOptions {
            url: "",
            background: (0, 0, 0, 0),
            intercept,
            peek_modifier: None,
            data_directory: Some(data_directory),
            download_folder: None,
            extensions_enabled: true,
            reputation_checks: true,
            private: false,
        },
        Rc::new(|_| {}),
    )?;
    view.set_visible(false);
    view.serve_folder(SIGNER_HOST, package)?;
    view.navigate(&format!("https://{SIGNER_HOST}/{SIGNER_PAGE_FILE}"));
    Ok(view)
}

async fn wait_for_page(view: &BrowserView, cx: &mut AsyncWindowContext) -> Result<()> {
    for _ in 0..PAGE_READY_TRIES {
        if evaluate(view, "typeof perseuLoad", cx).await == "function" {
            return Ok(());
        }
        cx.background_executor().timer(PAGE_READY_POLL).await;
    }
    anyhow::bail!("the signer page did not load")
}

/// Runs `expression` in the view and waits for its string result; `""`
/// when it fails, is not a string or takes longer than [`STEP_TIMEOUT`].
async fn evaluate(view: &BrowserView, expression: &str, cx: &AsyncWindowContext) -> String {
    let (sender, receiver) = oneshot::channel();
    view.evaluate_string(expression, move |value| {
        let _ = sender.send(value);
    });
    let timeout = cx.background_executor().timer(STEP_TIMEOUT);
    match futures::future::select(receiver, std::pin::pin!(timeout)).await {
        futures::future::Either::Left((value, _)) => value.unwrap_or_default(),
        futures::future::Either::Right(_) => {
            tracing::warn!("the signer page did not answer in time");
            String::new()
        }
    }
}

fn ask_shazam(signature: &str) -> Result<String> {
    use std::io::Write as _;
    use std::os::windows::process::CommandExt;
    let body = serde_json::json!({
        "data": signature,
        "sessionId": session_id(),
        "inid": ANONYMOUS_INID,
        "lang": "en",
        "country": "US",
    })
    .to_string();
    let mut child = std::process::Command::new("curl.exe")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            MATCH_TIMEOUT_SECS,
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            MATCH_URL,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("could not start curl")?;
    child
        .stdin
        .take()
        .context("curl input")?
        .write_all(body.as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "Shazam did not answer: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A random UUID, as the extension makes one per request.
fn session_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(Instant::now().elapsed().as_nanos());
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The first match of a matcher reply.
fn parse_match(reply: &str) -> Option<Song> {
    let reply: serde_json::Value = serde_json::from_str(reply).ok()?;
    let attributes = reply.pointer("/results/matches/0/attributes")?;
    let title = attributes.get("title")?.as_str()?.to_string();
    let artist = attributes
        .get("primaryArtist")
        .and_then(|artist| artist.as_str())
        .unwrap_or_default()
        .to_string();
    let url = attributes
        .get("webUrl")
        .and_then(|url| url.as_str())
        .filter(|url| url.starts_with("https://"))
        .map(str::to_string);
    Some(Song { title, artist, url })
}

/// 48 kHz to 16 kHz, averaging each three samples.
fn downsample(samples: &[f32]) -> Vec<f32> {
    let step = (loopback::SAMPLE_RATE / SIGN_RATE) as usize;
    samples
        .chunks_exact(step)
        .map(|chunk| chunk.iter().sum::<f32>() / step as f32)
        .collect()
}

fn f32_bytes(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |acc, (index, byte)| {
            acc | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(TABLE[(triple >> (18 - 6 * index) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd, 0x00]), "//79AA==");
    }

    #[test]
    fn a_match_names_title_artist_and_page() {
        let reply = r#"{"results":{"matches":[{"attributes":{"title":"Hit 'Em Up","primaryArtist":"2Pac","webUrl":"https://www.shazam.com/song/1?referrer=x"}}]}}"#;
        assert_eq!(
            parse_match(reply),
            Some(Song {
                title: "Hit 'Em Up".into(),
                artist: "2Pac".into(),
                url: Some("https://www.shazam.com/song/1?referrer=x".into()),
            })
        );
        assert_eq!(
            parse_match(r#"{"results":{"matches":[],"retry":{}}}"#),
            None
        );
    }

    #[test]
    fn audio_is_brought_down_to_the_signing_rate() {
        assert_eq!(downsample(&[3.0, 0.0, 0.0, 6.0, 0.0, 0.0, 1.0]), [1.0, 2.0]);
    }

    #[test]
    fn session_ids_look_like_uuids() {
        let id = session_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
    }
}
