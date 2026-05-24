// =============================================================================
//  app/playlist/CueSheet.h
//
//  最小可用 CUE Sheet 解析器。
//
//  典型 CUE 内容:
//      TITLE "Album"
//      PERFORMER "Artist"
//      FILE "album.flac" WAVE
//        TRACK 01 AUDIO
//          TITLE "Song A"
//          PERFORMER "Artist"
//          INDEX 01 00:00:00
//        TRACK 02 AUDIO
//          TITLE "Song B"
//          INDEX 01 03:25:00            ; mm:ss:ff (75 frame/秒)
//
//  本实现:
//    - 只解析 FILE / TRACK n AUDIO / TITLE / PERFORMER / INDEX 01 (其它忽略)
//    - 多 FILE 也支持(罕见但合法)
//    - INDEX 时间码 mm:ss:ff (75ths/秒) 解析为秒;ff 自动除 75
//    - REM 行整体忽略 (含 REM REPLAYGAIN_TRACK_GAIN 等,留给 ReplayGain 模块单独处理)
//
//  失败模式:文件读不到 / 无 FILE 行 / 无 TRACK 行 → 返回空 vector。
// =============================================================================
#pragma once

#include "app/playlist/Playlist.h"

#include <string>
#include <vector>

namespace apx {

class CueSheet {
public:
    // 把 cue_path 解析为一组 PlaylistItem (每个 TRACK 一项)。
    // base_dir 用于把 FILE 相对路径拼成绝对路径;通常传 cue_path 所在目录。
    // 解析失败返回空 vector;err 可选输出错误信息。
    static std::vector<PlaylistItem> parse(const std::wstring& cue_path,
                                           std::wstring* err = nullptr);

    // 把 mm:ss:ff (75ths/秒) 时间码转成秒。无效返回 -1。
    static double parseTimecode(const std::string& tc);
};

} // namespace apx
