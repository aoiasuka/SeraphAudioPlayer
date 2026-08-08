use super::prelude::*;

/// M-15：收藏夹批量导入的取消令牌。批量最多 200 首串行下载可达数小时，
/// 必须给用户中断手段；置位后当前这首完成即停止，已导入部分照常返回。
static FAVORITES_IMPORT_CANCELLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 批量导入进度事件通道（模式同 `seraph://ffmpeg-download`）。
const BATCH_PROGRESS_EVENT: &str = "seraph://bilibili-batch";

#[tauri::command]
pub fn cancel_bilibili_favorites_import() {
    FAVORITES_IMPORT_CANCELLED.store(true, std::sync::atomic::Ordering::Release);
}

#[tauri::command]
pub async fn import_bilibili_audio(app: AppHandle, input: String) -> Result<ImportedTrack, String> {
    import_bilibili_audio_with_options(app, input, None).await
}

#[tauri::command]
pub async fn import_bilibili_audio_with_options(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
) -> Result<ImportedTrack, String> {
    let client = bilibili_client_for_app(&app)?;
    import_bilibili_audio_inner(&app, &client, &input, &options.unwrap_or_default(), true).await
}

#[tauri::command]
pub async fn import_bilibili_favorites(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
) -> Result<BilibiliBatchImportResult, String> {
    use std::sync::atomic::Ordering;

    let media_id = extract_media_id(&input).ok_or_else(|| {
        "没有找到有效的 B 站收藏夹 media_id/fid，请粘贴收藏夹链接或数字 ID".to_string()
    })?;
    let options = options.unwrap_or_default();
    let client = bilibili_client_for_app(&app)?;
    let bvids = fetch_favorite_bvids(&client, &media_id, FAV_MAX_ITEMS).await?;
    if bvids.is_empty() {
        return Err("收藏夹里没有可导入的视频，或当前账号没有访问权限".into());
    }

    // 新一轮批量开始：清掉上一轮可能残留的取消标记
    FAVORITES_IMPORT_CANCELLED.store(false, Ordering::Release);

    let total = bvids.len();
    let mut tracks = Vec::new();
    let mut failed = Vec::new();
    let mut cancelled = false;
    // 审2-S3：收集本批全部成功导入的音频路径（ImportedTrack.path 即 ensure_audio_file
    // 返回的最终落盘路径，含 remux fallback），批量结束后统一清理时整体 preserve。
    let mut imported_paths = Vec::new();
    for (index, item) in bvids.into_iter().enumerate() {
        if FAVORITES_IMPORT_CANCELLED.load(Ordering::Acquire) {
            cancelled = true;
            break;
        }
        let bvid = item.bvid.clone().unwrap_or_default();
        let display_name = item.title.clone().unwrap_or_else(|| bvid.clone());
        let ok = match import_bilibili_audio_inner(&app, &client, &bvid, &options, false).await {
            Ok(track) => {
                imported_paths.push(PathBuf::from(&track.path));
                tracks.push(track);
                true
            }
            Err(reason) => {
                failed.push(BilibiliImportFailure {
                    input: display_name.clone(),
                    reason,
                });
                false
            }
        };
        // M-15：每首完成即推进度事件，前端展示 N/total 与当前曲目
        let _ = app.emit(
            BATCH_PROGRESS_EVENT,
            &BilibiliBatchProgress {
                current: index + 1,
                total,
                title: display_name,
                ok,
            },
        );
    }

    // 审2-S3：整批只在结束后清理一次缓存并 preserve 本批全部成功导入的文件，
    // 替代逐首清理只 preserve 当前一首（超限时会把同批先导入的文件删掉）。
    // 同步磁盘遍历放 spawn_blocking，失败只 warn 不影响导入结果（对齐 S2 语义）。
    if !imported_paths.is_empty() {
        let app_for_cache = app.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            enforce_cache_limit_preserving_many(&app_for_cache, &imported_paths)
        })
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => tracing::warn!("收藏夹批量导入后缓存清理失败: {err}"),
            Err(err) => tracing::warn!("收藏夹批量导入后缓存清理任务异常终止: {err}"),
        }
    }

    Ok(BilibiliBatchImportResult {
        tracks,
        failed,
        cancelled,
    })
}

#[tauri::command]
pub async fn bilibili_login_qrcode(app: AppHandle) -> Result<BilibiliLoginQrCode, String> {
    let client = bilibili_client_for_app(&app)?;
    let response = client
        .get(QR_GENERATE_API)
        .send()
        .await
        .map_err(|err| format!("无法请求 B 站二维码: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站二维码请求失败: {err}"))?;
    let api = parse_json_response::<ApiResponse<QrGenerateData>>(response, "bilibili qrcode")
        .await?
        .into_data("bilibili qrcode")?;

    Ok(BilibiliLoginQrCode {
        url: api.url,
        qrcode_key: api.qrcode_key,
    })
}

#[tauri::command]
pub async fn bilibili_poll_login(
    app: AppHandle,
    qrcode_key: String,
) -> Result<BilibiliLoginPollResult, String> {
    let qrcode_key = qrcode_key.trim();
    if qrcode_key.is_empty() {
        return Err("缺少二维码 key".into());
    }

    let client = bilibili_client_for_app(&app)?;
    let response = client
        .get(QR_POLL_API)
        .query(&[("qrcode_key", qrcode_key)])
        .send()
        .await
        .map_err(|err| format!("无法轮询 B 站登录状态: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站登录轮询失败: {err}"))?;
    let headers = response.headers().clone();
    let api = parse_json_response::<ApiResponse<QrPollData>>(response, "bilibili login poll")
        .await?
        .into_data("bilibili login poll")?;

    if api.code == 0 {
        // M-16：session 落盘含 Credential Manager 写入 + icacls 同步子进程，
        // 挪出 tokio worker（headers 已 clone，纯数据可安全移入）。
        let app_for_save = app.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
            let mut session = load_session(&app_for_save)?.unwrap_or_default();
            merge_set_cookie_headers(&headers, &mut session.cookies, &mut session.cookie_expires);
            session.saved_at = now_secs();
            save_session(&app_for_save, &session)
        })
        .await
        .map_err(|err| format!("保存 B 站登录会话任务异常: {err}"))??;

        let profile = bilibili_login_status(app.clone()).await?;
        return Ok(BilibiliLoginPollResult {
            code: api.code,
            message: api.message.unwrap_or_else(|| "登录成功".into()),
            url: api.url,
            logged_in: profile.logged_in,
            profile: Some(profile),
        });
    }

    Ok(BilibiliLoginPollResult {
        code: api.code,
        message: api.message.unwrap_or_else(|| login_poll_message(api.code)),
        url: api.url,
        logged_in: false,
        profile: None,
    })
}

#[tauri::command]
pub async fn bilibili_login_status(app: AppHandle) -> Result<BilibiliLoginStatus, String> {
    // M-16：session 读取（文件 + Credential Manager）是同步 IO，挪出 tokio worker
    let app_for_load = app.clone();
    let session = tauri::async_runtime::spawn_blocking(move || load_session(&app_for_load))
        .await
        .map_err(|err| format!("读取 B 站登录会话任务异常: {err}"))??;
    let Some(session) = session else {
        return Ok(BilibiliLoginStatus {
            logged_in: false,
            username: None,
            mid: None,
            face: None,
        });
    };

    let client = bilibili_client_with_cookie(session.cookie_header().as_deref())?;
    let response = client
        .get(NAV_API)
        .send()
        .await
        .map_err(|err| format!("无法检查 B 站登录状态: {err}"))?
        .error_for_status()
        .map_err(|err| format!("B 站登录状态请求失败: {err}"))?;
    let api = parse_json_response::<ApiResponse<NavData>>(response, "bilibili nav").await?;
    let data = match api.into_data("bilibili nav") {
        Ok(data) => data,
        Err(_) => {
            return Ok(BilibiliLoginStatus {
                logged_in: false,
                username: None,
                mid: None,
                face: None,
            })
        }
    };

    if data.is_login {
        let face = match data.face.as_deref() {
            Some(face) => {
                // S-02：头像不需要登录态——用不带 Cookie 的裸 client 下载，
                // 防止 face（外部 API 返回的 URL）把 SESSDATA 带向非预期主机；
                // resolve_avatar_data_url 内部还有 https + 官方图床域白名单兜底。
                let avatar_client = bilibili_client_with_cookie(None)?;
                resolve_avatar_data_url(&avatar_client, face)
                    .await
                    .or_else(|_| Ok::<_, String>(Some(normalize_url(face))))
                    .ok()
                    .flatten()
            }
            None => None,
        };
        // P2-8：仅当资料实际变化时才重写 Credential Manager + session 文件，
        // 避免前端周期性查询登录状态时的无谓 CredWriteW / icacls 开销。
        let changed =
            session.username != data.uname || session.mid != data.mid || session.face != face;
        if changed {
            let mut next_session = session;
            next_session.username = data.uname.clone();
            next_session.mid = data.mid;
            next_session.face = face.clone();
            next_session.saved_at = now_secs();
            // M-16：同上，落盘含 icacls 子进程，挪出 tokio worker
            let app_for_save = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                save_session(&app_for_save, &next_session)
            })
            .await
            .map_err(|err| format!("保存 B 站登录会话任务异常: {err}"))??;
        }
        return Ok(BilibiliLoginStatus {
            logged_in: true,
            username: data.uname,
            mid: data.mid,
            face,
        });
    }

    Ok(BilibiliLoginStatus {
        logged_in: false,
        username: None,
        mid: None,
        face: None,
    })
}

#[tauri::command]
pub fn bilibili_logout(app: AppHandle) -> Result<(), String> {
    let path = session_path(&app)?;
    delete_secure_bilibili_cookies()?;
    if path.is_file() {
        fs::remove_file(&path)
            .map_err(|err| format!("无法删除 B 站登录会话 {}: {err}", path.display()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn bilibili_ffmpeg_status(app: AppHandle) -> Result<BilibiliFfmpegStatus, String> {
    let path = find_ffmpeg(&app);
    Ok(BilibiliFfmpegStatus {
        available: path.is_some(),
        path: path.map(|value| value.to_string_lossy().to_string()),
    })
}

/// 一键下载并安装 ffmpeg / ffprobe 到 `app_data_dir/ffmpeg`，使 EAC3 /
/// 杜比全景声等 Symphonia 无法解码的格式可以走 ffmpeg fallback 播放。
/// 下载进度通过 [`FFMPEG_DOWNLOAD_EVENT`] 实时推送给前端。
#[tauri::command]
pub async fn download_ffmpeg(app: AppHandle) -> Result<BilibiliFfmpegStatus, String> {
    // 已经可用就直接返回，避免重复下载。
    if let Some(path) = find_ffmpeg(&app) {
        return Ok(BilibiliFfmpegStatus {
            available: true,
            path: Some(path.to_string_lossy().to_string()),
        });
    }

    // 审2-S5：并发保护——同一时刻只允许一个下载任务在跑，重复触发直接报错；
    // guard 存活到本函数返回，任何路径（成功/失败）都经 Drop 复位标记。
    let _download_slot = acquire_ffmpeg_download_slot()?;
    let result = download_ffmpeg_inner(&app).await;
    match &result {
        Ok(status) => emit_ffmpeg_progress(
            &app,
            FfmpegDownloadProgress {
                stage: "done",
                downloaded: 0,
                total: 0,
                percent: 100.0,
                message: status.path.clone(),
            },
        ),
        Err(reason) => emit_ffmpeg_progress(
            &app,
            FfmpegDownloadProgress {
                stage: "error",
                downloaded: 0,
                total: 0,
                percent: -1.0,
                message: Some(reason.clone()),
            },
        ),
    }
    result
}
