// =============================================================================
//  core/lyrics/LyricsLoader.h
//
//  解析 .lrc 歌词文件。同目录同名 .lrc 自动加载,大小写不敏感,
//  另外尝试 <音频目录>/lyrics/<同名>.lrc 作为第二候选。
//
//  支持:
//    - 多时间戳一行                  [00:12.30][01:45.20]text
//    - 元数据行                      [ti:..] [ar:..] [al:..] [by:..] [length:..]
//    - 全局时间偏移                  [offset:+250]    正值 = 歌词显示提前 (time -= offset/1000)
//    - 同一时间戳出现多次的"翻译副行"  第二次起的文本进入 translation
//    - 增强 LRC 词级时间戳            text<00:12.50>word1<00:12.80>word2
//    - 编码自动判别                  UTF-8 BOM / UTF-16 LE BOM / UTF-16 BE BOM
//                                  / 启发式 UTF-8 / 回退 GBK
// =============================================================================
#pragma once

#include <cstdint>
#include <string>
#include <utility>
#include <vector>

namespace apx {

struct LyricLine {
    double       time_sec    = 0.0;
    std::wstring text;                                  // 主文本(去掉词级标签)
    std::wstring translation;                           // 同 ts 第二次出现的文本
    // 词级 (time_sec, 累计字符数) — 卡拉OK 高亮用,空表示无
    std::vector<std::pair<double, int>> word_times;
};

struct LyricMetadata {
    std::wstring title;        // [ti:..]
    std::wstring artist;       // [ar:..]
    std::wstring album;        // [al:..]
    std::wstring by;           // [by:..]
    double       offset_ms = 0.0;   // [offset:..] (原始值,正负保留)
    int          length_sec = 0;    // [length:mm:ss]
};

struct LyricsDoc {
    LyricMetadata          meta;
    std::vector<LyricLine> lines;
    bool empty() const { return lines.empty(); }
};

class LyricsLoader {
public:
    // 给音频路径,自动找 .lrc;失败返回空 doc
    static LyricsDoc loadDocFor(const std::wstring& audio_path);
    // 给 lrc 路径
    static LyricsDoc loadDoc(const std::wstring& lrc_path);
    // 从内存字节流解析(用于 ID3v2 USLT 嵌入歌词)
    static LyricsDoc parseDoc(const std::string& raw_bytes);

    // 兼容旧 API:返回 std::vector<LyricLine> (已应用 offset)
    static std::vector<LyricLine> loadFor(const std::wstring& audio_path);
    static std::vector<LyricLine> load(const std::wstring& lrc_path);
};

} // namespace apx
