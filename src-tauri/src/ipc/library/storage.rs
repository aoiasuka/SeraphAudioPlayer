//! 元数据与歌词用不可变文件保存，最后一次原子替换清单才代表提交成功。
//! 保留上一份清单及其文件；旧版 library-cache.json / library-lyrics.json 保留作迁移备份。

use super::types::{ImportedTrack, LyricLine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MANIFEST: &str = "library-snapshot.json";
const PREVIOUS: &str = "library-snapshot.previous.json";
const SNAPSHOTS: &str = "library-snapshots";
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
    lyrics: &'a [LyricLine],
}

impl<'a> From<&'a ImportedTrack> for TrackMetadata<'a> {
    fn from(track: &'a ImportedTrack) -> Self {
        Self {
            id: &track.id,
            title: &track.title,
            artist: &track.artist,
            album: &track.album,
            album_year: &track.album_year,
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
            lyrics: &[],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    tracks: String,
    lyrics: String,
}

impl Manifest {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 || !is_snapshot_name(&self.tracks) || !is_snapshot_name(&self.lyrics) {
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
    pub(crate) fn load(&self) -> Result<Vec<ImportedTrack>, String> {
        let path = self.root.join(MANIFEST);
        let current = read_optional_json::<Manifest>(&path);
        if let Ok(Some(manifest)) = &current {
            // 旧程序遇到未来格式应停止，不能用旧备份覆盖新版本的数据。
            if manifest.version != 1 {
                return Err(format!("不支持的曲库快照版本：{}", manifest.version));
            }
            match self.read_snapshot(manifest) {
                Ok(tracks) => return Ok(tracks),
                Err(err) => return self.recover(&err),
            }
        }
        match current {
            Err(err) => self.recover(&err),
            Ok(None) if self.root.join(PREVIOUS).exists() => self.recover("当前清单缺失"),
            Ok(None) => {
                let tracks = read_optional_json::<Vec<ImportedTrack>>(
                    &self.root.join("library-cache.json"),
                )?
                .unwrap_or_default();
                let lyrics = read_optional_json::<BTreeMap<String, Vec<LyricLine>>>(
                    &self.root.join("library-lyrics.json"),
                )?
                .unwrap_or_default();
                Ok(merge_lyrics(tracks, lyrics))
            }
            Ok(Some(_)) => unreachable!(),
        }
    }

    fn read_snapshot(&self, manifest: &Manifest) -> Result<Vec<ImportedTrack>, String> {
        manifest.validate()?;
        let dir = self.root.join(SNAPSHOTS);
        let tracks = read_required_json(&dir.join(&manifest.tracks))?;
        let lyrics = read_required_json(&dir.join(&manifest.lyrics))?;
        Ok(merge_lyrics(tracks, lyrics))
    }

    fn recover(&self, error: &str) -> Result<Vec<ImportedTrack>, String> {
        let recovered = (|| {
            let previous: Manifest = read_required_json(&self.root.join(PREVIOUS))?;
            let tracks = self.read_snapshot(&previous)?;
            let path = self.root.join(MANIFEST);
            if path.exists() {
                backup_corrupt_file(&path)?;
            }
            write_json_atomic(&path, &json_bytes(&previous)?)?;
            Ok::<_, String>(tracks)
        })();
        match recovered {
            Ok(tracks) => {
                tracing::warn!(
                    "曲库快照损坏，已恢复上一份完整快照，损坏文件保留为 .corrupt：{error}"
                );
                Ok(tracks)
            }
            Err(recovery_error) => Err(format!(
                "曲库读取失败，已中止写入以免覆盖数据：{error}；上一版恢复失败：{recovery_error}"
            )),
        }
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
        let mut stats = SaveStats::default();
        if !metadata_changed && !lyrics_changed {
            return Ok(stats);
        }

        let generation = unique_suffix();
        let mut next = current.clone().unwrap_or(Manifest {
            version: 1,
            tracks: String::new(),
            lyrics: String::new(),
        });
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

fn lyrics_view(tracks: &[ImportedTrack]) -> BTreeMap<&str, &[LyricLine]> {
    tracks
        .iter()
        .filter(|track| !track.lyrics.is_empty())
        .map(|track| (track.id.as_str(), track.lyrics.as_slice()))
        .collect()
}

fn merge_lyrics(
    mut tracks: Vec<ImportedTrack>,
    mut lyrics: BTreeMap<String, Vec<LyricLine>>,
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

fn read_required_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    read_optional_json(path)?.ok_or_else(|| format!("曲库快照文件缺失：{}", path.display()))
}

fn read_optional_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("无法读取 {}：{err}", path.display())),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|err| {
        let backup = match backup_corrupt_file(path) {
            Ok(backup) => format!("备份 {}", backup.display()),
            Err(error) => format!("备份失败，原文件保留：{error}"),
        };
        format!("无法解析 {}（{}）：{err}", path.display(), backup)
    })
}

pub(crate) fn backup_corrupt_file(path: &Path) -> Result<PathBuf, String> {
    let backup = PathBuf::from(format!("{}.{}.corrupt", path.display(), unique_suffix()));
    fs::copy(path, &backup).map_err(|err| format!("无法备份 {}：{err}", path.display()))?;
    Ok(backup)
}

/// 数据先 flush 到磁盘，再以同卷 rename 切换。唯一临时名避免并发恢复互相覆盖。
pub(crate) fn write_json_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = PathBuf::from(format!("{}.{}.tmp", path.display(), unique_suffix()));
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
    result.map_err(|err| {
        let _ = fs::remove_file(&temp);
        format!("写入 {} 失败：{err}", path.display())
    })
}
