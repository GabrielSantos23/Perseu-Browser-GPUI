use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_DOWNLOAD_PROCESS_CRASHED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_ACCESS_DENIED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_BLOCKED_BY_POLICY,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_FAILED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_HASH_MISMATCH,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_MALICIOUS,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NAME_TOO_LONG,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_SECURITY_CHECK_FAILED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_LARGE,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_SHORT,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TRANSIENT_ERROR,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_DISCONNECTED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_INVALID_REQUEST,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_SERVER_DOWN,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_TIMEOUT,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_BAD_CONTENT,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CERTIFICATE_PROBLEM,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CONTENT_LENGTH_MISMATCH,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CROSS_ORIGIN_REDIRECT,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FAILED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FORBIDDEN,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_NO_RANGE,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNAUTHORIZED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNEXPECTED_RESPONSE,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_SHUTDOWN, COREWEBVIEW2_DOWNLOAD_STATE,
    COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED, COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS, ICoreWebView2,
    ICoreWebView2_4, ICoreWebView2DownloadOperation,
};
use webview2_com::{
    BytesReceivedChangedEventHandler, DownloadStartingEventHandler, StateChangedEventHandler,
    take_pwstr,
};
use windows::core::{Interface, PWSTR};

use crate::{DownloadOutcome, EventSink, WebViewEvent};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

static NEXT_DOWNLOAD_ID: AtomicU64 = AtomicU64::new(1);

struct ActiveDownload {
    operation: ICoreWebView2DownloadOperation,
    last_progress: Instant,
}

thread_local! {
    static ACTIVE: RefCell<HashMap<u64, ActiveDownload>> = RefCell::new(HashMap::new());
}

pub(crate) fn subscribe(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let Ok(webview) = webview.cast::<ICoreWebView2_4>() else {
        tracing::warn!("WebView2 runtime has no download API; its own download UI stays on");
        return Ok(());
    };
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        unsafe { args.SetHandled(true)? };
        let operation = unsafe { args.DownloadOperation()? };
        let id = NEXT_DOWNLOAD_ID.fetch_add(1, Ordering::Relaxed);
        let url = read_string(|value| unsafe { operation.Uri(value) })?;
        let path = read_string(|value| unsafe { operation.ResultFilePath(value) })?;
        let total = total_bytes(&operation)?;
        watch(&operation, id, sink.clone())?;
        ACTIVE.with(|active| {
            active.borrow_mut().insert(
                id,
                ActiveDownload {
                    operation,
                    last_progress: Instant::now(),
                },
            )
        });
        tracing::info!("download {id} started: {url} -> {path}");
        sink(WebViewEvent::DownloadStarted {
            id,
            url,
            path: PathBuf::from(path),
            total,
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_DownloadStarting(&handler, &mut token) }
}

/// Cancels a download started by any view. Unknown or finished ids are ignored.
pub fn cancel_download(id: u64) {
    let operation = ACTIVE.with(|active| {
        active
            .borrow()
            .get(&id)
            .map(|download| download.operation.clone())
    });
    if let Some(operation) = operation
        && let Err(error) = unsafe { operation.Cancel() }
    {
        tracing::warn!("failed to cancel download: {error}");
    }
}

fn watch(
    operation: &ICoreWebView2DownloadOperation,
    id: u64,
    sink: EventSink,
) -> windows::core::Result<()> {
    let progress_sink = sink.clone();
    let progress = BytesReceivedChangedEventHandler::create(Box::new(move |sender, _| {
        let Some(operation) = sender else {
            return Ok(());
        };
        let due = ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let Some(download) = active.get_mut(&id) else {
                return false;
            };
            let due = download.last_progress.elapsed() >= PROGRESS_INTERVAL;
            if due {
                download.last_progress = Instant::now();
            }
            due
        });
        if due {
            let mut received = 0i64;
            unsafe { operation.BytesReceived(&mut received)? };
            progress_sink(WebViewEvent::DownloadProgress {
                id,
                received: received.max(0) as u64,
                total: total_bytes(&operation)?,
            });
        }
        Ok(())
    }));
    let state = StateChangedEventHandler::create(Box::new(move |sender, _| {
        let Some(operation) = sender else {
            return Ok(());
        };
        let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
        unsafe { operation.State(&mut state)? };
        if state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
            return Ok(());
        }
        let outcome = if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
            tracing::info!("download {id} completed");
            DownloadOutcome::Completed
        } else {
            let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
            unsafe { operation.InterruptReason(&mut reason)? };
            if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED {
                tracing::info!("download {id} cancelled");
                DownloadOutcome::Cancelled
            } else {
                let why = interrupt_reason(reason);
                tracing::warn!("download {id} interrupted: {why}");
                DownloadOutcome::Failed(why)
            }
        };
        ACTIVE.with(|active| active.borrow_mut().remove(&id));
        sink(WebViewEvent::DownloadFinished { id, outcome });
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        operation.add_BytesReceivedChanged(&progress, &mut token)?;
        operation.add_StateChanged(&state, &mut token)
    }
}

fn total_bytes(operation: &ICoreWebView2DownloadOperation) -> windows::core::Result<Option<u64>> {
    let mut total = 0i64;
    unsafe { operation.TotalBytesToReceive(&mut total)? };
    Ok((total > 0).then_some(total as u64))
}

fn read_string(
    read: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<String> {
    let mut value = PWSTR::null();
    read(&mut value)?;
    Ok(take_pwstr(value))
}

/// The runtime's word for why a download stopped, for the logs and the
/// downloads panel — WebView2 gives no text of its own.
fn interrupt_reason(reason: COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON) -> &'static str {
    match reason {
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_ACCESS_DENIED => "access denied",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_BLOCKED_BY_POLICY => "blocked by policy",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_FAILED => "file failed",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_HASH_MISMATCH => "checksum mismatch",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_MALICIOUS => "flagged as dangerous",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NAME_TOO_LONG => "file name too long",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE => "no disk space",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_SECURITY_CHECK_FAILED => {
            "security check failed"
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_LARGE => "file too large",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TOO_SHORT => "file truncated",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_TRANSIENT_ERROR => "temporary file error",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_DISCONNECTED => "network disconnected",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED => "network failed",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_INVALID_REQUEST => "invalid request",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_SERVER_DOWN => "server down",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_TIMEOUT => "network timeout",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE => "unknown reason",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_BAD_CONTENT => "server has no such file",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CERTIFICATE_PROBLEM => "certificate problem",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CONTENT_LENGTH_MISMATCH => {
            "content length mismatch"
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_CROSS_ORIGIN_REDIRECT => {
            "cross-origin redirect"
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FAILED => "server error",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_FORBIDDEN => "server refused",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_NO_RANGE => "server does not resume",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNAUTHORIZED => "server wants a login",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_SERVER_UNEXPECTED_RESPONSE => {
            "unexpected server response"
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED => "paused",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_SHUTDOWN => "app closed",
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_DOWNLOAD_PROCESS_CRASHED => {
            "network process crashed"
        }
        _ => "unknown reason",
    }
}
