//! 歌词**显示层投影**（回传前端前的最后一步）。
//!
//! 与排除规则同一机制：前端 store 持有设置（持久化、进配置导出），经
//! `set_lyrics_display_options` 同步到这里的进程级 `RwLock`，后端只在**回传前**投影，曲库
//! 缓存里存的永远是解析原样。这样主窗口三处显示与任务栏歌词条（无 store、全走 IPC）天然一致，
//! 不必再给第二窗口同步任何设置。设置变更广播 `LYRICS_RULES_UPDATED_EVENT`，两个窗口重拉。
//!
//! 投影三件事：
//! 1. 排除规则打 `hidden`（`exclude::mark_hidden`）；
//! 2. 「显示制作信息」关闭时把 `role == Credit` 的行也打 `hidden`——前端与任务栏据同一标记隐藏；
//! 3. 「忽略歌词文件里的 offset」开启时把解析时折进去的 `offset_ms` 还原到各行与音节
//!    （L-9：解析是 `start = raw − offset`，还原即 `+ offset`，负值钳到 0）。文档的 `offset_ms`
//!    字段保持原值，前端可据此提示「文件 offset 已忽略」。

use super::prelude::*;
use std::sync::RwLock;

static OPTIONS: RwLock<LyricsDisplayOptions> = RwLock::new(LyricsDisplayOptions::DEFAULT);

/// 替换当前生效的显示选项；返回是否有变化（无变化时调用方不必广播重拉）。
pub(crate) fn set_display_options(options: LyricsDisplayOptions) -> bool {
    let mut guard = OPTIONS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let changed = *guard != options;
    *guard = options;
    changed
}

pub(crate) fn display_options() -> LyricsDisplayOptions {
    *OPTIONS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 按当前显示选项投影整份文档（原地）：打标 + 制作信息隐藏 + offset 还原。
pub(crate) fn project_document(document: &mut LyricDocument) {
    project_document_with(document, display_options());
}

/// 投影后的副本（命令返回值常用）。
pub(crate) fn projected(mut document: LyricDocument) -> LyricDocument {
    project_document(&mut document);
    document
}

pub(crate) fn project_document_with(document: &mut LyricDocument, options: LyricsDisplayOptions) {
    mark_hidden(&mut document.lines);
    if !options.show_credits {
        for line in &mut document.lines {
            if line.role == LyricRole::Credit {
                line.hidden = true;
            }
        }
    }
    if options.ignore_file_offset && document.offset_ms != 0 {
        shift_lines(&mut document.lines, i64::from(document.offset_ms));
    }
}

/// 全部行 / 音节 / 副轨音节的时间整体平移（毫秒，可负；结果不小于 0）。
fn shift_lines(lines: &mut [LyricLine], delta_ms: i64) {
    let shift = |ms: u64| -> u64 {
        i64::try_from(ms)
            .unwrap_or(i64::MAX)
            .saturating_add(delta_ms)
            .max(0) as u64
    };
    let shift_words = |words: &mut Vec<LyricWord>| {
        for word in words {
            word.start_ms = shift(word.start_ms);
            word.end_ms = word.end_ms.map(shift);
        }
    };
    for line in lines {
        line.start_ms = shift(line.start_ms);
        line.end_ms = line.end_ms.map(shift);
        if let Some(words) = line.words.as_mut() {
            shift_words(words);
        }
        for translation in &mut line.translations {
            if let Some(words) = translation.words.as_mut() {
                shift_words(words);
            }
        }
        if let Some(words) = line.roman.as_mut().and_then(|roman| roman.words.as_mut()) {
            shift_words(words);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LyricDocument {
        let mut credit = LyricLine::new(0, "作词：某人");
        credit.role = LyricRole::Credit;
        let mut sung = LyricLine::new(804, "我的天");
        sung.end_ms = Some(2000);
        sung.words = Some(vec![
            LyricWord::new(804, Some(1000), "我"),
            LyricWord::new(1000, None, "的天"),
        ]);
        let mut early = LyricLine::new(100, "提前");
        early.end_ms = Some(300);
        // 解析时按 offset:-196 折过：raw 1000 → 804
        LyricDocument::from_lines(vec![credit, early, sung], LyricSource::default())
            .with_offset(-196)
    }

    #[test]
    fn credits_hidden_only_when_option_is_off() {
        let mut shown = sample();
        project_document_with(&mut shown, LyricsDisplayOptions::DEFAULT);
        assert!(shown.lines.iter().all(|line| !line.hidden));

        let mut hidden = sample();
        project_document_with(
            &mut hidden,
            LyricsDisplayOptions {
                show_credits: false,
                ..LyricsDisplayOptions::DEFAULT
            },
        );
        assert_eq!(
            hidden
                .lines
                .iter()
                .map(|line| line.hidden)
                .collect::<Vec<_>>(),
            [true, false, false]
        );
        // 时间不动
        assert_eq!(hidden.lines[2].start_ms, 804);
    }

    #[test]
    fn ignoring_file_offset_restores_raw_times_and_clamps_at_zero() {
        let mut doc = sample();
        project_document_with(
            &mut doc,
            LyricsDisplayOptions {
                ignore_file_offset: true,
                ..LyricsDisplayOptions::DEFAULT
            },
        );
        // offset −196 折进去时是 raw + 196，忽略即 − 196
        let sung = &doc.lines[2];
        assert_eq!((sung.start_ms, sung.end_ms), (608, Some(1804)));
        let words = sung.words.as_ref().unwrap();
        assert_eq!((words[0].start_ms, words[0].end_ms), (608, Some(804)));
        assert_eq!((words[1].start_ms, words[1].end_ms), (804, None));
        // 负到 0 以下钳住
        assert_eq!((doc.lines[1].start_ms, doc.lines[1].end_ms), (0, Some(104)));
        assert_eq!(doc.lines[0].start_ms, 0);
        // 文档字段保留原值，前端可提示
        assert_eq!(doc.offset_ms, -196);

        // offset 为 0 的文档（在线来源）不受影响
        let mut plain =
            LyricDocument::from_lines(vec![LyricLine::new(500, "a")], LyricSource::default());
        project_document_with(
            &mut plain,
            LyricsDisplayOptions {
                ignore_file_offset: true,
                ..LyricsDisplayOptions::DEFAULT
            },
        );
        assert_eq!(plain.lines[0].start_ms, 500);
    }

    #[test]
    fn set_display_options_reports_change() {
        let _guard = super::super::exclude::tests::serial_guard();
        let initial = display_options();
        let flipped = LyricsDisplayOptions {
            ignore_file_offset: !initial.ignore_file_offset,
            ..initial
        };
        assert!(set_display_options(flipped));
        assert!(!set_display_options(flipped));
        assert_eq!(display_options(), flipped);
        assert!(set_display_options(initial));
    }
}
