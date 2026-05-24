// =============================================================================
//  app/playlist/PlaylistIO.h
//
//  Playlist 序列化与反序列化。
//    - M3U / M3U8 (UTF-8, #EXTM3U + #EXTINF) — 通用,与其它播放器互通
//    - JSON (自写最小子集,无外部依赖) — 完整保留 PlaylistItem 全部字段
//
//  失败一律返回 false 并填 err。
// =============================================================================
#pragma once

#include "app/playlist/Playlist.h"

#include <string>

namespace apx {

class PlaylistIO {
public:
    // ---- M3U / M3U8 ----
    // 读 m3u8:相对路径基于 m3u 所在目录展开;#EXTINF:<sec>,<title> 解析出 title
    static bool loadM3U(const std::wstring& path, Playlist& out,
                        std::wstring* err = nullptr);
    static bool saveM3U(const Playlist& in, const std::wstring& path,
                        std::wstring* err = nullptr);

    // ---- JSON (单文件简化格式) ----
    // 结构:
    //   {"version":1,"mode":"Sequential","current":-1,"items":[
    //     {"path":"...","title":"...","artist":"...","album":"...",
    //      "track":N,"duration":F,"cue_start":F,"cue_end":F}, ...
    //   ]}
    // 字符串采用 UTF-8 标准转义。
    static bool loadJson(const std::wstring& path, Playlist& out,
                         std::wstring* err = nullptr);
    static bool saveJson(const Playlist& in, const std::wstring& path,
                         std::wstring* err = nullptr);
};

} // namespace apx
