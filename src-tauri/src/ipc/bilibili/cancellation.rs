use parking_lot::Mutex;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::Notify;

#[derive(Clone)]
pub(crate) struct ImportCancellation(Arc<Inner>);

struct Inner {
    id: String,
    cancelled: AtomicBool,
    notify: Notify,
}

impl Default for ImportCancellation {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Self::new(format!(
            "{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

impl ImportCancellation {
    fn new(id: String) -> Self {
        Self(Arc::new(Inner {
            id,
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }))
    }
    pub(crate) fn id(&self) -> &str {
        &self.0.id
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("导入已取消".into())
        } else {
            Ok(())
        }
    }
    fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.notify.notify_waiters();
    }
    async fn cancelled(&self) {
        let notified = self.0.notify.notified();
        tokio::pin!(notified);
        // 先注册再检查原子状态，避免取消发生在检查和 await 之间时丢失通知。
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
    pub(crate) async fn run<T>(
        &self,
        operation: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        tokio::select! {
            biased;
            () = self.cancelled() => Err("导入已取消".into()),
            result = operation => result,
        }
    }
}

static ACTIVE_BATCH: Mutex<Option<ImportCancellation>> = Mutex::new(None);

pub(crate) struct BatchImportGuard(pub(crate) ImportCancellation);

impl BatchImportGuard {
    pub(crate) fn start(task_id: Option<String>) -> Result<Self, String> {
        let cancellation = if let Some(id) = task_id {
            if id.is_empty() || id.len() > 128 {
                return Err("无效的导入任务 ID".into());
            }
            ImportCancellation::new(id)
        } else {
            ImportCancellation::default()
        };
        let mut active = ACTIVE_BATCH.lock();
        if active.is_some() {
            return Err("已有收藏夹正在导入，请等待完成或取消".into());
        }
        *active = Some(cancellation.clone());
        Ok(Self(cancellation))
    }
}

impl Drop for BatchImportGuard {
    fn drop(&mut self) {
        let mut active = ACTIVE_BATCH.lock();
        if active
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(&token.0, &self.0 .0))
        {
            *active = None;
        }
    }
}

pub(crate) fn cancel_batch(task_id: Option<&str>) {
    if let Some(token) = ACTIVE_BATCH.lock().as_ref() {
        if task_id.is_none_or(|id| id == token.id()) {
            token.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_interrupts_pending_io_and_cannot_target_another_batch() {
        let first = BatchImportGuard::start(Some("first".into())).unwrap();
        assert!(BatchImportGuard::start(Some("second".into())).is_err());
        cancel_batch(Some("other"));
        assert!(!first.0.is_cancelled());
        let token = first.0.clone();
        let pending = tokio::spawn(async move {
            token
                .run(std::future::pending::<Result<(), String>>())
                .await
        });
        tokio::task::yield_now().await;
        cancel_batch(Some("first"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), pending)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(
            first.0.run(async { Ok(()) }).await.is_err(),
            "已取消令牌不能开始下一阶段"
        );
        drop(first);
        let second = BatchImportGuard::start(Some("second".into())).unwrap();
        cancel_batch(Some("first"));
        assert!(!second.0.is_cancelled());
    }
}
