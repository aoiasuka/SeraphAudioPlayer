//! 文件写入的公共做法。

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// 原子写：内容先写到同目录的临时文件并落盘同步，再改名替换目标。
///
/// 直接 `fs::write` 会先把目标截断再写——中途失败（磁盘满、U 盘被拔）时原文件已经没了。
/// 用这个函数，目标要么是完整的新内容，要么还是原来的内容（PL-02 / SEC3-03）。
/// 临时文件名唯一，并发写同一个目标不会互相覆盖；失败时清掉临时文件。
///
/// 目标所在目录必须能新建文件；目标正被别的程序独占打开时改名会失败并报错，原文件不动。
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = temp_sibling(path);
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// `<目标路径>.<纳秒>-<pid>-<序号>.tmp`：与目标同目录（同卷改名才是原子的）。
fn temp_sibling(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    PathBuf::from(format!(
        "{}.{nanos}-{}-{}.tmp",
        path.display(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(case: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("seraph-fs-util-{}-{case}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn replaces_existing_content_and_leaves_no_temp_file() {
        let dir = temp_dir("replace");
        let target = dir.join("list.m3u8");
        write_atomic(&target, b"first").unwrap();
        write_atomic(&target, b"second").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");
        assert_eq!(file_names(&dir), ["list.m3u8"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_write_keeps_the_original_file_intact() {
        // 目标是个目录时改名必然失败——等价于「写到最后一步出错」
        let dir = temp_dir("failure");
        let target = dir.join("occupied");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), b"original").unwrap();
        assert!(write_atomic(&target, b"new content").is_err());
        assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"original");
        assert_eq!(file_names(&dir), ["occupied"], "失败后临时文件要清掉");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn target_held_open_by_another_program_is_not_truncated() {
        use std::os::windows::fs::OpenOptionsExt;
        // PL-02：此前 `fs::write` 先截断再写。这里模拟别的程序占着目标文件：
        // 写不进去时原内容必须原样留着。
        let dir = temp_dir("locked");
        let target = dir.join("config.json");
        fs::write(&target, b"original").unwrap();
        {
            let _held = fs::OpenOptions::new()
                .read(true)
                .share_mode(1) // FILE_SHARE_READ：别人可读，不可写、不可删、不可改名
                .open(&target)
                .unwrap();
            assert!(write_atomic(&target, b"new content").is_err());
            assert_eq!(fs::read(&target).unwrap(), b"original");
        }
        assert_eq!(file_names(&dir), ["config.json"]);
        write_atomic(&target, b"new content").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new content");
        let _ = fs::remove_dir_all(&dir);
    }
}
