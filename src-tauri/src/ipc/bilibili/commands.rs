use super::cancellation::{cancel_batch, BatchImportGuard, ImportCancellation};
use super::prelude::*;

/// 批量导入进度事件通道（模式同 `seraph://ffmpeg-download`）。
const BATCH_PROGRESS_EVENT: &str = "seraph://bilibili-batch";

#[tauri::command]
pub fn cancel_bilibili_favorites_import(task_id: Option<String>) {
    cancel_batch(task_id.as_deref());
}

pub(crate) async fn import_bilibili_audio_inner_cmd(
    app: AppHandle,
    input: String,
) -> Result<ImportedTrack, String> {
    import_bilibili_audio_with_options_inner_cmd(app, input, None).await
}

pub(crate) async fn import_bilibili_audio_with_options_inner_cmd(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
) -> Result<ImportedTrack, String> {
    let client = bilibili_client_for_app(&app).await?;
    import_bilibili_audio_inner(
        &app,
        &client,
        &input,
        &options.unwrap_or_default(),
        true,
        &ImportCancellation::default(),
    )
    .await
}

pub(crate) async fn import_bilibili_favorites_inner_cmd(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
    task_id: Option<String>,
) -> Result<BilibiliBatchImportResult, String> {
    let task = BatchImportGuard::start(task_id)?;
    let cancellation = &task.0;

    let media_id = extract_media_id(&input).ok_or_else(|| {
        "没有找到有效的 B 站收藏夹 media_id/fid，请粘贴收藏夹链接或数字 ID".to_string()
    })?;
    let options = options.unwrap_or_default();
    let client = bilibili_client_for_app(&app).await?;
    let bvids = match cancellation
        .run(fetch_favorite_bvids(&client, &media_id, FAV_MAX_ITEMS))
        .await
    {
        Ok(bvids) => bvids,
        Err(_) if cancellation.is_cancelled() => {
            return Ok(BilibiliBatchImportResult {
                tracks: Vec::new(),
                failed: Vec::new(),
                cancelled: true,
            })
        }
        Err(error) => return Err(error),
    };
    if bvids.is_empty() {
        return Err("收藏夹里没有可导入的视频，或当前账号没有访问权限".into());
    }

    let total = bvids.len();
    let mut tracks = Vec::new();
    let mut failed = Vec::new();
    let mut cancelled = false;
    // 审2-S3：收集本批全部成功导入的音频路径（ImportedTrack.path 即 ensure_audio_file
    // 返回的最终落盘路径，含 remux fallback），批量结束后统一清理时整体 preserve。
    let mut imported_paths = Vec::new();
    for (index, item) in bvids.into_iter().enumerate() {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        let bvid = item.bvid.clone().unwrap_or_default();
        let display_name = item.title.clone().unwrap_or_else(|| bvid.clone());
        let ok =
            match import_bilibili_audio_inner(&app, &client, &bvid, &options, false, cancellation)
                .await
            {
                Ok(track) => {
                    imported_paths.push(PathBuf::from(&track.path));
                    tracks.push(track);
                    true
                }
                Err(_) if cancellation.is_cancelled() => {
                    cancelled = true;
                    break;
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
                task_id: cancellation.id().to_string(),
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

pub(crate) async fn bilibili_login_qrcode_inner_cmd(
    app: AppHandle,
) -> Result<BilibiliLoginQrCode, String> {
    let client = bilibili_client_for_app(&app).await?;
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
        data_url: super::qr::login_qr_data_url(&api.url)?,
        url: api.url,
        qrcode_key: api.qrcode_key,
    })
}

pub(crate) async fn bilibili_poll_login_inner_cmd(
    app: AppHandle,
    qrcode_key: String,
) -> Result<BilibiliLoginPollResult, String> {
    let qrcode_key = qrcode_key.trim();
    if qrcode_key.is_empty() {
        return Err("缺少二维码 key".into());
    }

    let client = bilibili_client_for_app(&app).await?;
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

        let profile = bilibili_login_status_inner_cmd(app.clone()).await?;
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

pub(crate) async fn bilibili_login_status_inner_cmd(
    app: AppHandle,
) -> Result<BilibiliLoginStatus, String> {
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

    // SESS-01：前端会周期性查询登录状态，同一份 Cookie 复用同一个 client。
    let client = bilibili_client_cached(session.cookie_header().as_deref())?;
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
                // F-07：该 client 另挂逐跳重定向复验，避免图床 302 变成 SSRF 探针。
                let avatar_client = bilibili_avatar_client()?;
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

pub(crate) fn bilibili_logout_inner_cmd(app: AppHandle) -> Result<(), String> {
    let path = session_path(&app)?;
    // SESS-01：先清缓存——带旧 Cookie 的 client 不能在退出登录后继续被复用。
    // 放在最前面：后面删凭据即使失败，也不会留着一个随时可用的登录态 client。
    clear_bilibili_client_cache();
    delete_secure_bilibili_cookies()?;
    if path.is_file() {
        fs::remove_file(&path)
            .map_err(|err| format!("无法删除 B 站登录会话 {}: {err}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn bilibili_ffmpeg_status_inner_cmd(
    app: AppHandle,
) -> Result<BilibiliFfmpegStatus, String> {
    let path = find_ffmpeg(&app);
    Ok(BilibiliFfmpegStatus {
        available: path.is_some(),
        path: path.map(|value| value.to_string_lossy().to_string()),
    })
}

/// 一键下载并安装 ffmpeg / ffprobe 到 `app_data_dir/ffmpeg`，使 EAC3 /
/// 杜比全景声等 Symphonia 无法解码的格式可以走 ffmpeg fallback 播放。
/// 下载进度通过 [`FFMPEG_DOWNLOAD_EVENT`] 实时推送给前端。
pub(crate) async fn download_ffmpeg_inner_cmd(
    app: AppHandle,
) -> Result<BilibiliFfmpegStatus, String> {
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

// ---- 命令边界（BE-03 / BE-04）----
// 上面的 `*_inner_cmd` 保持 `Result<_, String>`；这里统一转成结构化错误（带错误码）。
// 退出登录与 FFmpeg 探测有同步 IO（凭据管理器、删文件、逐个候选路径探测），
// 同步命令会占住主线程，所以改成 async + 阻塞线程。

use crate::ipc::error::{bilibili_error, IpcError, IpcResult};

async fn blocking_command<T, F>(job: F) -> IpcResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|err| IpcError::from(format!("bilibili task panicked: {err}")))?
        .map_err(bilibili_error)
}

#[tauri::command]
pub async fn import_bilibili_audio(app: AppHandle, input: String) -> IpcResult<ImportedTrack> {
    import_bilibili_audio_inner_cmd(app, input)
        .await
        .map_err(bilibili_error)
}

#[tauri::command]
pub async fn import_bilibili_audio_with_options(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
) -> IpcResult<ImportedTrack> {
    import_bilibili_audio_with_options_inner_cmd(app, input, options)
        .await
        .map_err(bilibili_error)
}

#[tauri::command]
pub async fn import_bilibili_favorites(
    app: AppHandle,
    input: String,
    options: Option<BilibiliImportOptions>,
    task_id: Option<String>,
) -> IpcResult<BilibiliBatchImportResult> {
    import_bilibili_favorites_inner_cmd(app, input, options, task_id)
        .await
        .map_err(bilibili_error)
}

#[tauri::command]
pub async fn bilibili_login_qrcode(app: AppHandle) -> IpcResult<BilibiliLoginQrCode> {
    bilibili_login_qrcode_inner_cmd(app)
        .await
        .map_err(bilibili_error)
}

#[tauri::command]
pub async fn bilibili_poll_login(
    app: AppHandle,
    qrcode_key: String,
) -> IpcResult<BilibiliLoginPollResult> {
    bilibili_poll_login_inner_cmd(app, qrcode_key)
        .await
        .map_err(bilibili_error)
}

#[tauri::command]
pub async fn bilibili_login_status(app: AppHandle) -> IpcResult<BilibiliLoginStatus> {
    bilibili_login_status_inner_cmd(app)
        .await
        .map_err(bilibili_error)
}

#[tauri::command]
pub async fn bilibili_logout(app: AppHandle) -> IpcResult<()> {
    blocking_command(move || bilibili_logout_inner_cmd(app)).await
}

#[tauri::command]
pub async fn bilibili_ffmpeg_status(app: AppHandle) -> IpcResult<BilibiliFfmpegStatus> {
    blocking_command(move || bilibili_ffmpeg_status_inner_cmd(app)).await
}

#[tauri::command]
pub async fn download_ffmpeg(app: AppHandle) -> IpcResult<BilibiliFfmpegStatus> {
    download_ffmpeg_inner_cmd(app).await.map_err(bilibili_error)
}
