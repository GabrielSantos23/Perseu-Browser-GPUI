use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY,
    COREWEBVIEW2_PERMISSION_KIND_CAMERA, COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ,
    COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE, COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION,
    COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
    COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES,
    COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS,
    COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS, COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS,
    COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    COREWEBVIEW2_PERMISSION_STATE_DENY, ICoreWebView2, ICoreWebView2_18, ICoreWebView2Deferral,
    ICoreWebView2NewWindowRequestedEventArgs, ICoreWebView2PermissionRequestedEventArgs,
    ICoreWebView2PermissionRequestedEventArgs3,
};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, DevToolsProtocolEventReceivedEventHandler,
    LaunchingExternalUriSchemeEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, take_pwstr,
};
use windows::core::{BOOL, HSTRING, Interface, PWSTR, w};

use crate::keys::held_modifiers;
use crate::{EventSink, LinkModifier, PermissionKind, SecurityState, WebViewEvent};

const SECURITY_EVENT: &str = "Security.visibleSecurityStateChanged";

static NEXT_PERMISSION_ID: AtomicU64 = AtomicU64::new(1);

struct PendingPermission {
    args: ICoreWebView2PermissionRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
}

struct PendingWindow {
    args: ICoreWebView2NewWindowRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
}

thread_local! {
    static PENDING: RefCell<HashMap<u64, PendingPermission>> = RefCell::new(HashMap::new());
    static PENDING_WINDOWS: RefCell<HashMap<u64, PendingWindow>> = RefCell::new(HashMap::new());
}

/// Lets an extension's window request go without a window (its opener
/// gets none). Unknown ids are ignored.
pub fn decline_new_window(id: u64) {
    if let Some(pending) = PENDING_WINDOWS.with(|pending| pending.borrow_mut().remove(&id))
        && let Err(error) = unsafe { pending.deferral.Complete() }
    {
        tracing::warn!("failed to finish a new window request: {error}");
    }
}

pub(crate) fn adopt_new_window(id: u64, webview: &ICoreWebView2) {
    let Some(pending) = PENDING_WINDOWS.with(|pending| pending.borrow_mut().remove(&id)) else {
        return;
    };
    let result = unsafe {
        pending
            .args
            .SetNewWindow(webview)
            .and_then(|()| pending.deferral.Complete())
    };
    if let Err(error) = result {
        tracing::warn!("failed to hand a view to a new window: {error}");
        let _ = unsafe { pending.deferral.Complete() };
    }
}

pub(crate) fn subscribe(
    webview: &ICoreWebView2,
    sink: EventSink,
    peek_modifier: Option<LinkModifier>,
) -> windows::core::Result<()> {
    on_new_window(webview, sink.clone(), peek_modifier)?;
    on_permission(webview, sink.clone())?;
    on_external_scheme(webview, sink.clone())?;
    watch_security_state(webview, sink)
}

/// Grants or refuses a permission request reported as
/// [`WebViewEvent::PermissionRequested`]. Unknown ids are ignored.
pub fn answer_permission(id: u64, allow: bool) {
    let Some(pending) = PENDING.with(|pending| pending.borrow_mut().remove(&id)) else {
        return;
    };
    let state = if allow {
        COREWEBVIEW2_PERMISSION_STATE_ALLOW
    } else {
        COREWEBVIEW2_PERMISSION_STATE_DENY
    };
    let result = unsafe {
        pending
            .args
            .SetState(state)
            .and_then(|()| pending.deferral.Complete())
    };
    if let Err(error) = result {
        tracing::warn!("failed to answer permission request: {error}");
    }
}

fn on_new_window(
    webview: &ICoreWebView2,
    sink: EventSink,
    peek_modifier: Option<LinkModifier>,
) -> windows::core::Result<()> {
    let handler = NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut user_initiated = BOOL::default();
        let mut uri = PWSTR::null();
        let mut has_size = BOOL::default();
        let (mut width, mut height) = (0u32, 0u32);
        unsafe {
            args.IsUserInitiated(&mut user_initiated)?;
            args.Uri(&mut uri)?;
            args.SetHandled(true)?;
            let features = args.WindowFeatures()?;
            features.HasSize(&mut has_size)?;
            features.Width(&mut width)?;
            features.Height(&mut height)?;
        }
        let size = (has_size.as_bool() && width > 0 && height > 0)
            .then_some((f64::from(width), f64::from(height)));
        let url = take_pwstr(uri);
        // Extensions use the window they get back (Bitwarden reads its id),
        // so theirs wait for a real view instead of opening nothing.
        let request = if url.starts_with("chrome-extension:") {
            let deferral = unsafe { args.GetDeferral()? };
            let id = NEXT_PERMISSION_ID.fetch_add(1, Ordering::Relaxed);
            PENDING_WINDOWS.with(|pending| {
                pending.borrow_mut().insert(
                    id,
                    PendingWindow {
                        args: args.clone(),
                        deferral,
                    },
                )
            });
            Some(id)
        } else {
            None
        };
        let user_initiated = user_initiated.as_bool();
        let (ctrl, shift, alt) = held_modifiers();
        let peek = user_initiated
            && is_web_url(&url)
            && peek_modifier.is_some_and(|modifier| peek_chord(modifier, ctrl, shift, alt));
        sink(if peek {
            WebViewEvent::PeekRequested(url)
        } else {
            WebViewEvent::NewWindowRequested {
                url,
                user_initiated,
                size,
                request,
            }
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NewWindowRequested(&handler, &mut token) }
}

fn on_permission(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let handler = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
        let mut uri = PWSTR::null();
        unsafe {
            args.PermissionKind(&mut kind)?;
            args.Uri(&mut uri)?;
            if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                args3.SetSavesInProfile(false)?;
            }
        }
        let deferral = unsafe { args.GetDeferral()? };
        let id = NEXT_PERMISSION_ID.fetch_add(1, Ordering::Relaxed);
        PENDING.with(|pending| {
            pending
                .borrow_mut()
                .insert(id, PendingPermission { args, deferral })
        });
        sink(WebViewEvent::PermissionRequested {
            id,
            url: take_pwstr(uri),
            kind: permission_kind(kind),
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_PermissionRequested(&handler, &mut token) }
}

fn on_external_scheme(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let Ok(webview) = webview.cast::<ICoreWebView2_18>() else {
        tracing::warn!(
            "WebView2 runtime cannot intercept external protocols; it keeps its own prompt"
        );
        return Ok(());
    };
    let handler = LaunchingExternalUriSchemeEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut uri = PWSTR::null();
        let mut origin = PWSTR::null();
        let mut user_initiated = BOOL::default();
        unsafe {
            args.SetCancel(true)?;
            args.Uri(&mut uri)?;
            args.InitiatingOrigin(&mut origin)?;
            args.IsUserInitiated(&mut user_initiated)?;
        }
        sink(WebViewEvent::ExternalSchemeRequested {
            uri: take_pwstr(uri),
            origin: take_pwstr(origin),
            user_initiated: user_initiated.as_bool(),
        });
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_LaunchingExternalUriScheme(&handler, &mut token) }
}

fn watch_security_state(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let receiver =
        unsafe { webview.GetDevToolsProtocolEventReceiver(&HSTRING::from(SECURITY_EVENT))? };
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut json = PWSTR::null();
        unsafe { args.ParameterObjectAsJson(&mut json)? };
        if let Some(state) = security_state(&take_pwstr(json)) {
            sink(WebViewEvent::SecurityChanged(state));
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        receiver.add_DevToolsProtocolEventReceived(&handler, &mut token)?;
        webview.CallDevToolsProtocolMethod(
            w!("Security.enable"),
            w!("{}"),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|result, _| {
                if let Err(error) = result {
                    tracing::warn!("security state unavailable: {error}");
                }
                Ok(())
            })),
        )
    }
}

fn is_web_url(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn peek_chord(modifier: LinkModifier, ctrl: bool, shift: bool, alt: bool) -> bool {
    match modifier {
        LinkModifier::Shift => shift && !ctrl && !alt,
        LinkModifier::Ctrl => ctrl && !shift && !alt,
    }
}

fn security_state(json: &str) -> Option<SecurityState> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let state = value
        .get("visibleSecurityState")?
        .get("securityState")?
        .as_str()?;
    Some(match state {
        "secure" => SecurityState::Secure,
        "insecure" | "insecure-broken" | "dangerous" => SecurityState::Insecure,
        _ => SecurityState::Neutral,
    })
}

fn permission_kind(kind: COREWEBVIEW2_PERMISSION_KIND) -> PermissionKind {
    match kind {
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => PermissionKind::Microphone,
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => PermissionKind::Camera,
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => PermissionKind::Geolocation,
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => PermissionKind::Notifications,
        COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS => PermissionKind::Sensors,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => PermissionKind::ClipboardRead,
        COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS => {
            PermissionKind::AutomaticDownloads
        }
        COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE => PermissionKind::FileSystem,
        COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY => PermissionKind::Autoplay,
        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS => PermissionKind::LocalFonts,
        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES => PermissionKind::Midi,
        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT => PermissionKind::WindowManagement,
        _ => PermissionKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_needs_exactly_the_chosen_modifier() {
        assert!(peek_chord(LinkModifier::Shift, false, true, false));
        assert!(!peek_chord(LinkModifier::Shift, true, true, false));
        assert!(!peek_chord(LinkModifier::Shift, false, false, false));
        assert!(peek_chord(LinkModifier::Ctrl, true, false, false));
        assert!(!peek_chord(LinkModifier::Ctrl, true, false, true));
    }

    #[test]
    fn only_web_urls_can_be_peeked() {
        assert!(is_web_url("https://a.example"));
        assert!(is_web_url("HTTP://a"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("file:///C:/x"));
    }

    #[test]
    fn security_state_comes_from_the_devtools_event() {
        let secure =
            r#"{"visibleSecurityState":{"securityState":"secure","securityStateIssueIds":[]}}"#;
        let broken = r#"{"visibleSecurityState":{"securityState":"insecure-broken"}}"#;
        let neutral = r#"{"visibleSecurityState":{"securityState":"neutral"}}"#;
        assert_eq!(security_state(secure), Some(SecurityState::Secure));
        assert_eq!(security_state(broken), Some(SecurityState::Insecure));
        assert_eq!(security_state(neutral), Some(SecurityState::Neutral));
        assert_eq!(security_state("{}"), None);
        assert_eq!(security_state("not json"), None);
    }
}
