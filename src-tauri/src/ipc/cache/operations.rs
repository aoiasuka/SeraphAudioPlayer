//! 按来源的下载/删除占用与整库清理共用一份注册表。
//! 持锁时间仅覆盖登记与释放，租约可跨 await；冲突立即报错，绝不等待曲库锁。

use parking_lot::Mutex;
use std::{collections::BTreeSet, sync::LazyLock};

#[derive(Default)]
struct State {
    sources: BTreeSet<String>,
    cleanup: bool,
}

#[derive(Default)]
pub(crate) struct OperationRegistry(Mutex<State>);

impl OperationRegistry {
    pub(crate) fn acquire_source(&self, source: &str) -> Result<AudioOperationSlot<'_>, String> {
        let key = source.trim().replace('\\', "/").to_ascii_lowercase();
        let mut state = self.0.lock();
        if state.cleanup {
            return Err("缓存正在清理或切换目录，请稍后重试".into());
        }
        if !state.sources.insert(key.clone()) {
            return Err("该曲目正在下载、重新缓存或删除，请稍后重试".into());
        }
        Ok(AudioOperationSlot {
            registry: self,
            key,
        })
    }

    pub(crate) fn acquire_cleanup(&self) -> Result<CacheCleanupSlot<'_>, String> {
        let mut state = self.0.lock();
        if state.cleanup || !state.sources.is_empty() {
            return Err("缓存仍有下载、重新缓存或删除任务，请完成后再清理或切换目录".into());
        }
        state.cleanup = true;
        Ok(CacheCleanupSlot(self))
    }
}

pub(crate) struct AudioOperationSlot<'a> {
    registry: &'a OperationRegistry,
    key: String,
}

impl Drop for AudioOperationSlot<'_> {
    fn drop(&mut self) {
        self.registry.0.lock().sources.remove(&self.key);
    }
}

pub(crate) struct CacheCleanupSlot<'a>(&'a OperationRegistry);

impl Drop for CacheCleanupSlot<'_> {
    fn drop(&mut self) {
        self.0 .0.lock().cleanup = false;
    }
}

pub(crate) static OPERATIONS: LazyLock<OperationRegistry> =
    LazyLock::new(OperationRegistry::default);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_and_source_operations_are_exclusive_without_holding_a_mutex() {
        let registry = OperationRegistry::default();
        let download = registry.acquire_source("BV-A").unwrap();
        assert!(registry.acquire_source("bv-a").is_err());
        assert!(registry.acquire_cleanup().is_err());
        let other = registry.acquire_source("BV-B").unwrap();
        drop(download);
        assert!(registry.acquire_cleanup().is_err());
        drop(other);
        let cleanup = registry.acquire_cleanup().unwrap();
        assert!(registry.acquire_source("BV-C").is_err());
        assert!(registry.acquire_cleanup().is_err());
        drop(cleanup);
        assert!(registry.acquire_source("BV-C").is_ok());
    }
}
