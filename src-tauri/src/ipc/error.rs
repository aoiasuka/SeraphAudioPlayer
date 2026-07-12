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
    #[allow(dead_code)] // M3U8 导入导出命令落地后使用
    Io,
    /// 网络请求失败（在线歌词/封面/更新检查）
    Network,
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

/// 内部 `Result<_, String>` 在命令边界经 `?` 自动转为通用 Internal 错误。
/// 需要具体错误码时在命令体内显式构造 [`IpcError`]。
impl From<String> for IpcError {
    fn from(message: String) -> Self {
        // 曲库损坏信息由 read_cached_tracks_for_update 拼装，识别后归类到具体码，
        // 让前端能对"曲库损坏"给出区别于普通错误的提示。
        let code = if message.contains("曲库缓存损坏") {
            IpcErrorCode::CacheCorrupt
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
    fn from_string_detects_cache_corruption() {
        let err: IpcError = "曲库缓存损坏，已中止写入".to_string().into();
        assert_eq!(err.code, IpcErrorCode::CacheCorrupt);
    }
}
