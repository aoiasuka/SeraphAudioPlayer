fn bilibili_client_for_app(app: &AppHandle) -> Result<Client, String> {
    let cookie = load_session(app)?.and_then(|session| session.cookie_header());
    bilibili_client_with_cookie(cookie.as_deref())
}

fn bilibili_client_with_cookie(cookie: Option<&str>) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .no_gzip()
        .no_brotli()
        .no_zstd()
        .no_deflate()
        .default_headers(bilibili_headers(cookie)?)
        .build()
        .map_err(|err| format!("failed to create http client: {err}"))
}

fn bilibili_headers(cookie: Option<&str>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, header_value(USER_AGENT_VALUE)?);
    headers.insert(REFERER, header_value(BILIBILI_REFERER)?);
    headers.insert(ORIGIN, header_value(BILIBILI_REFERER)?);
    headers.insert(ACCEPT, header_value("*/*")?);
    headers.insert(ACCEPT_ENCODING, header_value("identity")?);
    headers.insert(
        ACCEPT_LANGUAGE,
        header_value("zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")?,
    );
    if let Some(cookie) = cookie.map(str::trim).filter(|value| !value.is_empty()) {
        headers.insert(COOKIE, header_value(cookie)?);
    }
    Ok(headers)
}

fn header_value(value: &str) -> Result<HeaderValue, String> {
    HeaderValue::from_str(value).map_err(|err| format!("invalid http header: {err}"))
}

fn session_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    Ok(dir.join("bilibili-session.json"))
}

fn load_session(app: &AppHandle) -> Result<Option<BilibiliSession>, String> {
    let path = session_path(app)?;
    if !path.is_file() {
        return Ok(None);
    }

    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read bilibili session {}: {err}", path.display()))?;
    let mut session: BilibiliSession = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse bilibili session {}: {err}", path.display()))?;
    if let Some(cookies) = load_secure_bilibili_cookies()? {
        session.cookies = cookies;
        session.has_secure_cookies = true;
    } else if !session.cookies.is_empty() {
        save_secure_bilibili_cookies(&session.cookies)?;
        session.has_secure_cookies = true;
        write_session_file(app, &session, false)?;
    }
    Ok(Some(session))
}

fn save_session(app: &AppHandle, session: &BilibiliSession) -> Result<(), String> {
    if !session.cookies.is_empty() {
        save_secure_bilibili_cookies(&session.cookies)?;
    }
    write_session_file(app, session, false)
}

fn write_session_file(
    app: &AppHandle,
    session: &BilibiliSession,
    include_cookies: bool,
) -> Result<(), String> {
    let path = session_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create bilibili session dir: {err}"))?;
    }
    let disk_session = BilibiliSessionFile {
        saved_at: session.saved_at,
        username: &session.username,
        mid: session.mid,
        face: &session.face,
        has_secure_cookies: session.has_secure_cookies || !session.cookies.is_empty(),
        cookies: include_cookies.then_some(&session.cookies),
        cookie_expires: &session.cookie_expires,
    };
    let bytes = serde_json::to_vec(&disk_session)
        .map_err(|err| format!("failed to serialize bilibili session: {err}"))?;
    fs::write(&path, bytes)
        .map_err(|err| format!("failed to write bilibili session {}: {err}", path.display()))?;
    restrict_session_file_permissions(&path)?;
    Ok(())
}

const BILIBILI_CREDENTIAL_TARGET: &str = "SeraphAudioPlayer/BilibiliSession";
const BILIBILI_CREDENTIAL_USER: &str = "bilibili";

fn load_secure_bilibili_cookies() -> Result<Option<BTreeMap<String, String>>, String> {
    let Some(bytes) = windows_read_credential(BILIBILI_CREDENTIAL_TARGET)? else {
        return Ok(None);
    };
    let cookies = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse secure bilibili cookies: {err}"))?;
    Ok(Some(cookies))
}

fn save_secure_bilibili_cookies(cookies: &BTreeMap<String, String>) -> Result<(), String> {
    if cookies.is_empty() {
        return delete_secure_bilibili_cookies();
    }
    let bytes = serde_json::to_vec(cookies)
        .map_err(|err| format!("failed to serialize secure bilibili cookies: {err}"))?;
    windows_write_credential(BILIBILI_CREDENTIAL_TARGET, BILIBILI_CREDENTIAL_USER, &bytes)
}

fn delete_secure_bilibili_cookies() -> Result<(), String> {
    windows_delete_credential(BILIBILI_CREDENTIAL_TARGET)
}

#[cfg(windows)]
fn windows_read_credential(target: &str) -> Result<Option<Vec<u8>>, String> {
    use std::{ptr, slice};
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC},
    };

    let target = wide_null(target);
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        if err == ERROR_NOT_FOUND {
            return Ok(None);
        }
        return Err(format!("failed to read bilibili credential: {err}"));
    }

    let bytes = unsafe {
        let credential_ref = &*credential;
        let blob = slice::from_raw_parts(
            credential_ref.CredentialBlob,
            credential_ref.CredentialBlobSize as usize,
        );
        let bytes = blob.to_vec();
        CredFree(credential.cast());
        bytes
    };
    Ok(Some(bytes))
}

#[cfg(not(windows))]
fn windows_read_credential(_target: &str) -> Result<Option<Vec<u8>>, String> {
    Ok(None)
}

#[cfg(windows)]
fn windows_write_credential(target: &str, user: &str, secret: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::GetLastError,
        Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    };

    let mut target = wide_null(target);
    let mut user = wide_null(user);
    let mut secret = secret.to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        UserName: user.as_mut_ptr(),
        CredentialBlobSize: secret.len() as u32,
        CredentialBlob: secret.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };

    let ok = unsafe { CredWriteW(&credential, 0) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        return Err(format!("failed to write bilibili credential: {err}"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn windows_write_credential(_target: &str, _user: &str, _secret: &[u8]) -> Result<(), String> {
    Err("secure credential storage is only implemented on Windows".into())
}

#[cfg(windows)]
fn windows_delete_credential(target: &str) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC},
    };

    let target = wide_null(target);
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        if err != ERROR_NOT_FOUND {
            return Err(format!("failed to delete bilibili credential: {err}"));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn windows_delete_credential(_target: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn restrict_session_file_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|err| {
            format!(
                "failed to restrict bilibili session permissions {}: {err}",
                path.display()
            )
        })?;
    }

    #[cfg(windows)]
    {
        // 用户名可能含空格/中文/特殊字符；icacls 的 /grant 接受 "User:F" 形式，
        // 用引号包裹更稳妥。若 USERNAME 未设置则退到 SID S-1-5-32-545（Users），
        // 至少保留可访问性，不让权限设置直接失败。
        let user = std::env::var("USERNAME")
            .ok()
            .filter(|name| !name.trim().is_empty());
        let grant_arg = match &user {
            Some(name) => format!("\"{name}\":F"),
            None => "*S-1-5-32-545:F".to_string(),
        };
        let status = {
            let mut command = Command::new("icacls");
            hide_console_window(&mut command);
            command
                .arg(path)
                .arg("/inheritance:r")
                .arg("/grant:r")
                .arg(grant_arg)
                .arg("/remove:g")
                .arg("Users")
                .arg("Everyone")
                .output()
        };

        match status {
            Ok(output) if !output.status.success() => {
                tracing::warn!(
                    "failed to restrict bilibili session permissions with icacls: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(err) => {
                tracing::warn!("failed to run icacls for bilibili session permissions: {err}");
            }
            _ => {}
        }
    }

    Ok(())
}

fn merge_set_cookie_headers(
    headers: &HeaderMap,
    cookies: &mut BTreeMap<String, String>,
    expires: &mut BTreeMap<String, u64>,
) {
    let now = now_secs();
    for value in headers.get_all(SET_COOKIE).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        // Set-Cookie: <name>=<value>; Expires=...; Max-Age=...; Path=/; ...
        let mut parts = value.split(';');
        let Some((name, cookie_value)) = parts.next().and_then(|part| part.split_once('=')) else {
            continue;
        };
        let name = name.trim();
        let cookie_value = cookie_value.trim();
        if name.is_empty() || cookie_value.is_empty() {
            continue;
        }

        // 解析 Max-Age / Expires 任意一项
        let mut expire_at: Option<u64> = None;
        for attr in parts {
            let attr = attr.trim();
            if let Some(rest) = attr.strip_prefix(|c: char| c.eq_ignore_ascii_case(&'M')) {
                // 简化：用 to_ascii_lowercase 再判前缀更可靠
                let _ = rest;
            }
            let lower = attr.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("max-age=") {
                if let Ok(seconds) = rest.trim().parse::<i64>() {
                    if seconds <= 0 {
                        // 立即过期：跳过这个 cookie
                        cookies.remove(name);
                        expires.remove(name);
                        expire_at = Some(0);
                        break;
                    }
                    expire_at = Some(now.saturating_add(seconds as u64));
                }
            } else if let Some(rest) = lower.strip_prefix("expires=") {
                // 仅在没有 Max-Age 时考虑（Max-Age 优先）；这里如果已设过则跳过
                if expire_at.is_some() {
                    continue;
                }
                if let Some(timestamp) = parse_http_date_to_unix(rest.trim()) {
                    expire_at = Some(timestamp);
                }
            }
        }

        match expire_at {
            Some(0) => {
                // 已被立即过期处理掉了
            }
            Some(ts) if ts <= now => {
                // 显式已过期：清掉
                cookies.remove(name);
                expires.remove(name);
            }
            Some(ts) => {
                cookies.insert(name.to_string(), cookie_value.to_string());
                expires.insert(name.to_string(), ts);
            }
            None => {
                // session cookie，无过期时间
                cookies.insert(name.to_string(), cookie_value.to_string());
                expires.remove(name);
            }
        }
    }
}

/// 解析 RFC 7231 IMF-fixdate / RFC 850 / asctime 三种 HTTP 日期为 Unix 秒。
/// 这里仅做最小可用实现（IMF-fixdate 走 chrono-free 手解析）。失败返回 None。
fn parse_http_date_to_unix(text: &str) -> Option<u64> {
    // 例：Sun, 06 Nov 1994 08:49:37 GMT
    let mut iter = text.split_whitespace();
    let _weekday = iter.next()?;
    let day: u32 = iter.next()?.trim_end_matches(',').parse().ok()?;
    let month_str = iter.next()?;
    let year: i32 = iter.next()?.parse().ok()?;
    let time = iter.next()?;
    let mut t = time.split(':');
    let h: u32 = t.next()?.parse().ok()?;
    let m: u32 = t.next()?.parse().ok()?;
    let s: u32 = t.next()?.parse().ok()?;
    let month: u32 = match month_str {
        "Jan" => 1, "Feb" => 2, "Mar" => 3, "Apr" => 4, "May" => 5, "Jun" => 6,
        "Jul" => 7, "Aug" => 8, "Sep" => 9, "Oct" => 10, "Nov" => 11, "Dec" => 12,
        _ => return None,
    };
    // 计算自 1970-01-01 起的天数（Howard Hinnant 算法）
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era as i64 * 146097 + doe as i64 - 719468;
    if days < 0 {
        return None;
    }
    let secs = days as u64 * 86_400 + h as u64 * 3600 + m as u64 * 60 + s as u64;
    Some(secs)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}
