// =============================================================================
//  core/lyrics/LyricsLoader.h
//
//  解析 .lrc 歌词文件。同目录同名 .lrc 文件自动加载。
//
//  支持:
//    - 多时间戳一行 (e.g. [00:12.30][01:45.20]xxx)
//    - 元数据行 [ti:..] / [ar:..] / [al:..] (忽略)
//    - UTF-8 / GBK 自动判别 (GBK 用 Win32 MultiByteToWideChar)
// =============================================================================
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace apx {

struct LyricLine {
    double       time_sec = 0.0;
    std::wstring text;
};

class LyricsLoader {
public:
    // 给定音频路径,自动在同目录找同名 .lrc;失败返回空 vector
    static std::vector<LyricLine> loadFor(const std::wstring& audio_path);

    // 直接给 lrc 路径
    static std::vector<LyricLine> load(const std::wstring& lrc_path);
};

} // namespace apx
