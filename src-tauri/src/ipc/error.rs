//! 结构化 IPC 错误。
//!
//! Tauri 命令原先统一返回 `Result<T, String>`，前端只能靠字符串猜错误类型。
//! [`IpcError`] 序列化为 `{ code, message }`，前端可按 `code` 分支处理，
//! `message` 仍是面向用户的中文描述。
//!
//! 迁移策略：命令体内部的辅助函数仍返回 `Result<_, String>`，在命令边界经
//! `From<String>`（映射为通用 code）或显式构造（具体 code）转成 [`IpcError`]，
//! 改动量集中在命令签名，不动内部逻辑。

use serde::{Serialize, Serializer};
use thiserror::Error;

/// IPC 错误码：前端按此分支。新增变体时同步前端 `IpcErrorCode` 联合类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcErrorCode {
    /// 通用/未分类错误（由 `String` 转换而来的默认码）
    Internal,
    /// 入参非法（空 id、越界、格式错误等）
    InvalidInput,
    /// 目标资源不存在（曲目/歌单/文件）
    NotFound,
    /// 曲库缓存损坏（已备份坏文件、拒绝覆盖写）
    CacheCorrupt,
    /// 文件系统/IO 失败
    Io,
    /// 网络请求失败（在线歌词/封面/更新检查）
    Network,
    /// 队列增量同步的版本对不上（后端重启 / 队列被别的入口改写），前端应重发完整队列
    QueueRevisionMismatch,
    /// 上游拒绝访问（401 / 403）：需要登录或内容受限
    AuthRequired,
    /// 上游资源不存在（404）：链接已失效
    LinkExpired,
    /// 请求超时
    Timeout,
    /// 视频没有可用的音频流
    NoAudioStream,
}

impl IpcErrorCode {
    /// 稳定的字符串码，前端按此比较。
    pub fn as_str(self) -> &'static str {
        match self {
            IpcErrorCode::Internal => "internal",
            IpcErrorCode::InvalidInput => "invalid_input",
            IpcErrorCode::NotFound => "not_found",
            IpcErrorCode::CacheCorrupt => "cache_corrupt",
            IpcErrorCode::Io => "io",
            IpcErrorCode::Network => "network",
            IpcErrorCode::QueueRevisionMismatch => QUEUE_REVISION_MISMATCH,
            IpcErrorCode::AuthRequired => "auth_required",
            IpcErrorCode::LinkExpired => "link_expired",
            IpcErrorCode::Timeout => "timeout",
            IpcErrorCode::NoAudioStream => "no_audio_stream",
        }
    }
}

/// 序列化为 `{ code, message }` 的结构化 IPC 错误。
#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
}

impl IpcError {
    pub fn new(code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::InvalidInput, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::NotFound, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::Network, message)
    }
}

/// 曲库不可恢复损坏的消息前缀：`storage.rs` 的恢复失败路径以它开头，
/// [`From<String>`] 据此映射为 [`IpcErrorCode::CacheCorrupt`]。两处共用常量，
/// 由 `unrecoverable_snapshot_maps_to_cache_corrupt_code` 回归测试绑定——
/// 此前文案在存储重构时改掉，这个码就再也不会产生（BUG-05）。
pub(crate) const CACHE_CORRUPT_MARKER: &str = "曲库缓存损坏";

/// 队列版本失配：`state.rs` 以这个字符串作为内部错误，命令边界映射为同名错误码。
pub(crate) const QUEUE_REVISION_MISMATCH: &str = "queue_revision_mismatch";

/// B 站命令的错误分类（BE-03 / FE3-01）：内部仍是 `Result<_, String>`，在命令边界按
/// **后端自己生成的固定文案**归类成错误码，前端按码给中文提示，不再对任意消息做子串猜测
/// （文件路径里带 `404`、标题里带 `BV` 都曾被误分类）。
pub(crate) fn bilibili_error(message: String) -> IpcError {
    let lower = message.to_ascii_lowercase();
    let code = if lower.contains("no dash audio") || lower.contains("no usable audio") {
        IpcErrorCode::NoAudioStream
    } else if lower.contains("error (401") || lower.contains("error (403") {
        IpcErrorCode::AuthRequired
    } else if lower.contains("error (404") {
        IpcErrorCode::LinkExpired
    } else if lower.contains("timed out") || lower.contains("operation timeout") {
        IpcErrorCode::Timeout
    } else if message.contains("BV 号")
        || message.contains("media_id")
        || message.contains("二维码 key")
        // BILI-05：指定的分 P 不存在、分 P 数超过全量导入上限
        || message.contains("分 P")
    {
        IpcErrorCode::InvalidInput
    } else {
        return IpcError::from(message);
    };
    IpcError::new(code, message)
}

/// 内部 `Result<_, String>` 在命令边界经 `?` 自动转为通用 Internal 错误。
/// 需要具体错误码时在命令体内显式构造 [`IpcError`]。
impl From<String> for IpcError {
    fn from(message: String) -> Self {
        let code = if message.starts_with(CACHE_CORRUPT_MARKER) {
            IpcErrorCode::CacheCorrupt
        } else if message == QUEUE_REVISION_MISMATCH {
            IpcErrorCode::QueueRevisionMismatch
        } else {
            IpcErrorCode::Internal
        };
        Self::new(code, message)
    }
}

impl From<&str> for IpcError {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

impl Serialize for IpcError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("IpcError", 2)?;
        state.serialize_field("code", self.code.as_str())?;
        state.serialize_field("message", &self.message)?;
        state.end()
    }
}

pub type IpcResult<T> = Result<T, IpcError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_code_and_message() {
        let err = IpcError::not_found("曲目不存在");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "曲目不存在");
    }

    #[test]
    fn from_string_defaults_to_internal() {
        let err: IpcError = "something failed".to_string().into();
        assert_eq!(err.code, IpcErrorCode::Internal);
    }

    #[test]
    fn queue_revision_mismatch_gets_its_own_code() {
        let err: IpcError = QUEUE_REVISION_MISMATCH.to_string().into();
        assert_eq!(err.code, IpcErrorCode::QueueRevisionMismatch);
        assert_eq!(
            serde_json::to_value(&err).unwrap()["code"],
            "queue_revision_mismatch"
        );
    }

    #[test]
    fn bilibili_errors_are_classified_by_backend_generated_text() {
        let code = |message: &str| bilibili_error(message.to_string()).code;
        assert_eq!(
            code("下载失败: HTTP status client error (403 Forbidden) for url (https://x)"),
            IpcErrorCode::AuthRequired
        );
        assert_eq!(
            code("HTTP status client error (404 Not Found) for url (https://x)"),
            IpcErrorCode::LinkExpired
        );
        assert_eq!(
            code("bilibili response has no dash audio streams"),
            IpcErrorCode::NoAudioStream
        );
        assert_eq!(
            code("error sending request: operation timed out"),
            IpcErrorCode::Timeout
        );
        assert_eq!(
            code("没有找到有效的 B 站 BV 号或视频链接"),
            IpcErrorCode::InvalidInput
        );
        assert_eq!(
            code("这个视频共 3 个分 P，没有第 9 P"),
            IpcErrorCode::InvalidInput
        );
        // 路径或标题里碰巧带数字 / BV 字样的普通失败不被误分类
        assert_eq!(
            code("写入 D:/cache/404/BV-live.m4a 失败: 磁盘已满"),
            IpcErrorCode::Internal
        );
    }

    #[test]
    fn from_string_detects_cache_corruption() {
        let err: IpcError = "曲库缓存损坏，已中止写入".to_string().into();
        assert_eq!(err.code, IpcErrorCode::CacheCorrupt);
    }
}
