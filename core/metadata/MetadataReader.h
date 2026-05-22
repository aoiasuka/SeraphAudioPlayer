// =============================================================================
//  core/metadata/MetadataReader.h
//
//  轻量元数据读取器(不依赖 TagLib)。
//
//  支持:
//    - WAV 的 LIST/INFO chunk (INAM/IART/IPRD/ICRD/ITRK)
//    - FLAC 的 VORBIS_COMMENT block (TITLE/ARTIST/ALBUM/DATE/TRACKNUMBER)
//
//  接口同步,适合在 UI 线程偶尔调用(单文件 <1ms);
//  大量调用应在调用方做缓存。
// =============================================================================
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace apx {

struct TrackMetadata {
    std::wstring title;
    std::wstring artist;
    std::wstring album;
    std::wstring date;       // 年份字符串
    int          track_no = 0;
    double       duration_sec = 0.0;  // 时长(秒);未知为 0
    bool         has_cover = false;   // 是否含内嵌封面(细节通过 readCover 单独取)
};

// 单独的封面数据(可能很大,几百 KB ~ 几 MB),不放进 TrackMetadata
struct CoverImage {
    std::vector<uint8_t> data;
    std::string mime;        // e.g. "image/jpeg" / "image/png"
};

class MetadataReader {
public:
    // 读取文件元数据(不含封面二进制);失败返回 nullopt。后缀大小写不敏感。
    static std::optional<TrackMetadata> read(const std::wstring& path);

    // 单独读取封面二进制。如 has_cover=false 或不存在则返回 nullopt。
    static std::optional<CoverImage> readCover(const std::wstring& path);

private:
    static std::optional<TrackMetadata> readWav(const std::wstring& path);
    static std::optional<TrackMetadata> readFlac(const std::wstring& path);
    static std::optional<CoverImage>    readFlacCover(const std::wstring& path);
};

} // namespace apx
