//! 以纯 Rust 生成登录二维码。SVG 只包含编码后的几何坐标，不插入 URL 文本，
//! 避免渲染进程静态加载 qrcode CommonJS 模块时写入继承的 toString 属性。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use qrcodegen::{QrCode, QrCodeEcc};
use std::fmt::Write;

const MAX_LOGIN_URL_BYTES: usize = 1024;

pub(super) fn login_qr_data_url(url: &str) -> Result<String, String> {
    if url.is_empty() || url.len() > MAX_LOGIN_URL_BYTES {
        return Err("登录二维码地址为空或过长".into());
    }
    let qr = QrCode::encode_text(url, QrCodeEcc::Medium)
        .map_err(|_| "无法编码登录二维码".to_string())?;
    let border = 4;
    let size = qr.size() + border * 2;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><path fill=\"#0f172a\" d=\""
    );
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                let _ = write!(svg, "M{},{}h1v1h-1z", x + border, y + border);
            }
        }
    }
    svg.push_str("\"/></svg>");
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        STANDARD.encode(svg)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_qr_is_bounded_deterministic_geometry_without_raw_input() {
        let url = "https://passport.bilibili.com/login?secret=sample&value=<tag>";
        let data = login_qr_data_url(url).unwrap();
        assert_eq!(data, login_qr_data_url(url).unwrap());
        let svg =
            String::from_utf8(STANDARD.decode(data.split_once(',').unwrap().1).unwrap()).unwrap();
        assert!(svg.starts_with("<svg ") && svg.ends_with("</svg>"));
        assert!(!svg.contains("secret") && !svg.contains("<tag>"));
        assert!(data.len() < 64 * 1024);
        assert!(login_qr_data_url("").is_err());
        assert!(login_qr_data_url(&"a".repeat(MAX_LOGIN_URL_BYTES + 1)).is_err());
    }
}
