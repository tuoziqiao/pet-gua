//! 读取统一的品牌配置，并把品牌名称和图标编进 Windows 可执行文件。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn brand() -> (String, String, String) {
    let text = fs::read_to_string("branding.json").expect("无法读取 branding.json");
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .expect("branding.json 不是有效的 JSON");
    let display = value
        .get("display_name")
        .and_then(|v| v.as_str())
        .expect("branding.json 缺少字符串 display_name");
    let package = value
        .get("package_name")
        .and_then(|v| v.as_str())
        .expect("branding.json 缺少字符串 package_name");
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .expect("branding.json 缺少字符串 version");
    assert!(
        !display.trim().is_empty()
            && !display.chars().any(char::is_control)
            && display.encode_utf16().count() <= 80
            && !display.contains("${")
            && !display.contains("$%"),
        "display_name 必须非空、不能包含控制字符或 ${{、$% 模板符号，且不能超过 80 个 UTF-16 字符"
    );
    let mut chars = package.bytes();
    let valid = package.len() <= 40
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'));
    let upper = package.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "UNINSTALL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    assert!(valid && !reserved,
        "package_name 须以字母或数字开头，只能包含英文字母、数字、下划线、连字符，最多 40 个字符且不能是 Windows 保留名称或 uninstall");
    assert!(
        regex_like_version(version),
        "version 须匹配 semver 基础格式 x.y.z（三段十进制整数，每段 ≤ 65535）"
    );
    (display.to_owned(), package.to_owned(), version.to_owned())
}

fn regex_like_version(v: &str) -> bool {
    let mut splits = v.split('.');
    let Some(a) = splits.next() else { return false };
    let Some(b) = splits.next() else { return false };
    let Some(c) = splits.next() else { return false };
    splits.next().is_none()
        && a.parse::<u16>().is_ok()
        && b.parse::<u16>().is_ok()
        && c.parse::<u16>().is_ok()
}

fn rc_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\"\"")
}

fn render_resource(mut template: &str, values: &[(&str, &str)]) -> String {
    let mut result = String::new();
    loop {
        let next = values
            .iter()
            .filter_map(|(key, value)| template.find(key).map(|at| (at, *key, *value)))
            .min_by_key(|(at, _, _)| *at);
        let Some((at, key, value)) = next else {
            result.push_str(template);
            break;
        };
        result.push_str(&template[..at]);
        result.push_str(value);
        template = &template[at + key.len()..];
    }
    result
}

fn find_rc() -> Option<PathBuf> {
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let rc = dir.join("x64").join("rc.exe");
            if rc.exists() {
                found.push(rc);
            }
        }
    }
    found.sort();
    found.pop()
}

fn main() {
    println!("cargo:rerun-if-changed=branding.json");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/gua.ico");
    println!("cargo:rerun-if-changed=build.rs");
    let (display, package, version) = brand();
    println!("cargo:rustc-env=GUA_BRAND_DISPLAY_NAME={display}");
    println!("cargo:rustc-env=GUA_BRAND_PACKAGE_NAME={package}");
    println!("cargo:rustc-env=GUA_BRAND_VERSION={version}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out = out_dir.join("gua.res");
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe 未找到，跳过图标资源");
        return;
    };
    let numeric_parts: Vec<u16> = version
        .split('.')
        .map(|s| s.parse::<u16>().unwrap())
        .collect();
    let icon = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/gua.ico");
    let template = fs::read_to_string("app.rc").expect("无法读取 app.rc");
    let icon_path = rc_escape(&icon.to_string_lossy());
    let display_name = rc_escape(&display);
    let version_numeric = format!(
        "{},{},{},0",
        numeric_parts[0], numeric_parts[1], numeric_parts[2]
    );
    let resource = render_resource(
        &template,
        &[
            ("@ICON_PATH@", &icon_path),
            ("@DISPLAY_NAME@", &display_name),
            ("@PACKAGE_NAME@", &package),
            ("@VERSION_NUMERIC@", &version_numeric),
            ("@VERSION@", &version),
        ],
    );
    let generated = out_dir.join("app-brand.rc");
    fs::write(&generated, resource).expect("无法生成品牌资源文件");
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg(&generated)
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        panic!("rc.exe 编译品牌资源失败");
    }
    println!("cargo:rustc-link-arg={}", out.display());
}
