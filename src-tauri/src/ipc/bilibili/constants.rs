// 纯常量模块，无需 prelude
pub(crate) const VIEW_API: &str = "https://api.bilibili.com/x/web-interface/view";
pub(crate) const PLAY_URL_API: &str = "https://api.bilibili.com/x/player/playurl";
pub(crate) const NAV_API: &str = "https://api.bilibili.com/x/web-interface/nav";
pub(crate) const QR_GENERATE_API: &str =
    "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
pub(crate) const QR_POLL_API: &str =
    "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
pub(crate) const FAV_RESOURCE_LIST_API: &str = "https://api.bilibili.com/x/v3/fav/resource/list";
pub(crate) const BILIBILI_REFERER: &str = "https://www.bilibili.com";
pub(crate) const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0 Safari/537.36";
pub(crate) const PLAY_URL_FNVAL: &str = "4048";
pub(crate) const FAV_PAGE_SIZE: usize = 20;
pub(crate) const FAV_MAX_ITEMS: usize = 200;
pub(crate) const MAX_AVATAR_BYTES: usize = 512 * 1024;
/// 单次音频下载上限。普通 m4a < 50 MB，FLAC 流可达数百 MB，给到 1.5 GB 已远超合理上限。
pub(crate) const MAX_AUDIO_DOWNLOAD_BYTES: u64 = 1_500 * 1024 * 1024;
/// BILI-01：批量导入攒够这么多首就合并写一次曲库。
pub(crate) const BATCH_COMMIT_MAX_TRACKS: usize = 20;
/// BILI-01：待提交队列里最早一首最多等这么久。下载慢时不必等凑满一批；
/// 程序中途被结束时最多丢这段时间内下好的曲目（音频文件仍在缓存里，重新导入直接命中）。
pub(crate) const BATCH_COMMIT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(10);
/// BILI-03：同一地址下载中断后，每次续传前的等待。连续这么多次续传都没有拿到新数据才放弃
/// （合计约 30 秒，覆盖短暂断网）；期间只要有新数据写入就从第一档重新计。
pub(crate) const DOWNLOAD_RESUME_BACKOFF: [std::time::Duration; 5] = [
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(4),
    std::time::Duration::from_secs(8),
    std::time::Duration::from_secs(15),
];
/// BILI-03：一次下载的续传总次数上限，防止「每次只挤出一点数据」的连接无限拖下去。
pub(crate) const MAX_DOWNLOAD_RESUMES: u32 = 30;
/// B 站视频页 HTML 抓取上限：BVID 一般在前若干 KB 出现，给 1 MB 防御性裁剪。
pub(crate) const MAX_HTML_BYTES: u64 = 1024 * 1024;

/// 前端监听的 ffmpeg 下载进度频道。
pub const FFMPEG_DOWNLOAD_EVENT: &str = "seraph://ffmpeg-download";
/// ffmpeg 压缩包下载上限（防御性裁剪，正常 essentials 包 ~40-80 MB）。
pub(crate) const MAX_FFMPEG_DOWNLOAD_BYTES: u64 = 400 * 1024 * 1024;
/// S-01：ffmpeg 下载候选（URL + 官方 SHA-256），按顺序尝试直到某个成功。
/// P1-4：只保留官方/第一方来源；S-01 进一步固定版本并校验哈希——
/// 此前的 latest 滚动链接内容会随上游更新漂移，无法锚定完整性，
/// 上游账号被盗/TLS 异常时恶意二进制会被直接执行。
/// 两个候选是同一构建（gyan.dev 官网 + 其官方 GitHub 镜像 GyanD/codexffmpeg），
/// 哈希取自 gyan.dev 发布的 .sha256 文件，并经双源实际下载交叉核验一致。
#[cfg(windows)]
pub(crate) const FFMPEG_DOWNLOADS: &[FfmpegDownloadSource] = &[
    FfmpegDownloadSource {
        url: "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0-essentials_build.zip",
        sha256: "e6b54767a6065919048f1a098eb27211ca4e12b4348a05d88777a5855d0b6e71",
    },
    FfmpegDownloadSource {
        url: "https://github.com/GyanD/codexffmpeg/releases/download/9.0/ffmpeg-9.0-essentials_build.zip",
        sha256: "e6b54767a6065919048f1a098eb27211ca4e12b4348a05d88777a5855d0b6e71",
    },
];

/// ffmpeg 下载源描述：升级版本时同时更新 url 与官方公布的 SHA-256。
#[cfg(windows)]
pub(crate) struct FfmpegDownloadSource {
    pub(crate) url: &'static str,
    pub(crate) sha256: &'static str,
}
