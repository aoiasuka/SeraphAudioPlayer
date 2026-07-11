/// 审2-S5：ffmpeg 下载进行中标记。download_ffmpeg 命令可被前端重复触发，
/// 并发下载会互相覆盖/截断落盘文件，这里全局串行化为同时最多一个任务。
static FFMPEG_DOWNLOAD_IN_FLIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 审2-S5：下载占位 guard——Drop 时复位标记，确保任何返回路径
/// （成功 / 失败 / panic 展开）都能释放，不会把标记卡死在 true。
struct FfmpegDownloadSlot;

impl Drop for FfmpegDownloadSlot {
    fn drop(&mut self) {
        FFMPEG_DOWNLOAD_IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// 审2-S5：compare_exchange(false→true) 抢占下载权，已有任务在跑则直接报错。
fn acquire_ffmpeg_download_slot() -> Result<FfmpegDownloadSlot, String> {
    if FFMPEG_DOWNLOAD_IN_FLIGHT
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err("FFmpeg 正在下载中".into());
    }
    Ok(FfmpegDownloadSlot)
}

#[cfg(not(windows))]
async fn download_ffmpeg_inner(_app: &AppHandle) -> Result<BilibiliFfmpegStatus, String> {
    Err("自动下载暂仅支持 Windows，请手动安装 ffmpeg 并加入 PATH".into())
}

#[cfg(windows)]
async fn download_ffmpeg_inner(app: &AppHandle) -> Result<BilibiliFfmpegStatus, String> {
    let ffmpeg_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("无法定位应用数据目录: {err}"))?
        .join("ffmpeg");
    fs::create_dir_all(&ffmpeg_dir)
        .map_err(|err| format!("无法创建 ffmpeg 目录 {}: {err}", ffmpeg_dir.display()))?;

    let client = ffmpeg_download_client()?;
    let archive_path = ffmpeg_dir.join("ffmpeg-download.zip");

    let mut last_error = String::from("没有可用的下载地址");
    for (index, url) in FFMPEG_DOWNLOAD_URLS.iter().enumerate() {
        emit_ffmpeg_progress(
            app,
            FfmpegDownloadProgress {
                stage: "download",
                downloaded: 0,
                total: 0,
                percent: 0.0,
                message: Some(format!(
                    "正在连接下载源 {}/{}…",
                    index + 1,
                    FFMPEG_DOWNLOAD_URLS.len()
                )),
            },
        );

        match download_to_file(&client, url, &archive_path, app).await {
            Ok(()) => match extract_ffmpeg_tools(&archive_path, &ffmpeg_dir) {
                Ok(()) => {
                    let _ = fs::remove_file(&archive_path);
                    // find_ffmpeg 会重新搜索工具目录并刷新解码器缓存。
                    if let Some(path) = find_ffmpeg(app) {
                        return Ok(BilibiliFfmpegStatus {
                            available: true,
                            path: Some(path.to_string_lossy().to_string()),
                        });
                    }
                    last_error = "下载完成但仍未能定位 ffmpeg 可执行文件".to_string();
                }
                Err(reason) => {
                    let _ = fs::remove_file(&archive_path);
                    last_error = format!("解压失败: {reason}");
                }
            },
            Err(reason) => {
                let _ = fs::remove_file(&archive_path);
                last_error = reason;
            }
        }
    }

    Err(format!(
        "ffmpeg 下载失败：{last_error}。可手动下载 ffmpeg.exe 与 ffprobe.exe 放入 {}",
        ffmpeg_dir.display()
    ))
}

fn emit_ffmpeg_progress(app: &AppHandle, progress: FfmpegDownloadProgress) {
    use tauri::Emitter as _;
    let _ = app.emit(FFMPEG_DOWNLOAD_EVENT, progress);
}

#[cfg(windows)]
fn ffmpeg_download_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .user_agent(USER_AGENT_VALUE)
        .build()
        .map_err(|err| format!("无法创建下载客户端: {err}"))
}

/// 流式下载到文件，边写边推送进度。
/// 审2-S5：先写唯一临时名，完整落盘后 rename 到目标，失败路径清掉临时文件；
/// 防止下载中断 / 并发写残留半截 zip 被后续解压流程误用。
#[cfg(windows)]
async fn download_to_file(
    client: &Client,
    url: &str,
    dest: &Path,
    app: &AppHandle,
) -> Result<(), String> {
    let temp_path = unique_temp_path(dest);
    if let Err(err) = download_to_temp_file(client, url, &temp_path, app).await {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }
    fs::rename(&temp_path, dest).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        format!("无法落盘下载文件 {}: {err}", dest.display())
    })
}

#[cfg(windows)]
async fn download_to_temp_file(
    client: &Client,
    url: &str,
    dest: &Path,
    app: &AppHandle,
) -> Result<(), String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("请求失败: {err}"))?
        .error_for_status()
        .map_err(|err| format!("下载源返回错误: {err}"))?;

    let total = response.content_length().unwrap_or(0);
    let mut file = fs::File::create(dest).map_err(|err| format!("无法写入临时文件: {err}"))?;
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("下载中断: {err}"))?
    {
        downloaded += chunk.len() as u64;
        if downloaded > MAX_FFMPEG_DOWNLOAD_BYTES {
            return Err("下载内容超出体积上限，已中止".into());
        }
        file.write_all(&chunk)
            .map_err(|err| format!("写入失败: {err}"))?;

        // 每累积 ~1 MB 推送一次进度，避免事件风暴。
        if downloaded - last_emit >= 1024 * 1024 {
            last_emit = downloaded;
            let percent = if total > 0 {
                (downloaded as f64 / total as f64) * 100.0
            } else {
                -1.0
            };
            emit_ffmpeg_progress(
                app,
                FfmpegDownloadProgress {
                    stage: "download",
                    downloaded,
                    total,
                    percent,
                    message: None,
                },
            );
        }
    }

    file.flush().map_err(|err| format!("刷新文件失败: {err}"))?;
    Ok(())
}

/// 从 zip 包中提取 ffmpeg.exe 与 ffprobe.exe 到目标目录（忽略包内层级）。
#[cfg(windows)]
fn extract_ffmpeg_tools(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|err| format!("无法打开压缩包: {err}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| format!("压缩包格式无效: {err}"))?;

    let wanted = ["ffmpeg.exe", "ffprobe.exe"];
    let mut extracted: Vec<String> = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("读取压缩条目失败: {err}"))?;
        if !entry.is_file() {
            continue;
        }
        let entry_name = entry.name().to_ascii_lowercase();
        let file_name = entry_name.rsplit(['/', '\\']).next().unwrap_or(&entry_name);
        let Some(target) = wanted.iter().find(|name| **name == file_name) else {
            continue;
        };
        if extracted.iter().any(|done| done == *target) {
            continue;
        }

        let out_path = dest_dir.join(target);
        let mut out_file =
            fs::File::create(&out_path).map_err(|err| format!("无法写入 {target}: {err}"))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|err| format!("解压 {target} 失败: {err}"))?;
        extracted.push((*target).to_string());
    }

    if !extracted.iter().any(|name| name == "ffmpeg.exe") {
        return Err("压缩包内未找到 ffmpeg.exe".into());
    }
    if !extracted.iter().any(|name| name == "ffprobe.exe") {
        return Err("压缩包内未找到 ffprobe.exe".into());
    }
    Ok(())
}
