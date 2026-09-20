//! 歌词排除规则（设置 → 歌词设置 → 歌词排除配置），Rust 侧编译与匹配。
//!
//! 前端只负责编辑规则列表并经 `set_lyrics_exclude_rules` 同步到这里；所有回传前端的
//! 歌词（`get_track_info` / 保存与应用歌词的返回值 / 在线候选预览）都经 `mark_hidden`
//! 打上 `LyricLine.hidden`，三处显示组件按标记隐藏。曲库缓存里**不存**该标记。
//!
//! 关键词按大小写不敏感子串匹配；正则用 `regex` crate（无环视/反向引用，编译期报错），
//! 用 `RegexBuilder::size_limit` 封顶避免灾难性膨胀。原文、译文、音译任一命中即隐藏整行。

use super::prelude::*;
use regex::RegexBuilder;
use std::sync::RwLock;

pub(crate) const MAX_EXCLUDE_RULES: usize = 50;
pub(crate) const MAX_EXCLUDE_PATTERN_CHARS: usize = 200;
/// 单条正则的编译后大小上限（regex crate 的程序字节数）。
const REGEX_SIZE_LIMIT: usize = 256 * 1024;

pub(crate) const LYRICS_RULES_UPDATED_EVENT: &str = "seraph://lyrics-rules-updated";

pub(crate) enum Compiled {
    Keyword(String),
    Regex(Regex),
}

static RULES: RwLock<Vec<Compiled>> = RwLock::new(Vec::new());

/// 编译单条规则；返回用户可读的错误。
pub(crate) fn compile_rule(rule: &LyricsExcludeRule) -> Result<Compiled, String> {
    let pattern = rule.pattern.trim();
    if pattern.is_empty() {
        return Err("规则内容为空".into());
    }
    if pattern.chars().count() > MAX_EXCLUDE_PATTERN_CHARS {
        return Err(format!("规则超过 {MAX_EXCLUDE_PATTERN_CHARS} 字符"));
    }
    match rule.kind.as_str() {
        "keyword" => Ok(Compiled::Keyword(pattern.to_lowercase())),
        "regex" => RegexBuilder::new(pattern)
            .case_insensitive(true)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()
            .map(Compiled::Regex)
            .map_err(|err| friendly_regex_error(&err)),
        other => Err(format!("未知规则类型：{other}")),
    }
}

fn friendly_regex_error(err: &regex::Error) -> String {
    let raw = err.to_string();
    // regex crate 对环视/反向引用的报错很长，前端提示只要一句
    if raw.contains("look-around") || raw.contains("backreference") {
        return "Rust 正则不支持环视（?=/?!/?<=/?<!）与反向引用（\\1）".into();
    }
    if raw.contains("exceeds size limit") {
        return "正则过于复杂（编译后超出大小上限）".into();
    }
    raw.lines()
        .find(|line| line.starts_with("error:"))
        .map(|line| line.trim_start_matches("error:").trim().to_string())
        .unwrap_or(raw)
}

/// 校验规则；前端逐条拿结果，全部无误才允许保存。
pub(crate) fn validate_rules(rules: &[LyricsExcludeRule]) -> Vec<LyricsExcludeRuleStatus> {
    rules
        .iter()
        .map(|rule| LyricsExcludeRuleStatus {
            id: rule.id.clone(),
            error: compile_rule(rule).err(),
        })
        .collect()
}

/// 替换当前生效规则；坏规则跳过（前端已阻止保存坏规则，这里是兜底）。返回逐条状态。
pub(crate) fn replace_rules(rules: &[LyricsExcludeRule]) -> Vec<LyricsExcludeRuleStatus> {
    let mut compiled = Vec::new();
    let mut statuses = Vec::new();
    for rule in rules.iter().take(MAX_EXCLUDE_RULES) {
        match compile_rule(rule) {
            Ok(item) => {
                compiled.push(item);
                statuses.push(LyricsExcludeRuleStatus {
                    id: rule.id.clone(),
                    error: None,
                });
            }
            Err(error) => statuses.push(LyricsExcludeRuleStatus {
                id: rule.id.clone(),
                error: Some(error),
            }),
        }
    }
    *RULES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = compiled;
    statuses
}

fn matches_any(compiled: &[Compiled], value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let lower = value.to_lowercase();
    compiled.iter().any(|rule| match rule {
        Compiled::Keyword(keyword) => lower.contains(keyword),
        Compiled::Regex(regex) => regex.is_match(value),
    })
}

/// 按当前规则给歌词打 `hidden` 标记（原地）。无规则时不动，O(1) 返回。
/// 回传前的完整投影（含制作信息隐藏与 offset 还原）见 `display::project_document`。
pub(crate) fn mark_hidden(lyrics: &mut [LyricLine]) {
    let compiled = RULES
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if compiled.is_empty() {
        for line in lyrics.iter_mut() {
            line.hidden = false;
        }
        return;
    }
    for line in lyrics.iter_mut() {
        line.hidden = matches_any(&compiled, &line.text)
            || line
                .translations
                .iter()
                .any(|value| matches_any(&compiled, &value.text))
            || line
                .roman_text()
                .is_some_and(|value| matches_any(&compiled, value));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    // 规则与显示选项都是进程级全局，涉及它们的测试串行以免互相干扰
    static SERIAL: Mutex<()> = Mutex::new(());

    pub(crate) fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn rule(kind: &str, pattern: &str) -> LyricsExcludeRule {
        LyricsExcludeRule {
            id: format!("{kind}:{pattern}"),
            kind: kind.into(),
            pattern: pattern.into(),
        }
    }

    #[test]
    fn keyword_is_case_insensitive_and_regex_uses_rust_syntax() {
        let _guard = serial_guard();
        let statuses = replace_rules(&[
            rule("keyword", "HELLO"),
            rule("regex", r"^(作词|作曲)\s*[:：]"),
        ]);
        assert!(statuses.iter().all(|s| s.error.is_none()));

        let mut lines = vec![
            LyricLine::new(0, "作词：某人"),
            LyricLine {
                translations: vec![LyricText::new("say hello")],
                ..LyricLine::new(1000, "第二句")
            },
            LyricLine {
                roman: Some(LyricText::new("ta ci")),
                ..LyricLine::new(2000, "第三句")
            },
        ];
        mark_hidden(&mut lines);
        assert_eq!(
            lines.iter().map(|l| l.hidden).collect::<Vec<_>>(),
            [true, true, false]
        );

        replace_rules(&[]);
        mark_hidden(&mut lines);
        assert!(lines.iter().all(|l| !l.hidden));
    }

    #[test]
    fn validation_rejects_lookaround_backrefs_and_empty() {
        let _guard = serial_guard();
        let statuses = validate_rules(&[
            rule("regex", "(?=x)"),
            rule("regex", r"(a)\1"),
            rule("regex", "("),
            rule("keyword", "   "),
            rule("nope", "x"),
            rule("regex", "^ok$"),
        ]);
        let errors = statuses
            .iter()
            .map(|s| s.error.is_some())
            .collect::<Vec<_>>();
        assert_eq!(errors, [true, true, true, true, true, false]);
        assert!(statuses[0].error.as_deref().unwrap().contains("环视"));
    }

    #[test]
    fn bad_rules_are_skipped_not_fatal() {
        let _guard = serial_guard();
        let statuses = replace_rules(&[rule("regex", "("), rule("keyword", "跳过我")]);
        assert!(statuses[0].error.is_some() && statuses[1].error.is_none());
        let mut lines = vec![LyricLine::new(0, "请跳过我"), LyricLine::new(1000, "留下")];
        mark_hidden(&mut lines);
        assert_eq!(
            lines.iter().map(|l| l.hidden).collect::<Vec<_>>(),
            [true, false]
        );
        replace_rules(&[]);
    }
}
