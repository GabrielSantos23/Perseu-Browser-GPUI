use crate::BROWSER_ARGS;

const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("lib.rs")),
    ("events.rs", include_str!("events.rs")),
    ("extensions.rs", include_str!("extensions.rs")),
    ("guard.rs", include_str!("guard.rs")),
    ("downloads.rs", include_str!("downloads.rs")),
    ("keys.rs", include_str!("keys.rs")),
    ("region.rs", include_str!("region.rs")),
];

fn forbidden(parts: &[&str]) -> String {
    parts.concat()
}

#[test]
fn web_pages_get_no_bridge_into_the_app() {
    let bridges = [
        forbidden(&["with_ipc", "_handler"]),
        forbidden(&["add_Web", "MessageReceived"]),
        forbidden(&["AddHost", "ObjectToScript"]),
        forbidden(&["with_initialization", "_script"]),
        forbidden(&["AddScriptToExecute", "OnDocumentCreated"]),
        forbidden(&["with_custom", "_protocol"]),
    ];
    for (file, source) in SOURCES {
        for bridge in &bridges {
            assert!(!source.contains(bridge.as_str()), "{file} uses {bridge}");
        }
    }
}

#[test]
fn certificate_errors_are_never_accepted_by_the_app() {
    let overrides = [
        forbidden(&["ServerCertificate", "ErrorDetected"]),
        forbidden(&["ALWAYS", "_ALLOW"]),
        forbidden(&["ignore-certificate", "-errors"]),
    ];
    for (file, source) in SOURCES {
        for item in &overrides {
            assert!(!source.contains(item.as_str()), "{file} uses {item}");
        }
    }
}

#[test]
fn browser_arguments_keep_smartscreen_and_gesture_gated_autoplay() {
    assert!(!BROWSER_ARGS.contains("SmartScreen"));
    assert!(!BROWSER_ARGS.contains("autoplay"));
    assert!(!BROWSER_ARGS.contains("remote-debugging"));
    assert!(!BROWSER_ARGS.contains("disable-web-security"));
}

#[test]
fn evergreen_runtime_only() {
    let fixed_runtime = forbidden(&["browser_executable", "_folder"]);
    for (file, source) in SOURCES {
        assert!(
            !source.contains(fixed_runtime.as_str()),
            "{file} points at a fixed runtime"
        );
    }
}
