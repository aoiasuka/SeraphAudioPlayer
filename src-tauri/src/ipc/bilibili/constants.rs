const VIEW_API: &str = "https://api.bilibili.com/x/web-interface/view";
const PLAY_URL_API: &str = "https://api.bilibili.com/x/player/playurl";
const NAV_API: &str = "https://api.bilibili.com/x/web-interface/nav";
const QR_GENERATE_API: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
const QR_POLL_API: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
const FAV_RESOURCE_LIST_API: &str = "https://api.bilibili.com/x/v3/fav/resource/list";
const BILIBILI_REFERER: &str = "https://www.bilibili.com";
const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0 Safari/537.36";
const PLAY_URL_FNVAL: &str = "4048";
const FAV_PAGE_SIZE: usize = 20;
const FAV_MAX_ITEMS: usize = 200;
const MAX_AVATAR_BYTES: usize = 512 * 1024;
/// 单次音频下载上限。普通 m4a < 50 MB，FLAC 流可达数百 MB，给到 1.5 GB 已远超合理上限。
const MAX_AUDIO_DOWNLOAD_BYTES: u64 = 1_500 * 1024 * 1024;
/// B 站视频页 HTML 抓取上限：BVID 一般在前若干 KB 出现，给 1 MB 防御性裁剪。
const MAX_HTML_BYTES: u64 = 1024 * 1024;

/// 前端监听的 ffmpeg 下载进度频道。
pub const FFMPEG_DOWNLOAD_EVENT: &str = "seraph://ffmpeg-download";
/// ffmpeg 压缩包下载上限（防御性裁剪，正常 essentials 包 ~40-80 MB）。
const MAX_FFMPEG_DOWNLOAD_BYTES: u64 = 400 * 1024 * 1024;
/// Windows x64 ffmpeg 静态构建候选下载地址，按顺序尝试直到某个成功。
/// 同时含官方源与镜像，缓解国内 GitHub 直连不稳定的问题。
#[cfg(windows)]
const FFMPEG_DOWNLOAD_URLS: &[&str] = &[
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
    "https://github.com/GyanD/codexffmpeg/releases/latest/download/ffmpeg-release-essentials.zip",
    "https://mirror.ghproxy.com/https://github.com/GyanD/codexffmpeg/releases/latest/download/ffmpeg-release-essentials.zip",
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
];
