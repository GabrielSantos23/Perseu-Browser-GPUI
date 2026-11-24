use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, COREWEBVIEW2_KEY_EVENT_KIND,
    COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    ICoreWebView2, ICoreWebView2_8, ICoreWebView2_15, ICoreWebView2AcceleratorKeyPressedEventArgs,
    ICoreWebView2Controller,
};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, ContainsFullScreenElementChangedEventHandler,
    FaviconChangedEventHandler, FocusChangedEventHandler, GetFaviconCompletedHandler,
    HistoryChangedEventHandler, IsDocumentPlayingAudioChangedEventHandler,
    IsMutedChangedEventHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler,
    SourceChangedEventHandler, WindowCloseRequestedEventHandler, ZoomFactorChangedEventHandler,
    take_pwstr,
};
use windows::Win32::System::Com::IStream;
use windows::core::{BOOL, Interface, PWSTR};

use crate::keys::chord_for;
use crate::{EventSink, ShortcutFilter, WebViewEvent};

const STREAM_CHUNK: usize = 16 * 1024;

pub(crate) fn subscribe(
    controller: &ICoreWebView2Controller,
    webview: &ICoreWebView2,
    sink: EventSink,
    intercept: ShortcutFilter,
) -> windows::core::Result<()> {
    on_accelerator_key(controller, sink.clone(), intercept)?;
    on_got_focus(controller, sink.clone())?;
    on_zoom_changed(controller, sink.clone())?;
    on_navigation_starting(webview, sink.clone())?;
    on_navigation_completed(webview, sink.clone())?;
    on_source_changed(webview, sink.clone())?;
    on_history_changed(webview, sink.clone())?;
    crate::downloads::subscribe(webview, sink.clone())?;
    on_audio_changed(webview, sink.clone())?;
    on_close_requested(webview, sink.clone())?;
    on_fullscreen_changed(webview, sink.clone())?;
    on_favicon_changed(webview, sink)
}

fn on_close_requested(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let handler = WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
        sink(WebViewEvent::CloseRequested);
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_WindowCloseRequested(&handler, &mut token) }
}

fn on_fullscreen_changed(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let handler =
        ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
            let mut fullscreen = BOOL::default();
            if let Some(sender) = sender {
                unsafe { sender.ContainsFullScreenElement(&mut fullscreen)? };
            }
            sink(WebViewEvent::FullscreenChanged(fullscreen.as_bool()));
            Ok(())
        }));
    let mut token = 0;
    unsafe { webview.add_ContainsFullScreenElementChanged(&handler, &mut token) }
}

fn on_accelerator_key(
    controller: &ICoreWebView2Controller,
    sink: EventSink,
    intercept: ShortcutFilter,
) -> windows::core::Result<()> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let Some(chord) = pressed_chord(&args)? else {
            return Ok(());
        };
        if intercept(&chord) {
            unsafe { args.SetHandled(true)? };
            sink(WebViewEvent::Shortcut(chord));
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) }
}

fn pressed_chord(
    args: &ICoreWebView2AcceleratorKeyPressedEventArgs,
) -> windows::core::Result<Option<crate::KeyChord>> {
    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
    let mut virtual_key = 0u32;
    unsafe {
        args.KeyEventKind(&mut kind)?;
        args.VirtualKey(&mut virtual_key)?;
    }
    let is_press = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
        || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN;
    Ok(is_press.then(|| chord_for(virtual_key)).flatten())
}

fn on_got_focus(
    controller: &ICoreWebView2Controller,
    sink: EventSink,
) -> windows::core::Result<()> {
    let handler = FocusChangedEventHandler::create(Box::new(move |_, _| {
        sink(WebViewEvent::FocusGained);
        Ok(())
    }));
    let mut token = 0;
    unsafe { controller.add_GotFocus(&handler, &mut token) }
}

fn on_zoom_changed(
    controller: &ICoreWebView2Controller,
    sink: EventSink,
) -> windows::core::Result<()> {
    let handler = ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(controller) = sender {
            let mut factor = 1.0;
            unsafe { controller.ZoomFactor(&mut factor)? };
            sink(WebViewEvent::ZoomChanged(factor));
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { controller.add_ZoomFactorChanged(&handler, &mut token) }
}

fn on_navigation_starting(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let handler = NavigationStartingEventHandler::create(Box::new(move |_, _| {
        sink(WebViewEvent::LoadStarted);
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NavigationStarting(&handler, &mut token) }
}

fn on_navigation_completed(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_, _| {
        sink(WebViewEvent::LoadFinished);
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NavigationCompleted(&handler, &mut token) }
}

fn on_source_changed(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            sink(WebViewEvent::UrlChanged(committed_url(&sender)?));
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_SourceChanged(&handler, &mut token) }
}

fn on_history_changed(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let handler = HistoryChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            sink(history_state(&sender)?);
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_HistoryChanged(&handler, &mut token) }
}

fn on_audio_changed(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let Ok(webview) = webview.cast::<ICoreWebView2_8>() else {
        tracing::warn!("WebView2 runtime has no audio API");
        return Ok(());
    };
    let playing_sink = sink.clone();
    let playing = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            playing_sink(audio_state(&sender.cast()?)?);
        }
        Ok(())
    }));
    let muted = IsMutedChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            sink(audio_state(&sender.cast()?)?);
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        webview.add_IsDocumentPlayingAudioChanged(&playing, &mut token)?;
        webview.add_IsMutedChanged(&muted, &mut token)
    }
}

fn audio_state(webview: &ICoreWebView2_8) -> windows::core::Result<WebViewEvent> {
    let mut playing = BOOL::default();
    let mut muted = BOOL::default();
    unsafe {
        webview.IsDocumentPlayingAudio(&mut playing)?;
        webview.IsMuted(&mut muted)?;
    }
    Ok(WebViewEvent::AudioChanged {
        playing: playing.as_bool(),
        muted: muted.as_bool(),
    })
}

fn on_favicon_changed(webview: &ICoreWebView2, sink: EventSink) -> windows::core::Result<()> {
    let Ok(webview) = webview.cast::<ICoreWebView2_15>() else {
        tracing::warn!("WebView2 runtime has no favicon API");
        return Ok(());
    };
    let handler = FaviconChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            request_favicon(&sender.cast()?, sink.clone())?;
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_FaviconChanged(&handler, &mut token) }
}

fn request_favicon(webview: &ICoreWebView2_15, sink: EventSink) -> windows::core::Result<()> {
    let completed = GetFaviconCompletedHandler::create(Box::new(move |result, stream| {
        result?;
        let png = stream.map(|stream| read_stream(&stream)).transpose()?;
        sink(WebViewEvent::FaviconChanged(
            png.filter(|bytes| !bytes.is_empty()),
        ));
        Ok(())
    }));
    unsafe { webview.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &completed) }
}

fn committed_url(webview: &ICoreWebView2) -> windows::core::Result<String> {
    let mut uri = PWSTR::null();
    unsafe { webview.Source(&mut uri)? };
    Ok(take_pwstr(uri))
}

fn history_state(webview: &ICoreWebView2) -> windows::core::Result<WebViewEvent> {
    let mut can_go_back = BOOL::default();
    let mut can_go_forward = BOOL::default();
    unsafe {
        webview.CanGoBack(&mut can_go_back)?;
        webview.CanGoForward(&mut can_go_forward)?;
    }
    Ok(WebViewEvent::HistoryChanged {
        can_go_back: can_go_back.as_bool(),
        can_go_forward: can_go_forward.as_bool(),
    })
}

fn read_stream(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; STREAM_CHUNK];
    loop {
        let mut read = 0u32;
        unsafe {
            stream
                .Read(
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    Some(&mut read),
                )
                .ok()?;
        }
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
}
