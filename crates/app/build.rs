use std::fs;

const NAME_SOURCE: &str = "../browser-core/src/lib.rs";
const NAME_PREFIX: &str = "pub const APP_NAME: &str = \"";

fn app_name() -> String {
    let source = fs::read_to_string(NAME_SOURCE).expect("browser-core lib.rs is readable");
    source
        .lines()
        .find_map(|line| line.trim().strip_prefix(NAME_PREFIX))
        .and_then(|rest| rest.split('"').next())
        .expect("APP_NAME is defined in browser-core")
        .to_string()
}

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed={NAME_SOURCE}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let name = app_name();
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets the version");
    let mut numbers: Vec<String> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse::<u16>().unwrap_or(0).to_string())
        .collect();
    numbers.resize(4, "0".to_string());
    let macros = [
        format!("APP_NAME=\"{name}\""),
        format!("APP_EXE=\"{name}.exe\""),
        format!("VERSION_TEXT=\"{version}\""),
        format!("VERSION_NUMBERS={}", numbers.join(",")),
    ];
    embed_resource::compile("app.rc", &macros)
        .manifest_optional()
        .expect("app resources compile");
}
