//! 元数据与歌词用不可变文件保存，最后一次原子替换清单才代表提交成功。
//! 保留上一份清单及其文件；旧版 library-cache.json / library-lyrics.json 保留作迁移备份。

use super::types::{ImportedTrack, LyricDocument, EMPTY_LYRICS};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MANIFEST: &str = "library-snapshot.json";
const PREVIOUS: &str = "library-snapshot.previous.json";
const SNAPSHOTS: &str = "library-snapshots";
/// 迁移到快照存储之前的两个文件，只在没有任何清单时读取。
const LEGACY_TRACKS: &str = "library-cache.json";
const LEGACY_LYRICS: &str = "library-lyrics.json";
/// 同一个目录里最多保留多少份本模块生成的 `.corrupt` 现场（STO-01）。一次无法恢复的损坏
/// 最多涉及两代的元数据与歌词共 4 个文件，8 份够留下最近两次事故；更早的按生成顺序淘汰。
pub(crate) const MAX_CORRUPT_BACKUPS_PER_DIR: usize = 8;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

fn unique_suffix() -> String {
    format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    )
}

/// 借用元数据字段，不克隆或编码歌词；同时用于列表摘要和变更比较。
#[derive(Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackMetadata<'a> {
    id: &'a str,
    title: &'a str,
    artist: &'a str,
    album: &'a str,
    album_year: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    track_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disc_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    album_artist: &'a Option<String>,
    cover: &'a str,
    format: &'a str,
    bitdepth: &'a str,
    sample_rate: &'a str,
    bitrate: &'a str,
    channels: &'a str,
    size: &'a str,
    path: &'a str,
    source_url: &'a Option<String>,
    source_id: &'a Option<String>,
    cache_missing: bool,
    duration: u64,
    glow_color: &'a str,
    glow1: &'a str,
    glow2: &'a str,
    lyrics: &'a LyricDocument,
}

impl<'a> From<&'a ImportedTrack> for TrackMetadata<'a> {
    fn from(track: &'a ImportedTrack) -> Self {
        Self {
            id: &track.id,
            title: &track.title,
            artist: &track.artist,
            album: &track.album,
            album_year: &track.album_year,
            track_number: track.track_number,
            disc_number: track.disc_number,
            album_artist: &track.album_artist,
            cover: &track.cover,
            format: &track.format,
            bitdepth: &track.bitdepth,
            sample_rate: &track.sample_rate,
            bitrate: &track.bitrate,
            channels: &track.channels,
            size: &track.size,
            path: &track.path,
            source_url: &track.source_url,
            source_id: &track.source_id,
            cache_missing: track.cache_missing,
            duration: track.duration,
            glow_color: &track.glow_color,
            glow1: &track.glow1,
            glow2: &track.glow2,
            lyrics: &EMPTY_LYRICS,
        }
    }
}

/// 清单版本：1 = 歌词文件是 `{id: [旧格式行数组]}`（v0.5.12–v0.6.1）；
/// 2 = `{id: LyricDocument}`（2026-09-20 模型重构）。读时两者都接受（`LyricDocument`
/// 的反序列化兼容旧数组），写时一律 2；旧程序遇到 2 会拒绝读写（不回滚数据）。
const MANIFEST_VERSION: u32 = 2;
const SUPPORTED_MANIFEST_VERSIONS: [u32; 2] = [1, 2];

#[derive(Clone, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    tracks: String,
    lyrics: String,
}

impl Manifest {
    fn validate(&self) -> Result<(), String> {
        if !SUPPORTED_MANIFEST_VERSIONS.contains(&self.version)
            || !is_snapshot_name(&self.tracks)
            || !is_snapshot_name(&self.lyrics)
        {
            return Err("不支持的曲库快照清单或非法文件名".into());
        }
        Ok(())
    }
}

fn is_snapshot_name(name: &str) -> bool {
    let Some(stem) = name
        .strip_suffix("-tracks.json")
        .or_else(|| name.strip_suffix("-lyrics.json"))
    else {
        return false;
    };
    let mut parts = stem.split('-');
    (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

#[derive(Debug, Default)]
pub(crate) struct SaveStats {
    pub(crate) metadata_bytes: usize,
    pub(crate) lyrics_bytes: usize,
}

/// 冷读失败。`stable` 为真表示「存储文件不变，结果就不会变」（内容损坏、文件缺失、
/// 版本不支持），可以在进程内记住（见 [`LoadFailureMemo`]）；读写盘的 IO 错误可能只是
/// 一时的（文件被别的进程独占、网络盘抖动），不算。
#[derive(Debug)]
pub(crate) struct LoadFailure {
    pub(crate) message: String,
    pub(crate) stable: bool,
}

impl LoadFailure {
    fn stable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            stable: true,
        }
    }

    fn transient(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            stable: false,
        }
    }
}

impl From<LoadFailure> for String {
    fn from(failure: LoadFailure) -> Self {
        failure.message
    }
}

/// 冷读会碰到的全部文件的（名字、大小、修改时间），用来判断上次读失败之后存储有没有变过。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StorageFingerprint(Vec<(String, u64, Option<SystemTime>)>);

pub(crate) struct LibraryStorage {
    root: PathBuf,
}

impl LibraryStorage {
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// 调用者串行化磁盘读写（LIBRARY_LOCK），包括恢复和旧文件清理。
    /// 生产路径经 [`LoadFailureMemo::load`] 进来；这个不带记忆的版本只给测试用。
    #[cfg(test)]
    pub(crate) fn load(&self) -> Result<Vec<ImportedTrack>, String> {
        self.try_load().map_err(String::from)
    }

    /// 冷读；失败时带一个「这次失败是否稳定」的标记。
    pub(crate) fn try_load(&self) -> Result<Vec<ImportedTrack>, LoadFailure> {
        let path = self.root.join(MANIFEST);
        let current = read_optional_json::<Manifest>(&path);
        if let Ok(Some(manifest)) = &current {
            // 旧程序遇到未来格式应停止，不能用旧备份覆盖新版本的数据。
            if !SUPPORTED_MANIFEST_VERSIONS.contains(&manifest.version) {
                return Err(LoadFailure::stable(format!(
                    "不支持的曲库快照版本：{}",
                    manifest.version
                )));
            }
            return match self.read_snapshot(manifest) {
                Ok(tracks) => Ok(tracks),
                Err(err) => self.recover(err),
            };
        }
        match current {
            Err(err) => self.recover(err),
            Ok(None) if self.root.join(PREVIOUS).exists() => {
                self.recover(LoadFailure::stable("当前清单缺失"))
            }
            Ok(None) => {
                let tracks =
                    read_optional_json::<Vec<ImportedTrack>>(&self.root.join(LEGACY_TRACKS))?
                        .unwrap_or_default();
                let lyrics = read_optional_json::<BTreeMap<String, LyricDocument>>(
                    &self.root.join(LEGACY_LYRICS),
                )?
                .unwrap_or_default();
                Ok(merge_lyrics(tracks, lyrics))
            }
            Ok(Some(_)) => unreachable!(),
        }
    }

    fn read_snapshot(&self, manifest: &Manifest) -> Result<Vec<ImportedTrack>, LoadFailure> {
        manifest.validate().map_err(LoadFailure::stable)?;
        let dir = self.root.join(SNAPSHOTS);
        let tracks = read_required_json(&dir.join(&manifest.tracks))?;
        let lyrics = read_required_json(&dir.join(&manifest.lyrics))?;
        Ok(merge_lyrics(tracks, lyrics))
    }

    fn recover(&self, error: LoadFailure) -> Result<Vec<ImportedTrack>, LoadFailure> {
        let recovered = (|| {
            let previous: Manifest = read_required_json(&self.root.join(PREVIOUS))?;
            let tracks = self.read_snapshot(&previous)?;
            let path = self.root.join(MANIFEST);
            if path.exists() {
                backup_corrupt_file(&path).map_err(LoadFailure::transient)?;
            }
            let bytes = json_bytes(&previous).map_err(LoadFailure::transient)?;
            write_json_atomic(&path, &bytes).map_err(LoadFailure::transient)?;
            Ok::<_, LoadFailure>(tracks)
        })();
        match recovered {
            Ok(tracks) => {
                tracing::warn!(
                    "曲库快照损坏，已恢复上一份完整快照，损坏文件保留为 .corrupt：{}",
                    error.message
                );
                Ok(tracks)
            }
            Err(recovery) => Err(LoadFailure {
                message: format!(
                    "{}：曲库读取失败，已中止写入以免覆盖数据：{}；上一版恢复失败：{}",
                    crate::ipc::error::CACHE_CORRUPT_MARKER,
                    error.message,
                    recovery.message
                ),
                stable: error.stable && recovery.stable,
            }),
        }
    }

    /// 冷读涉及的文件现状。清单、上一版清单、两个旧版文件，加上快照目录里本模块生成的代次文件；
    /// `.corrupt` 现场与临时文件不算（冷读自己会产生它们）。
    pub(crate) fn fingerprint(&self) -> StorageFingerprint {
        let mut files = Vec::new();
        let mut stamp = |name: String, path: &Path| {
            // 取不到元数据（不存在、一时读不了）就当它不在：指纹因此不同，下次照常重读
            if let Ok(meta) = fs::metadata(path) {
                files.push((name, meta.len(), meta.modified().ok()));
            }
        };
        for name in [MANIFEST, PREVIOUS, LEGACY_TRACKS, LEGACY_LYRICS] {
            stamp(name.to_string(), &self.root.join(name));
        }
        if let Ok(entries) = fs::read_dir(self.root.join(SNAPSHOTS)) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if is_snapshot_name(name) {
                    stamp(format!("{SNAPSHOTS}/{name}"), &entry.path());
                }
            }
        }
        files.sort();
        StorageFingerprint(files)
    }

    pub(crate) fn save(
        &self,
        tracks: &[ImportedTrack],
        previous: Option<&[ImportedTrack]>,
    ) -> Result<SaveStats, String> {
        self.save_with(tracks, previous, write_json_atomic)
    }

    /// 可注入写失败以验证任一提交阶段中断后，旧清单仍然可读。
    pub(super) fn save_with(
        &self,
        tracks: &[ImportedTrack],
        previous_tracks: Option<&[ImportedTrack]>,
        mut write: impl FnMut(&Path, &[u8]) -> Result<(), String>,
    ) -> Result<SaveStats, String> {
        let dir = self.root.join(SNAPSHOTS);
        fs::create_dir_all(&dir).map_err(|err| format!("无法创建曲库目录：{err}"))?;
        let current: Option<Manifest> = read_optional_json(&self.root.join(MANIFEST))?;
        if let Some(manifest) = &current {
            manifest.validate()?;
        }
        let metadata_changed = current.is_none()
            || previous_tracks.is_none_or(|previous| {
                tracks.len() != previous.len()
                    || tracks
                        .iter()
                        .zip(previous)
                        .any(|(a, b)| TrackMetadata::from(a) != TrackMetadata::from(b))
            });
        let lyrics = lyrics_view(tracks);
        let lyrics_changed = current.is_none()
            || previous_tracks.is_none_or(|previous| lyrics != lyrics_view(previous));
        // v1 清单升级：歌词文件格式变了，即使内容没变也要把元数据与歌词按新格式重写一遍
        let upgrading = current
            .as_ref()
            .is_some_and(|manifest| manifest.version != MANIFEST_VERSION);
        let metadata_changed = metadata_changed || upgrading;
        let lyrics_changed = lyrics_changed || upgrading;
        let mut stats = SaveStats::default();
        if !metadata_changed && !lyrics_changed {
            return Ok(stats);
        }

        let generation = unique_suffix();
        let mut next = current.clone().unwrap_or(Manifest {
            version: MANIFEST_VERSION,
            tracks: String::new(),
            lyrics: String::new(),
        });
        next.version = MANIFEST_VERSION;
        if metadata_changed {
            let bytes = json_bytes(&tracks.iter().map(TrackMetadata::from).collect::<Vec<_>>())?;
            next.tracks = format!("{generation}-tracks.json");
            write(&dir.join(&next.tracks), &bytes)?;
            stats.metadata_bytes = bytes.len();
        }
        if lyrics_changed {
            let bytes = json_bytes(&lyrics)?;
            next.lyrics = format!("{generation}-lyrics.json");
            write(&dir.join(&next.lyrics), &bytes)?;
            stats.lyrics_bytes = bytes.len();
        }
        // 备份的是旧清单引用，不覆盖旧数据。直到最后一次 rename，读者都只会看到旧版本。
        if let Some(manifest) = &current {
            write(&self.root.join(PREVIOUS), &json_bytes(manifest)?)?;
        }
        write(&self.root.join(MANIFEST), &json_bytes(&next)?)?;
        self.cleanup(&next, current.as_ref());
        Ok(stats)
    }

    fn cleanup(&self, current: &Manifest, previous: Option<&Manifest>) {
        let keep: HashSet<&str> = std::iter::once(current)
            .chain(previous)
            .flat_map(|manifest| [manifest.tracks.as_str(), manifest.lyrics.as_str()])
            .collect();
        let Ok(entries) = fs::read_dir(self.root.join(SNAPSHOTS)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if is_snapshot_name(name) && !keep.contains(name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn lyrics_view(tracks: &[ImportedTrack]) -> BTreeMap<&str, &LyricDocument> {
    tracks
        .iter()
        .filter(|track| !track.lyrics.is_empty())
        .map(|track| (track.id.as_str(), &track.lyrics))
        .collect()
}

fn merge_lyrics(
    mut tracks: Vec<ImportedTrack>,
    mut lyrics: BTreeMap<String, LyricDocument>,
) -> Vec<ImportedTrack> {
    // 正常曲库 ID 唯一。旧版重复 ID 仍取得同一份边车歌词。
    for track in &mut tracks {
        if let Some(lines) = lyrics.get_mut(&track.id) {
            track.lyrics.clone_from(lines);
        }
    }
    tracks
}

fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|err| format!("曲库序列化失败：{err}"))
}

fn read_required_json<T: DeserializeOwned>(path: &Path) -> Result<T, LoadFailure> {
    read_optional_json(path)?
        .ok_or_else(|| LoadFailure::stable(format!("曲库快照文件缺失：{}", path.display())))
}

fn read_optional_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, LoadFailure> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(LoadFailure::transient(format!(
                "无法读取 {}：{err}",
                path.display()
            )))
        }
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|err| {
        // 备份的就是刚才解析失败的这份字节，不再回头读一遍文件
        let backup = match backup_corrupt_bytes(path, &bytes) {
            Ok(backup) => format!("备份 {}", backup.display()),
            Err(error) => format!("备份失败，原文件保留：{error}"),
        };
        LoadFailure::stable(format!("无法解析 {}（{}）：{err}", path.display(), backup))
    })
}

/// 把坏文件另存为 `<原文件名>.<唯一后缀>.corrupt`，原文件不动。
pub(crate) fn backup_corrupt_file(path: &Path) -> Result<PathBuf, String> {
    let bytes = fs::read(path).map_err(|err| format!("无法备份 {}：{err}", path.display()))?;
    backup_corrupt_bytes(path, &bytes)
}

/// STO-01：同一份坏内容只留一份现场。曲库无法恢复时每条曲库命令都会重走一遍冷读，
/// 此前每次都再复制一份——大曲库的元数据文件几十 MB 一份，没有上限也没有流程清理。
/// 已有内容相同的现场就直接沿用；新写的现场落盘后按 [`MAX_CORRUPT_BACKUPS_PER_DIR`] 淘汰旧的。
fn backup_corrupt_bytes(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let source = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let existing = list_corrupt_backups(dir);
    let identical = existing.iter().find(|backup| {
        backup.source == source
            && backup.len == bytes.len() as u64
            && file_has_content(&backup.path, bytes)
    });
    let backup = match identical {
        Some(backup) => backup.path.clone(),
        None => {
            let backup = PathBuf::from(format!("{}.{}.corrupt", path.display(), unique_suffix()));
            if let Err(err) = fs::write(&backup, bytes) {
                // 写到一半（磁盘满）的现场没有意义，也不能让它被当成已备份
                let _ = fs::remove_file(&backup);
                return Err(format!("无法备份 {}：{err}", path.display()));
            }
            backup
        }
    };
    prune_corrupt_backups(dir, &backup);
    Ok(backup)
}

/// 本模块生成的一份损坏现场。
struct CorruptBackup {
    path: PathBuf,
    /// 被备份的原文件名
    source: String,
    /// 生成顺序（纳秒时间戳、进程内序号），越大越新
    order: (u128, u128),
    len: u64,
}

/// `<原文件名>.<纳秒>-<pid>-<序号>.corrupt` → （原文件名, 生成顺序）。后缀不是
/// [`unique_suffix`] 的形状就不是本模块的文件（例如别的模块写的 `xxx.json.corrupt`）。
fn parse_corrupt_backup_name(name: &str) -> Option<(&str, (u128, u128))> {
    let (source, suffix) = name.strip_suffix(".corrupt")?.rsplit_once('.')?;
    let mut parts = suffix.split('-').map(|part| {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        part.parse::<u128>().ok()
    });
    let (nanos, _pid, sequence) = (parts.next()??, parts.next()??, parts.next()??);
    (parts.next().is_none() && !source.is_empty()).then_some((source, (nanos, sequence)))
}

/// 目录里全部损坏现场，最新的在前。
fn list_corrupt_backups(dir: &Path) -> Vec<CorruptBackup> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut backups: Vec<CorruptBackup> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let (source, order) = parse_corrupt_backup_name(name.to_str()?)?;
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| CorruptBackup {
                path: entry.path(),
                source: source.to_string(),
                order,
                len: meta.len(),
            })
        })
        .collect();
    backups.sort_by_key(|backup| std::cmp::Reverse(backup.order));
    backups
}

/// 只淘汰本模块自己的现场：原文件名必须是清单、上一版清单、旧版两个文件或代次文件。
fn is_owned_backup_source(source: &str) -> bool {
    matches!(source, MANIFEST | PREVIOUS | LEGACY_TRACKS | LEGACY_LYRICS)
        || is_snapshot_name(source)
}

/// 每个目录只留最新的 [`MAX_CORRUPT_BACKUPS_PER_DIR`] 份现场；`keep`（本次用到的那份）一定留下。
fn prune_corrupt_backups(dir: &Path, keep: &Path) {
    let owned: Vec<CorruptBackup> = list_corrupt_backups(dir)
        .into_iter()
        .filter(|backup| is_owned_backup_source(&backup.source))
        .collect();
    let is_kept = |backup: &CorruptBackup| backup.path.file_name() == keep.file_name();
    let mut retained = usize::from(owned.iter().any(is_kept));
    for backup in owned.iter().filter(|backup| !is_kept(backup)) {
        if retained < MAX_CORRUPT_BACKUPS_PER_DIR {
            retained += 1;
        } else if let Err(err) = fs::remove_file(&backup.path) {
            tracing::debug!("无法清理旧的损坏现场 {}：{err}", backup.path.display());
        }
    }
}

/// 流式比较文件内容，不把整个现场读进内存。读失败按「不同」处理（宁可多备份一份）。
fn file_has_content(path: &Path, expected: &[u8]) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut offset = 0_usize;
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return offset == expected.len(),
            Ok(read) => {
                if expected.get(offset..offset + read) != Some(&buffer[..read]) {
                    return false;
                }
                offset += read;
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}

/// 冷读失败的进程内记忆（STO-01）。曲库无法恢复时内存快照一直是空的，此前每条曲库命令
/// （切歌时的 `get_track_info`、导入、删除……）都把磁盘上几十 MB 的文件重读一遍再失败一次。
/// 这里记下失败时的存储指纹与错误：存储文件没有变化就直接返回同一个错误，不碰磁盘；
/// 用户换回好的文件后指纹变化，下一条命令照常重读。只记 [`LoadFailure::stable`] 的失败，
/// 一时的 IO 错误每次都重试。
pub(crate) struct LoadFailureMemo(parking_lot::Mutex<Option<RememberedFailure>>);

struct RememberedFailure {
    root: PathBuf,
    fingerprint: StorageFingerprint,
    message: String,
}

impl LoadFailureMemo {
    pub(crate) const fn new() -> Self {
        Self(parking_lot::Mutex::new(None))
    }

    /// 调用者与 [`LibraryStorage::load`] 一样要持有曲库锁。
    pub(crate) fn load(&self, storage: &LibraryStorage) -> Result<Vec<ImportedTrack>, String> {
        // 指纹在读之前取：读的过程中文件被换掉，下次比较时指纹必然不同，不会把旧错误粘住
        let fingerprint = storage.fingerprint();
        if let Some(remembered) = self.0.lock().as_ref() {
            if remembered.root == storage.root && remembered.fingerprint == fingerprint {
                return Err(remembered.message.clone());
            }
        }
        match storage.try_load() {
            Ok(tracks) => {
                *self.0.lock() = None;
                Ok(tracks)
            }
            Err(failure) => {
                *self.0.lock() = failure.stable.then(|| RememberedFailure {
                    root: storage.root.clone(),
                    fingerprint,
                    message: failure.message.clone(),
                });
                Err(failure.message)
            }
        }
    }
}

/// 数据先 flush 到磁盘，再以同卷 rename 切换。唯一临时名避免并发恢复互相覆盖。
/// 实现在 `ipc::fs_util::write_atomic`，导出类命令也用它（SEC3-03）。
pub(crate) fn write_json_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::ipc::fs_util::write_atomic(path, bytes)
        .map_err(|err| format!("写入 {} 失败：{err}", path.display()))
}
