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
    let media_id = extract_media_id(&input).ok_or_else(|| {
        "没有找到有效的 B 站收藏夹 media_id/fid，请粘贴收藏夹链接或数字 ID".to_string()
    })?;
    let options = options.unwrap_or_default();
    let client = bilibili_client_for_app(&app)?;
    let bvids = fetch_favorite_bvids(&client, &media_id, FAV_MAX_ITEMS).await?;
    if bvids.is_empty() {
        return Err("收藏夹里没有可导入的视频，或当前账号没有访问权限".into());
    }

    let mut tracks = Vec::new();
    let mut failed = Vec::new();
    // 审2-S3：收集本批全部成功导入的音频路径（ImportedTrack.path 即 ensure_audio_file
    // 返回的最终落盘路径，含 remux fallback），批量结束后统一清理时整体 preserve。
    let mut imported_paths = Vec::new();
    for item in bvids {
        let bvid = item.bvid.clone().unwrap_or_default();
        let display_name = item.title.clone().unwrap_or_else(|| bvid.clone());
        match import_bilibili_audio_inner(&app, &client, &bvid, &options, false).await {
            Ok(track) => {
                imported_paths.push(PathBuf::from(&track.path));
                tracks.push(track);
            }
            Err(reason) => failed.push(BilibiliImportFailure {
                input: display_name,
                reason,
            }),
        }
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

    Ok(BilibiliBatchImportResult { tracks, failed })
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
        let mut session = load_session(&app)?.unwrap_or_default();
        merge_set_cookie_headers(&headers, &mut session.cookies, &mut session.cookie_expires);
        session.saved_at = now_secs();
        save_session(&app, &session)?;

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
    let Some(session) = load_session(&app)? else {
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
            Some(face) => resolve_avatar_data_url(&client, face)
                .await
                .or_else(|_| Ok::<_, String>(Some(normalize_url(face))))
                .ok()
                .flatten(),
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
            save_session(&app, &next_session)?;
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
