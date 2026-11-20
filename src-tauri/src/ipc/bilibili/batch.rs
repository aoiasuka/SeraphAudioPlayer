//! 批量导入的待提交队列（BILI-01）。
//!
//! 批量导入逐首下载，但不再逐首整库重写：下载好的曲目先压在这里，攒够
//! [`BATCH_COMMIT_MAX_TRACKS`] 首或最早一首已等了 [`BATCH_COMMIT_MAX_AGE`] 才合并写盘一次；
//! 批量结束、取消或出错时把剩下的一并提交。

use super::prelude::*;
use crate::ipc::cache::operations::AudioOperationSlot;
use std::sync::Arc;
use std::time::Instant;

/// 一首已下载、尚未写进曲库的曲目。
pub(crate) struct PendingImport {
    pub(crate) track: ImportedTrack,
    /// 失败时报给用户的名字
    pub(crate) label: String,
    /// 来源占用的键（BV 号）
    pub(crate) source: String,
    /// 来源占用必须保持到入库完成：否则同一来源的删除 / 缓存清理可以插到
    /// 「文件已落盘」与「曲库已登记」之间，把刚下好的文件删掉。
    pub(crate) operation: Arc<AudioOperationSlot<'static>>,
}

pub(crate) struct PendingImports {
    items: Vec<PendingImport>,
    /// 队列里最早一首的入队时间
    oldest: Option<Instant>,
    max_items: usize,
    max_age: Duration,
}

impl PendingImports {
    pub(crate) fn new() -> Self {
        Self::with_limits(BATCH_COMMIT_MAX_TRACKS, BATCH_COMMIT_MAX_AGE)
    }

    pub(crate) fn with_limits(max_items: usize, max_age: Duration) -> Self {
        Self {
            items: Vec::new(),
            oldest: None,
            max_items: max_items.max(1),
            max_age,
        }
    }

    pub(crate) fn push(&mut self, item: PendingImport, now: Instant) {
        self.oldest.get_or_insert(now);
        self.items.push(item);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 该提交了：攒够数量，或最早一首等得够久。
    pub(crate) fn is_due(&self, now: Instant) -> bool {
        self.items.len() >= self.max_items
            || self
                .oldest
                .is_some_and(|oldest| now.saturating_duration_since(oldest) >= self.max_age)
    }

    /// 队列里是否压着这个来源。压着就得先提交——它的来源占用还没释放，
    /// 同一来源的下一首（同一视频的另一个分 P、收藏夹里的重复条目）拿不到占用。
    pub(crate) fn holds_source(&self, source: &str) -> bool {
        self.items
            .iter()
            .any(|item| item.source.eq_ignore_ascii_case(source.trim()))
    }

    pub(crate) fn take(&mut self) -> Vec<PendingImport> {
        self.oldest = None;
        std::mem::take(&mut self.items)
    }
}

/// 一次提交的结果。
#[derive(Default)]
pub(crate) struct BatchCommit {
    /// 曲库里的最终记录（已有同来源记录时是合并后的那一条）
    pub(crate) tracks: Vec<ImportedTrack>,
    pub(crate) failed: Vec<BilibiliImportFailure>,
}

/// 把一批曲目一次写进曲库。`merge` 即 `merge_tracks_into_cache`（测试里换成桩）。
/// 写失败时这一批逐首记为失败；音频文件留在缓存里，重新导入会直接命中。
/// 来源占用在 `merge` 返回之后才释放。同步执行，调用方放在阻塞线程。
pub(crate) fn commit_pending(
    items: Vec<PendingImport>,
    merge: impl FnOnce(&[ImportedTrack]) -> Result<Vec<ImportedTrack>, String>,
) -> BatchCommit {
    if items.is_empty() {
        return BatchCommit::default();
    }
    let mut labels = Vec::with_capacity(items.len());
    let mut tracks = Vec::with_capacity(items.len());
    let mut operations = Vec::with_capacity(items.len());
    for item in items {
        labels.push(item.label);
        tracks.push(item.track);
        operations.push(item.operation);
    }
    let result = merge(&tracks);
    drop(operations);
    match result {
        Ok(tracks) => BatchCommit {
            tracks,
            failed: Vec::new(),
        },
        Err(reason) => BatchCommit {
            tracks: Vec::new(),
            failed: labels
                .into_iter()
                .map(|input| BilibiliImportFailure {
                    input,
                    reason: format!("写入曲库失败: {reason}"),
                })
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::cache::operations::OperationRegistry;
    use std::cell::Cell;

    fn registry() -> &'static OperationRegistry {
        Box::leak(Box::new(OperationRegistry::default()))
    }

    fn track(id: &str) -> ImportedTrack {
        ImportedTrack {
            id: format!("bilibili-{id}-1"),
            title: id.to_string(),
            artist: "up".into(),
            album: "Bilibili".into(),
            album_year: None,
            track_number: None,
            disc_number: None,
            album_artist: None,
            cover: String::new(),
            format: "M4A".into(),
            bitdepth: String::new(),
            sample_rate: String::new(),
            bitrate: String::new(),
            channels: "Stereo".into(),
            size: "1 MB".into(),
            path: format!("C:/cache/{id}-1.m4a"),
            source_url: None,
            source_id: Some(id.to_string()),
            cache_missing: false,
            duration: 1,
            glow_color: String::new(),
            glow1: String::new(),
            glow2: String::new(),
            lyrics: crate::ipc::library::LyricDocument::EMPTY,
        }
    }

    fn pending(registry: &'static OperationRegistry, id: &str) -> PendingImport {
        PendingImport {
            track: track(id),
            label: format!("标题 {id}"),
            source: id.to_string(),
            operation: Arc::new(registry.acquire_source(id).unwrap()),
        }
    }

    #[test]
    fn bili01_queue_is_due_by_count_or_by_age_of_the_oldest_item() {
        let registry = registry();
        let start = Instant::now();
        let mut queue = PendingImports::with_limits(3, Duration::from_secs(10));
        assert!(
            !queue.is_due(start + Duration::from_secs(3600)),
            "空队列不提交"
        );

        queue.push(pending(registry, "BV1aaaaaaaaaa"), start);
        queue.push(
            pending(registry, "BV1bbbbbbbbbb"),
            start + Duration::from_secs(4),
        );
        assert!(!queue.is_due(start + Duration::from_secs(9)));
        // 按最早一首计时，而不是最近一首
        assert!(queue.is_due(start + Duration::from_secs(10)));

        queue.push(
            pending(registry, "BV1cccccccccc"),
            start + Duration::from_secs(5),
        );
        assert!(
            queue.is_due(start + Duration::from_secs(5)),
            "攒够数量即提交"
        );

        assert!(queue.holds_source(" bv1AAAAAAAAAA "));
        assert!(!queue.holds_source("BV1zzzzzzzzzz"));
        assert_eq!(queue.take().len(), 3);
        // 取走后重新计时
        assert!(queue.is_empty());
        assert!(!queue.is_due(start + Duration::from_secs(3600)));
        assert!(!queue.holds_source("BV1aaaaaaaaaa"));
    }

    /// 一批只写一次曲库；来源占用保持到写完之后。
    #[test]
    fn bili01_commit_merges_once_and_releases_sources_only_afterwards() {
        let registry = registry();
        let items = vec![
            pending(registry, "BV1aaaaaaaaaa"),
            pending(registry, "BV1bbbbbbbbbb"),
        ];
        let calls = Cell::new(0);
        let commit = commit_pending(items, |tracks| {
            calls.set(calls.get() + 1);
            assert_eq!(tracks.len(), 2);
            assert_eq!(tracks[0].title, "BV1aaaaaaaaaa", "保持下载顺序");
            assert!(
                registry.acquire_source("BV1aaaaaaaaaa").is_err(),
                "写盘期间来源仍被占用"
            );
            assert!(registry.acquire_cleanup().is_err(), "写盘期间不能清理缓存");
            Ok(tracks.to_vec())
        });
        assert_eq!(calls.get(), 1);
        assert_eq!(commit.tracks.len(), 2);
        assert!(commit.failed.is_empty());
        assert!(registry.acquire_cleanup().is_ok(), "提交后全部释放");
    }

    /// 写曲库失败：这一批逐首记为失败（带原因），占用照常释放。
    #[test]
    fn bili01_failed_commit_reports_every_track_and_still_releases_sources() {
        let registry = registry();
        let items = vec![
            pending(registry, "BV1aaaaaaaaaa"),
            pending(registry, "BV1bbbbbbbbbb"),
        ];
        let commit = commit_pending(items, |_| Err("磁盘已满".into()));
        assert!(commit.tracks.is_empty());
        assert_eq!(
            commit
                .failed
                .iter()
                .map(|failure| failure.input.as_str())
                .collect::<Vec<_>>(),
            ["标题 BV1aaaaaaaaaa", "标题 BV1bbbbbbbbbb"]
        );
        assert!(commit.failed[0].reason.contains("磁盘已满"));
        assert!(registry.acquire_source("BV1aaaaaaaaaa").is_ok());

        // 空批不碰曲库
        let commit = commit_pending(Vec::new(), |_| panic!("empty batch must not merge"));
        assert!(commit.tracks.is_empty() && commit.failed.is_empty());
    }

    /// 回归闸：45 首按 20 首一批只写 3 次曲库（此前是 45 次），且一首不落。
    #[test]
    fn bili01_forty_five_tracks_cost_three_library_writes() {
        let registry = registry();
        let now = Instant::now();
        let mut queue = PendingImports::with_limits(20, Duration::from_secs(3600));
        let mut writes = 0;
        let mut committed = 0;
        let mut flush = |queue: &mut PendingImports| {
            let commit = commit_pending(queue.take(), |tracks| {
                writes += 1;
                Ok(tracks.to_vec())
            });
            committed += commit.tracks.len();
        };
        for index in 0..45 {
            queue.push(pending(registry, &format!("BV1{index:010}")), now);
            if queue.is_due(now) {
                flush(&mut queue);
            }
        }
        if !queue.is_empty() {
            flush(&mut queue);
        }
        assert_eq!((writes, committed), (3, 45));
    }
}
