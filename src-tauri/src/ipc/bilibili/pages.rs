//! 分 P 的选择、身份与命名（BILI-05）。
//!
//! 一个 BV 号下可以有多个分 P（每个分 P 有自己的 cid 与音频流）。此前只取视频信息里的
//! 首个 cid，带 `?p=3` 的链接和「一个 BV 号多首歌」的合集都只能导入第一段。

use super::prelude::*;

/// 视频里的一个分 P（已规整：序号从 1 起、标题去掉首尾空白）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VideoPageInfo {
    pub(crate) number: u32,
    pub(crate) cid: i64,
    pub(crate) part: String,
    /// 这一 P 的时长（秒）；接口没给时为 0
    pub(crate) duration: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageNaming {
    /// 沿用视频信息：标题是视频标题，专辑是「Bilibili」。用于单 P 视频，以及没有指定
    /// 分 P、只导入首 P 的多 P 视频——与分 P 功能出现之前导入的记录完全一致。
    Video,
    /// 按分 P 命名：标题是分 P 标题，专辑是视频标题，音轨号是分 P 序号。
    /// 用于链接指定了分 P、或整个视频全量导入的多 P 视频。
    Part,
}

/// 一个选定要导入的分 P。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedPage {
    pub(crate) number: u32,
    pub(crate) cid: i64,
    pub(crate) part: String,
    pub(crate) duration: u64,
    pub(crate) naming: PageNaming,
}

impl VideoPageInfo {
    fn select(self, naming: PageNaming) -> SelectedPage {
        SelectedPage {
            number: self.number,
            cid: self.cid,
            part: self.part,
            duration: self.duration,
            naming,
        }
    }
}

/// 视频的分 P 列表，按序号升序。接口没给 `pages`（或给的都不可用）时，
/// 用视频信息里的首个 cid 合成单 P——与此前的行为一致。
pub(crate) fn video_pages(video: &VideoData) -> Vec<VideoPageInfo> {
    let mut pages = video
        .pages
        .iter()
        .enumerate()
        .filter(|(_, page)| page.cid > 0)
        .map(|(index, page)| VideoPageInfo {
            number: if page.page > 0 {
                page.page
            } else {
                u32::try_from(index + 1).unwrap_or(u32::MAX)
            },
            cid: page.cid,
            part: page.part.trim().to_string(),
            duration: page.duration,
        })
        .collect::<Vec<_>>();
    pages.sort_by_key(|page| page.number);
    pages.dedup_by_key(|page| page.number);
    if pages.is_empty() {
        pages.push(VideoPageInfo {
            number: 1,
            cid: video.cid,
            part: String::new(),
            duration: video.duration,
        });
    }
    pages
}

/// 决定导入哪些分 P。
///
/// - 链接指定了分 P（`requested`）：只导入那一 P，不存在就报错；
/// - 没有指定、视频有多个分 P：按 `mode`——全部，或只要第 1 P；
/// - 单 P 视频：就是那一 P，`mode` 不起作用。
pub(crate) fn select_pages(
    video: &VideoData,
    requested: Option<u32>,
    mode: MultiPageMode,
) -> Result<Vec<SelectedPage>, String> {
    let pages = video_pages(video);
    let count = pages.len();
    let multiple = count > 1;
    if let Some(number) = requested {
        let naming = if multiple {
            PageNaming::Part
        } else {
            PageNaming::Video
        };
        return pages
            .into_iter()
            .find(|page| page.number == number)
            .map(|page| vec![page.select(naming)])
            .ok_or_else(|| format!("这个视频共 {count} 个分 P，没有第 {number} P"));
    }
    if multiple && mode == MultiPageMode::All {
        if count > MAX_VIDEO_PAGES_PER_IMPORT {
            return Err(format!(
                "这个视频有 {count} 个分 P，超过一次全量导入的上限 {MAX_VIDEO_PAGES_PER_IMPORT}；请用带 ?p= 的链接逐个导入"
            ));
        }
        return Ok(pages
            .into_iter()
            .map(|page| page.select(PageNaming::Part))
            .collect());
    }
    Ok(pages
        .into_iter()
        .take(1)
        .map(|page| page.select(PageNaming::Video))
        .collect())
}

/// 曲库里区分来源的 ID。第 1 P 就是 BV 号——与分 P 功能出现之前的记录是同一条，
/// 不会重复入库；其余分 P 是 `BV…-pN`，各占一条记录。
/// 两种写法 [`extract_bvid`] 都能取回 BV 号（来源占用、删除按 BV 号互斥）。
pub(crate) fn page_source_id(bvid: &str, page: &SelectedPage) -> String {
    if page.number <= 1 {
        bvid.to_string()
    } else {
        format!("{bvid}-p{}", page.number)
    }
}

/// 曲目的来源链接，也是重新缓存时的输入。按分 P 命名的曲目带 `?p=N`（第 1 P 也带），
/// 重新缓存时据此仍按分 P 命名；沿用视频信息的曲目不带，重新缓存仍取首 P。
pub(crate) fn page_source_url(bvid: &str, page: &SelectedPage) -> String {
    match page.naming {
        PageNaming::Video => format!("https://www.bilibili.com/video/{bvid}"),
        PageNaming::Part => format!("https://www.bilibili.com/video/{bvid}?p={}", page.number),
    }
}

pub(crate) fn page_title(video: &VideoData, page: &SelectedPage) -> String {
    match page.naming {
        PageNaming::Video => video.title.clone(),
        PageNaming::Part if page.part.is_empty() => format!("{} P{}", video.title, page.number),
        PageNaming::Part => page.part.clone(),
    }
}

pub(crate) fn page_album(video: &VideoData, page: &SelectedPage) -> String {
    match page.naming {
        PageNaming::Video => "Bilibili".into(),
        PageNaming::Part => video.title.clone(),
    }
}

/// 这一 P 的时长；接口没给分 P 时长时退回整个视频的时长。
pub(crate) fn page_duration(video: &VideoData, page: &SelectedPage) -> u64 {
    if page.duration > 0 {
        page.duration
    } else {
        video.duration
    }
}

/// 批量导入里报给用户的名字：多 P 视频带上分 P 序号与标题。
pub(crate) fn page_label(video_label: &str, page: &SelectedPage) -> String {
    format!("{video_label} · P{} {}", page.number, page.part)
        .trim_end()
        .to_string()
}

/// 给前端的视频概况：有几个分 P、链接有没有指定分 P。
pub(crate) fn video_info(video: &VideoData, requested_page: Option<u32>) -> BilibiliVideoInfo {
    BilibiliVideoInfo {
        bvid: video.bvid.clone(),
        title: video.title.clone(),
        requested_page,
        pages: video_pages(video)
            .into_iter()
            .map(|page| BilibiliVideoPageInfo {
                page: page.number,
                title: page.part,
                duration: page.duration,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BVID: &str = "BV1xx411c7mD";

    fn video(pages: serde_json::Value) -> VideoData {
        serde_json::from_value(json!({
            "bvid": BVID,
            "title": "合集标题",
            "cid": 101,
            "duration": 600,
            "pic": "https://i0.hdslb.com/cover.jpg",
            "owner": { "name": "UP" },
            "pages": pages,
        }))
        .expect("valid video fixture")
    }

    fn three_pages() -> VideoData {
        video(json!([
            { "cid": 101, "page": 1, "part": " 第一首 ", "duration": 180 },
            { "cid": 102, "page": 2, "part": "第二首", "duration": 200 },
            { "cid": 103, "page": 3, "part": "", "duration": 220 },
        ]))
    }

    #[test]
    fn bili05_page_number_comes_from_the_query_or_the_source_id_form() {
        for (input, expected) in [
            ("https://www.bilibili.com/video/BV1xx411c7mD?p=3", Some(3)),
            (
                "https://www.bilibili.com/video/BV1xx411c7mD/?spm_id_from=333.1&p=12",
                Some(12),
            ),
            (
                "https://www.bilibili.com/video/BV1xx411c7mD?p=2&t=30#reply",
                Some(2),
            ),
            ("BV1xx411c7mD?p=4", Some(4)),
            // 曲库里的来源 ID 形式
            ("BV1xx411c7mD-p7", Some(7)),
            (" bv1xx411c7md-p7 ", Some(7)),
            // 没有指定
            ("https://www.bilibili.com/video/BV1xx411c7mD", None),
            ("BV1xx411c7mD", None),
            // 别的参数名以 p 结尾、p 不是数字、p=0、后面粘着字母：都不算
            ("https://www.bilibili.com/video/BV1xx411c7mD?top=3", None),
            ("https://www.bilibili.com/video/BV1xx411c7mD?p=abc", None),
            ("https://www.bilibili.com/video/BV1xx411c7mD?p=0", None),
            ("https://www.bilibili.com/video/BV1xx411c7mD?p=3x", None),
            ("BV1xx411c7mD-p", None),
            ("BV1xx411c7mD-p3x", None),
            ("BV1xx411c7mD-part2", None),
            ("", None),
        ] {
            assert_eq!(extract_page_number(input), expected, "{input}");
        }
    }

    /// 接口旧数据没有 `pages`、或 `pages` 不可用时，退回视频信息里的首个 cid。
    #[test]
    fn bili05_missing_or_unusable_pages_fall_back_to_the_video_cid() {
        let without: VideoData = serde_json::from_value(json!({
            "bvid": BVID, "title": "t", "cid": 101, "duration": 600,
            "pic": null, "owner": { "name": "UP" },
        }))
        .expect("pages is optional");
        for video in [
            without,
            video(json!([])),
            video(json!([{ "cid": 0, "page": 1 }])),
        ] {
            assert_eq!(
                video_pages(&video),
                [VideoPageInfo {
                    number: 1,
                    cid: 101,
                    part: String::new(),
                    duration: 600
                }]
            );
        }

        // 序号缺失按出现顺序补；乱序排好；重复的序号只留一个
        let messy = video(json!([
            { "cid": 203, "page": 3, "part": "c" },
            { "cid": 201, "part": "a" },
            { "cid": 299, "page": 3, "part": "dup" },
        ]));
        let pages = video_pages(&messy);
        assert_eq!(
            pages
                .iter()
                .map(|page| (page.number, page.cid))
                .collect::<Vec<_>>(),
            [(2, 201), (3, 203)]
        );
    }

    #[test]
    fn bili05_page_selection_follows_the_link_then_the_mode() {
        let video = three_pages();
        let numbers = |pages: &[SelectedPage]| {
            pages
                .iter()
                .map(|page| (page.number, page.cid, page.naming))
                .collect::<Vec<_>>()
        };

        // 链接指定了分 P：只要那一 P，mode 不起作用
        for mode in [MultiPageMode::First, MultiPageMode::All] {
            let pages = select_pages(&video, Some(3), mode).unwrap();
            assert_eq!(numbers(&pages), [(3, 103, PageNaming::Part)]);
        }
        // 明确指定第 1 P 也按分 P 命名
        let pages = select_pages(&video, Some(1), MultiPageMode::First).unwrap();
        assert_eq!(numbers(&pages), [(1, 101, PageNaming::Part)]);
        // 指定了不存在的分 P：报错，不悄悄换成别的
        let err = select_pages(&video, Some(9), MultiPageMode::All).unwrap_err();
        assert!(
            err.contains("共 3 个分 P") && err.contains("第 9 P"),
            "{err}"
        );

        // 没有指定：按 mode
        let pages = select_pages(&video, None, MultiPageMode::First).unwrap();
        assert_eq!(numbers(&pages), [(1, 101, PageNaming::Video)]);
        let pages = select_pages(&video, None, MultiPageMode::All).unwrap();
        assert_eq!(
            numbers(&pages),
            [
                (1, 101, PageNaming::Part),
                (2, 102, PageNaming::Part),
                (3, 103, PageNaming::Part)
            ]
        );
        assert_eq!(pages[0].part, "第一首", "分 P 标题去掉首尾空白");

        // 单 P 视频：mode 与 ?p=1 都不改变命名
        let single = video_with_one_page();
        for (requested, mode) in [
            (None, MultiPageMode::All),
            (None, MultiPageMode::First),
            (Some(1), MultiPageMode::All),
        ] {
            let pages = select_pages(&single, requested, mode).unwrap();
            assert_eq!(numbers(&pages), [(1, 101, PageNaming::Video)]);
        }
        assert!(select_pages(&single, Some(2), MultiPageMode::All).is_err());
    }

    fn video_with_one_page() -> VideoData {
        video(json!([{ "cid": 101, "page": 1, "part": "原始文件名", "duration": 600 }]))
    }

    #[test]
    fn bili05_full_import_refuses_videos_with_too_many_pages() {
        let pages = (1..=MAX_VIDEO_PAGES_PER_IMPORT + 1)
            .map(|number| json!({ "cid": 1000 + number, "page": number, "part": "p" }))
            .collect::<Vec<_>>();
        let huge = video(json!(pages));
        let err = select_pages(&huge, None, MultiPageMode::All).unwrap_err();
        assert!(err.contains("上限"), "{err}");
        // 只要首 P、或点名某一 P 不受影响
        assert_eq!(
            select_pages(&huge, None, MultiPageMode::First)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            select_pages(&huge, Some(7), MultiPageMode::All)
                .unwrap()
                .len(),
            1
        );
    }

    /// 身份与命名：第 1 P 沿用 BV 号（与旧记录是同一条），其余分 P 各占一条；
    /// 来源链接能原样还原出 BV 号与分 P。
    #[test]
    fn bili05_page_identity_and_naming() {
        let video = three_pages();
        let all = select_pages(&video, None, MultiPageMode::All).unwrap();
        let first_only = select_pages(&video, None, MultiPageMode::First).unwrap();

        // 只导入首 P：与分 P 功能出现之前完全一样（时长改用这一 P 自己的）
        let legacy = &first_only[0];
        assert_eq!(page_source_id(BVID, legacy), BVID);
        assert_eq!(
            page_source_url(BVID, legacy),
            "https://www.bilibili.com/video/BV1xx411c7mD"
        );
        assert_eq!(page_title(&video, legacy), "合集标题");
        assert_eq!(page_album(&video, legacy), "Bilibili");
        assert_eq!(page_duration(&video, legacy), 180);

        // 全量导入：第 1 P 的来源 ID 不变（同一条记录），链接带 ?p=1 以便重新缓存时保持命名
        assert_eq!(page_source_id(BVID, &all[0]), BVID);
        assert_eq!(
            page_source_url(BVID, &all[0]),
            "https://www.bilibili.com/video/BV1xx411c7mD?p=1"
        );
        assert_eq!(page_title(&video, &all[0]), "第一首");
        assert_eq!(page_album(&video, &all[0]), "合集标题");

        assert_eq!(page_source_id(BVID, &all[1]), "BV1xx411c7mD-p2");
        assert_eq!(page_title(&video, &all[1]), "第二首");
        assert_eq!(page_duration(&video, &all[1]), 200);
        // 分 P 没有标题：用视频标题加序号
        assert_eq!(page_title(&video, &all[2]), "合集标题 P3");
        assert_eq!(
            page_label("收藏夹里的名字", &all[1]),
            "收藏夹里的名字 · P2 第二首"
        );
        assert_eq!(page_label("收藏夹里的名字", &all[2]), "收藏夹里的名字 · P3");

        // 来源 ID、来源链接都能还原
        for page in &all {
            let source_id = page_source_id(BVID, page);
            let source_url = page_source_url(BVID, page);
            assert_eq!(extract_bvid(&source_id).as_deref(), Some(BVID));
            assert_eq!(extract_bvid(&source_url).as_deref(), Some(BVID));
            assert_eq!(extract_page_number(&source_url), Some(page.number));
            // 重新缓存：按来源链接再选一次，得到同一个分 P、同样的命名
            let again = select_pages(
                &video,
                extract_page_number(&source_url),
                MultiPageMode::First,
            )
            .unwrap();
            assert_eq!(again, std::slice::from_ref(page));
        }
        assert_eq!(extract_page_number("BV1xx411c7mD-p2"), Some(2));
        let again = select_pages(
            &video,
            extract_page_number(&page_source_url(BVID, legacy)),
            MultiPageMode::First,
        )
        .unwrap();
        assert_eq!(again, std::slice::from_ref(legacy));

        // 分 P 时长缺失时退回视频时长
        let mut unknown = all[1].clone();
        unknown.duration = 0;
        assert_eq!(page_duration(&video, &unknown), 600);
    }

    /// 生成的曲目记录：各分 P 的曲库键互不相同，第 1 P 与旧记录同键。
    #[test]
    fn bili05_tracks_of_different_pages_do_not_collide_in_the_library() {
        use crate::ipc::library::import_track_key;

        let dir = std::env::temp_dir().join(format!(
            "seraph-bili05-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let video = three_pages();
        let stream = AudioStream {
            base_url: "https://example.com/a.m4s".into(),
            backup_urls: Vec::new(),
            bandwidth: Some(192_000),
            codecs: Some("mp4a.40.2".into()),
            kind: AudioKind::Normal,
        };
        let build = |page: &SelectedPage| {
            let path = dir.join(format!("{BVID}-{}.m4a", page.cid));
            fs::write(&path, b"audio").unwrap();
            track_from_resolved_audio(
                &ResolvedAudio {
                    video: &video,
                    page,
                    stream: stream.clone(),
                },
                &path,
                false,
                None,
            )
            .unwrap()
        };

        let all = select_pages(&video, None, MultiPageMode::All).unwrap();
        let tracks = all.iter().map(build).collect::<Vec<_>>();
        let legacy = build(&select_pages(&video, None, MultiPageMode::First).unwrap()[0]);

        let keys = tracks.iter().map(import_track_key).collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "source-id:bv1xx411c7md",
                "source-id:bv1xx411c7md-p2",
                "source-id:bv1xx411c7md-p3"
            ]
        );
        assert_eq!(
            import_track_key(&legacy),
            keys[0],
            "第 1 P 与旧记录是同一条"
        );
        assert_eq!(
            tracks
                .iter()
                .map(|track| track.id.as_str())
                .collect::<Vec<_>>(),
            [
                "bilibili-bv1xx411c7md-101",
                "bilibili-bv1xx411c7md-102",
                "bilibili-bv1xx411c7md-103"
            ]
        );
        assert_eq!(legacy.id, tracks[0].id);

        assert_eq!(
            (
                tracks[1].title.as_str(),
                tracks[1].album.as_str(),
                tracks[1].track_number
            ),
            ("第二首", "合集标题", Some(2))
        );
        assert_eq!(tracks[1].duration, 200);
        assert_eq!(tracks[1].source_id.as_deref(), Some("BV1xx411c7mD-p2"));
        assert_eq!(
            tracks[1].source_url.as_deref(),
            Some("https://www.bilibili.com/video/BV1xx411c7mD?p=2")
        );
        assert_eq!(
            (
                legacy.title.as_str(),
                legacy.album.as_str(),
                legacy.track_number
            ),
            ("合集标题", "Bilibili", None)
        );
        assert_eq!(
            legacy.source_url.as_deref(),
            Some("https://www.bilibili.com/video/BV1xx411c7mD")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bili05_video_info_lists_pages_and_the_requested_one() {
        let info = video_info(&three_pages(), Some(2));
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({
                "bvid": BVID,
                "title": "合集标题",
                "requestedPage": 2,
                "pages": [
                    { "page": 1, "title": "第一首", "duration": 180 },
                    { "page": 2, "title": "第二首", "duration": 200 },
                    { "page": 3, "title": "", "duration": 220 },
                ],
            })
        );
        let info = video_info(&video_with_one_page(), None);
        assert_eq!(info.pages.len(), 1);
        assert_eq!(
            serde_json::to_value(&info).unwrap()["requestedPage"],
            json!(null)
        );
    }

    #[test]
    fn bili05_multi_page_mode_wire_format() {
        let options: BilibiliImportOptions =
            serde_json::from_value(json!({ "multiPage": "all" })).unwrap();
        assert_eq!(options.multi_page, Some(MultiPageMode::All));
        let options: BilibiliImportOptions =
            serde_json::from_value(json!({ "multiPage": "first" })).unwrap();
        assert_eq!(options.multi_page, Some(MultiPageMode::First));
        // 旧前端不带这个字段：默认只导入首 P
        let options: BilibiliImportOptions = serde_json::from_value(json!({})).unwrap();
        assert_eq!(options.multi_page.unwrap_or_default(), MultiPageMode::First);
    }
}
