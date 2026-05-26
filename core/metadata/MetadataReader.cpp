// =============================================================================
//  core/metadata/MetadataReader.cpp
// =============================================================================
#include "MetadataReader.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <algorithm>
#include <vector>

namespace apx {

namespace {

// ---- UTF-8 -> wstring (BMP 范围;FLAC tag 实际罕有 BMP 外字符) ----
std::wstring utf8ToW(const std::string& s)
{
    std::wstring out;
    out.reserve(s.size());
    size_t i = 0;
    while (i < s.size()) {
        unsigned char c = static_cast<unsigned char>(s[i]);
        uint32_t cp = 0;
        int extra = 0;
        if (c < 0x80) { cp = c; extra = 0; }
        else if ((c & 0xE0) == 0xC0) { cp = c & 0x1F; extra = 1; }
        else if ((c & 0xF0) == 0xE0) { cp = c & 0x0F; extra = 2; }
        else if ((c & 0xF8) == 0xF0) { cp = c & 0x07; extra = 3; }
        else { ++i; continue; } // 非法字节,跳过
        ++i;
        bool ok = true;
        for (int k = 0; k < extra; ++k) {
            if (i >= s.size()) { ok = false; break; }
            unsigned char cc = static_cast<unsigned char>(s[i++]);
            if ((cc & 0xC0) != 0x80) { ok = false; break; }
            cp = (cp << 6) | (cc & 0x3F);
        }
        if (!ok) continue;
        if (cp < 0x10000) {
            out.push_back(static_cast<wchar_t>(cp));
        } else {
            // surrogate pair (Windows wchar_t = 16-bit)
            cp -= 0x10000;
            out.push_back(static_cast<wchar_t>(0xD800 | (cp >> 10)));
            out.push_back(static_cast<wchar_t>(0xDC00 | (cp & 0x3FF)));
        }
    }
    return out;
}

// Windows-1252 fallback(常见于老 WAV 的 LIST/INFO)
std::wstring cp1252ToW(const std::string& s)
{
    // 简化:CP1252 在 0x80-0x9F 有特殊映射,这里仅做 Latin-1 近似
    std::wstring out;
    out.reserve(s.size());
    for (unsigned char c : s) {
        if (c >= 0x20 && c < 0x80) out.push_back(static_cast<wchar_t>(c));
        else if (c >= 0xA0)        out.push_back(static_cast<wchar_t>(c));
        else if (c == 0x09 || c == 0x0A || c == 0x0D) out.push_back(static_cast<wchar_t>(c));
        // 其他控制字符忽略
    }
    return out;
}

// 启发式:看起来像 UTF-8 则按 UTF-8,否则按 CP1252
bool looksLikeUtf8(const std::string& s)
{
    int hint = 0;
    for (size_t i = 0; i < s.size(); ) {
        unsigned char c = static_cast<unsigned char>(s[i]);
        if (c < 0x80) { ++i; continue; }
        int extra = 0;
        if      ((c & 0xE0) == 0xC0) extra = 1;
        else if ((c & 0xF0) == 0xE0) extra = 2;
        else if ((c & 0xF8) == 0xF0) extra = 3;
        else return false;
        if (i + extra >= s.size()) return false;
        for (int k = 1; k <= extra; ++k) {
            if ((static_cast<unsigned char>(s[i + k]) & 0xC0) != 0x80) return false;
        }
        i += extra + 1;
        ++hint;
    }
    return hint > 0; // 完全 ASCII 时直接走 UTF-8 路径也无碍
}

std::wstring decodeText(const std::string& s)
{
    if (s.empty()) return {};
    if (looksLikeUtf8(s)) return utf8ToW(s);
    return cp1252ToW(s);
}

void trimNulAndSpace(std::string& s)
{
    while (!s.empty() && (s.back() == '\0' || s.back() == ' ' || s.back() == '\r' || s.back() == '\n')) s.pop_back();
}

uint32_t readU32LE(const uint8_t* p) {
    return  uint32_t(p[0])        |
           (uint32_t(p[1]) << 8)  |
           (uint32_t(p[2]) << 16) |
           (uint32_t(p[3]) << 24);
}

uint32_t readU32BE(const uint8_t* p) {
    return (uint32_t(p[0]) << 24) |
           (uint32_t(p[1]) << 16) |
           (uint32_t(p[2]) << 8)  |
            uint32_t(p[3]);
}

// 读取受限大小的字段,失败返回 false
bool fread_n(FILE* f, void* buf, size_t n)
{
    return std::fread(buf, 1, n, f) == n;
}

std::wstring lower(const std::wstring& s)
{
    std::wstring r = s;
    std::transform(r.begin(), r.end(), r.begin(), [](wchar_t c) {
        return (c >= L'A' && c <= L'Z') ? wchar_t(c - L'A' + L'a') : c;
    });
    return r;
}

bool endsWith(const std::wstring& s, const std::wstring& suf)
{
    if (s.size() < suf.size()) return false;
    return lower(s.substr(s.size() - suf.size())) == suf;
}

// wstring(UTF-16) -> UTF-8 字节流
std::string wToUtf8(const std::wstring& w)
{
    std::string out;
    out.reserve(w.size());
    size_t i = 0;
    while (i < w.size()) {
        uint32_t cp = static_cast<uint32_t>(static_cast<uint16_t>(w[i]));
        ++i;
        if (cp >= 0xD800 && cp <= 0xDBFF && i < w.size()) {
            uint32_t lo = static_cast<uint32_t>(static_cast<uint16_t>(w[i]));
            if (lo >= 0xDC00 && lo <= 0xDFFF) {
                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                ++i;
            }
        }
        if (cp < 0x80) {
            out.push_back(static_cast<char>(cp));
        } else if (cp < 0x800) {
            out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else if (cp < 0x10000) {
            out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else {
            out.push_back(static_cast<char>(0xF0 | (cp >> 18)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        }
    }
    return out;
}

} // namespace

// ============================================================================
//  WAV LIST/INFO + 时长
// ============================================================================
std::optional<TrackMetadata> MetadataReader::readWav(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return std::nullopt;

    auto close = [&]() { if (f) { std::fclose(f); f = nullptr; } };

    uint8_t hdr[12];
    if (!fread_n(f, hdr, 12)) { close(); return std::nullopt; }
    if (std::memcmp(hdr, "RIFF", 4) != 0 || std::memcmp(hdr + 8, "WAVE", 4) != 0) {
        close(); return std::nullopt;
    }

    TrackMetadata md;
    bool found = false;

    uint32_t byteRate = 0;
    uint32_t dataSize = 0;

    // 限制扫描范围,防止异常文件占用过久(8MB 已远超合理 RIFF 元数据)
    constexpr long kMaxScan = 8 * 1024 * 1024;
    long scanned = 12;

    while (scanned < kMaxScan) {
        uint8_t ck[8];
        if (!fread_n(f, ck, 8)) break;
        uint32_t sz = readU32LE(ck + 4);
        scanned += 8;

        if (std::memcmp(ck, "fmt ", 4) == 0) {
            // fmt chunk: 至少 16 字节
            //   2 wFormatTag, 2 nChannels, 4 nSamplesPerSec, 4 nAvgBytesPerSec, 2 nBlockAlign, 2 wBitsPerSample
            std::vector<uint8_t> fmt(std::min<uint32_t>(sz, 40));
            if (std::fread(fmt.data(), 1, fmt.size(), f) != fmt.size()) { break; }
            if (fmt.size() >= 16) {
                byteRate = readU32LE(fmt.data() + 8);
            }
            uint32_t remain = sz - static_cast<uint32_t>(fmt.size());
            if (remain > 0) std::fseek(f, remain, SEEK_CUR);
            scanned += sz;
            if (sz & 1u) { std::fseek(f, 1, SEEK_CUR); ++scanned; }
        } else if (std::memcmp(ck, "data", 4) == 0) {
            dataSize = sz;
            // 不读 data 内容
            std::fseek(f, sz, SEEK_CUR);
            scanned += sz;
            if (sz & 1u) { std::fseek(f, 1, SEEK_CUR); ++scanned; }
        } else if (std::memcmp(ck, "LIST", 4) == 0) {
            // LIST <size> "INFO" <subchunks>
            if (sz < 4) { std::fseek(f, sz, SEEK_CUR); scanned += sz; continue; }
            char tag[4];
            if (!fread_n(f, tag, 4)) break;
            scanned += 4;
            uint32_t remain = sz - 4;
            if (std::memcmp(tag, "INFO", 4) != 0) {
                std::fseek(f, remain, SEEK_CUR);
                scanned += remain;
                continue;
            }
            // 解析 INFO 子块
            while (remain >= 8) {
                uint8_t sc[8];
                if (!fread_n(f, sc, 8)) { remain = 0; break; }
                uint32_t ssz = readU32LE(sc + 4);
                remain -= 8;
                scanned += 8;
                if (ssz > remain) ssz = remain;
                std::string text(ssz, '\0');
                if (ssz > 0) {
                    if (std::fread(text.data(), 1, ssz, f) != ssz) { remain = 0; break; }
                    remain -= ssz;
                    scanned += ssz;
                }
                // 字段值用 NUL 结束,可能含 padding
                trimNulAndSpace(text);
                std::wstring w = decodeText(text);

                if      (std::memcmp(sc, "INAM", 4) == 0) { md.title  = w; found = true; }
                else if (std::memcmp(sc, "IART", 4) == 0) { md.artist = w; found = true; }
                else if (std::memcmp(sc, "IPRD", 4) == 0) { md.album  = w; found = true; }
                else if (std::memcmp(sc, "ICRD", 4) == 0) { md.date   = w; found = true; }
                else if (std::memcmp(sc, "ITRK", 4) == 0) {
                    try { md.track_no = std::stoi(text); found = true; } catch (...) {}
                }
                // chunk size 偶数对齐
                if (ssz & 1u) {
                    if (remain > 0) { std::fseek(f, 1, SEEK_CUR); --remain; ++scanned; }
                }
            }
            if (remain > 0) { std::fseek(f, remain, SEEK_CUR); scanned += remain; }
        } else {
            // 跳过其他 chunk(data/fmt 等)
            std::fseek(f, sz, SEEK_CUR);
            scanned += sz;
            if (sz & 1u) { std::fseek(f, 1, SEEK_CUR); ++scanned; }
        }
    }

    close();

    if (byteRate > 0 && dataSize > 0) {
        md.duration_sec = static_cast<double>(dataSize) / static_cast<double>(byteRate);
        found = true;
    }

    if (!found) return std::nullopt;
    return md;
}

// ============================================================================
//  FLAC VORBIS_COMMENT + STREAMINFO 时长
// ============================================================================
std::optional<TrackMetadata> MetadataReader::readFlac(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return std::nullopt;
    auto close = [&]() { if (f) { std::fclose(f); f = nullptr; } };

    uint8_t marker[4];
    if (!fread_n(f, marker, 4)) { close(); return std::nullopt; }
    if (std::memcmp(marker, "fLaC", 4) != 0) { close(); return std::nullopt; }

    TrackMetadata md;
    bool found = false;
    bool last = false;
    bool seenComment = false;

    // 限制扫描:最多 32 个 metadata block
    int blocks = 0;
    while (!last && blocks++ < 32) {
        uint8_t bh[4];
        if (!fread_n(f, bh, 4)) break;
        last = (bh[0] & 0x80) != 0;
        int type = bh[0] & 0x7F;
        uint32_t len = (uint32_t(bh[1]) << 16) | (uint32_t(bh[2]) << 8) | uint32_t(bh[3]);

        if (type == 0) {
            // STREAMINFO (34 bytes)
            std::vector<uint8_t> buf(len);
            if (len > 0 && std::fread(buf.data(), 1, len, f) != len) break;
            if (buf.size() >= 18) {
                uint64_t big8 = 0;
                for (int i = 0; i < 8; ++i) big8 = (big8 << 8) | buf[10 + i];
                uint32_t sample_rate = static_cast<uint32_t>((big8 >> 44) & 0xFFFFFu);
                uint64_t total_samples = big8 & ((1ULL << 36) - 1ULL);
                if (sample_rate > 0 && total_samples > 0) {
                    md.duration_sec = static_cast<double>(total_samples) / static_cast<double>(sample_rate);
                    found = true;
                }
            }
        } else if (type == 6) {
            // PICTURE — 标记 has_cover,不读二进制(由 readFlacCover 单独读)
            md.has_cover = true;
            found = true;
            std::fseek(f, len, SEEK_CUR);
        } else if (type == 4) {
            // VORBIS_COMMENT
            std::vector<uint8_t> buf(len);
            if (len > 0 && std::fread(buf.data(), 1, len, f) != len) break;
            // vendor
            if (buf.size() < 4) continue;
            uint32_t vlen = readU32LE(buf.data());
            size_t off = 4;
            if (off + vlen + 4 > buf.size()) continue;
            off += vlen;
            uint32_t n = readU32LE(buf.data() + off);
            off += 4;
            for (uint32_t i = 0; i < n; ++i) {
                if (off + 4 > buf.size()) { off = buf.size(); break; }
                uint32_t clen = readU32LE(buf.data() + off);
                off += 4;
                if (off + clen > buf.size()) break;
                std::string entry(reinterpret_cast<const char*>(buf.data() + off), clen);
                off += clen;
                // "KEY=value" (UTF-8)
                auto eq = entry.find('=');
                if (eq == std::string::npos) continue;
                std::string key = entry.substr(0, eq);
                std::string val = entry.substr(eq + 1);
                // key 大写规范化
                std::transform(key.begin(), key.end(), key.begin(),
                               [](unsigned char c){ return static_cast<char>(std::toupper(c)); });
                std::wstring w = utf8ToW(val);
                if      (key == "TITLE")        { md.title  = w; found = true; seenComment = true; }
                else if (key == "ARTIST")       { md.artist = w; found = true; seenComment = true; }
                else if (key == "ALBUM")        { md.album  = w; found = true; seenComment = true; }
                else if (key == "DATE" || key == "YEAR") { md.date = w; found = true; seenComment = true; }
                else if (key == "TRACKNUMBER")  {
                    try { md.track_no = std::stoi(val); found = true; seenComment = true; } catch (...) {}
                }
                else if (key == "REPLAYGAIN_TRACK_GAIN") {
                    // 形如 "-6.55 dB" / "-6.55dB" / "-6.55"
                    try { md.rg_track_gain_db = std::stod(val); found = true; } catch (...) {}
                }
                else if (key == "REPLAYGAIN_TRACK_PEAK") {
                    try { md.rg_track_peak = std::stod(val); found = true; } catch (...) {}
                }
                else if (key == "REPLAYGAIN_ALBUM_GAIN") {
                    try { md.rg_album_gain_db = std::stod(val); found = true; } catch (...) {}
                }
                else if (key == "REPLAYGAIN_ALBUM_PEAK") {
                    try { md.rg_album_peak = std::stod(val); found = true; } catch (...) {}
                }
            }
        } else {
            // 跳过其他 block
            std::fseek(f, len, SEEK_CUR);
        }
    }

    close();
    (void)seenComment;
    if (!found) return std::nullopt;
    return md;
}

// ============================================================================
//  MP3 — ID3v2 (TXXX:REPLAYGAIN_* / TIT2 / TPE1 / TALB / TYER / TDRC / TRCK)
// ============================================================================
//
//  ID3v2 头(10 字节):
//    'I' 'D' '3' <ver_hi> <ver_lo> <flags> <size:4 sync-safe>
//  size 是 "synchsafe" 7-bit-per-byte 编码,实际 = byte0<<21 | byte1<<14 | byte2<<7 | byte3
//
//  各 frame:
//    ID(4) Size(4) Flags(2) [encoding(1)] payload...
//    ID3v2.4 frame size 是 synchsafe;ID3v2.3 是普通 big-endian。
//    encoding: 0=ISO-8859-1, 1=UTF-16 BOM, 2=UTF-16BE, 3=UTF-8(只在 v2.4)
//
//  TXXX frame 是 "用户自定义文本":payload = enc(1) + description(NUL) + value
//    description 例:"REPLAYGAIN_TRACK_GAIN" → value "-6.55 dB"
std::optional<TrackMetadata> MetadataReader::readMp3(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return std::nullopt;
    auto close = [&]() { if (f) { std::fclose(f); f = nullptr; } };

    uint8_t hdr[10];
    if (!fread_n(f, hdr, 10)) { close(); return std::nullopt; }
    if (std::memcmp(hdr, "ID3", 3) != 0) { close(); return std::nullopt; }

    const int ver_major = hdr[3];      // 2/3/4
    // const int ver_minor = hdr[4];
    const uint8_t flags = hdr[5];
    // synchsafe size
    const uint32_t total =
        (uint32_t(hdr[6] & 0x7F) << 21) |
        (uint32_t(hdr[7] & 0x7F) << 14) |
        (uint32_t(hdr[8] & 0x7F) << 7)  |
        (uint32_t(hdr[9] & 0x7F));
    if (total == 0 || total > 64 * 1024 * 1024) { close(); return std::nullopt; }

    std::vector<uint8_t> tag(total);
    if (std::fread(tag.data(), 1, total, f) != total) { close(); return std::nullopt; }
    close();

    // 跳过 extended header (v2.3/v2.4)
    size_t pos = 0;
    if (flags & 0x40) {
        if (pos + 4 > tag.size()) return std::nullopt;
        uint32_t ext;
        if (ver_major >= 4) {
            ext = (uint32_t(tag[pos]&0x7F)<<21) | (uint32_t(tag[pos+1]&0x7F)<<14)
                | (uint32_t(tag[pos+2]&0x7F)<<7)  |  uint32_t(tag[pos+3]&0x7F);
        } else {
            ext = (uint32_t(tag[pos])<<24)|(uint32_t(tag[pos+1])<<16)
                | (uint32_t(tag[pos+2])<<8) | uint32_t(tag[pos+3]);
        }
        pos += ext;
        if (pos >= tag.size()) return std::nullopt;
    }

    auto decode_text = [](const uint8_t* p, size_t n, uint8_t enc) -> std::wstring {
        if (n == 0) return {};
        std::string s(reinterpret_cast<const char*>(p), n);
        // 去掉尾 NUL
        while (!s.empty() && s.back() == '\0') s.pop_back();
        switch (enc) {
        case 0: return cp1252ToW(s);    // ISO-8859-1 ≈ CP1252
        case 3: return utf8ToW(s);      // UTF-8 (only v2.4)
        case 1: {
            // UTF-16 with BOM
            if (s.size() < 2) return {};
            const uint8_t* b = reinterpret_cast<const uint8_t*>(s.data());
            bool be = false;
            size_t start = 0;
            if (b[0] == 0xFF && b[1] == 0xFE)      { be = false; start = 2; }
            else if (b[0] == 0xFE && b[1] == 0xFF) { be = true;  start = 2; }
            std::wstring w;
            for (size_t i = start; i + 1 < s.size(); i += 2) {
                uint16_t u = be ? (uint16_t(b[i])<<8) | b[i+1]
                                : (uint16_t(b[i+1])<<8) | b[i];
                w.push_back(static_cast<wchar_t>(u));
            }
            // 去尾
            while (!w.empty() && w.back() == 0) w.pop_back();
            return w;
        }
        case 2: {
            // UTF-16BE no BOM
            std::wstring w;
            for (size_t i = 0; i + 1 < s.size(); i += 2) {
                uint16_t u = (uint16_t(static_cast<unsigned char>(s[i]))<<8)
                           | static_cast<unsigned char>(s[i+1]);
                w.push_back(static_cast<wchar_t>(u));
            }
            while (!w.empty() && w.back() == 0) w.pop_back();
            return w;
        }
        }
        return {};
    };

    TrackMetadata md;
    bool found = false;

    while (pos + 10 <= tag.size()) {
        char fid[5] = {0};
        std::memcpy(fid, tag.data() + pos, 4);
        if (fid[0] == 0) break;     // padding
        uint32_t fsize;
        if (ver_major >= 4) {
            fsize = (uint32_t(tag[pos+4]&0x7F)<<21) | (uint32_t(tag[pos+5]&0x7F)<<14)
                  | (uint32_t(tag[pos+6]&0x7F)<<7)  |  uint32_t(tag[pos+7]&0x7F);
        } else {
            fsize = (uint32_t(tag[pos+4])<<24)|(uint32_t(tag[pos+5])<<16)
                  | (uint32_t(tag[pos+6])<<8) | uint32_t(tag[pos+7]);
        }
        // flags v2.3/2.4: [10..11]
        const size_t pl = pos + 10;
        if (pl + fsize > tag.size()) break;
        if (fsize == 0) { pos = pl; continue; }
        const uint8_t enc = tag[pl];     // 第一字节通常是 encoding(对 T*** frame 始终如此)

        auto read_text_frame = [&]() {
            return decode_text(tag.data() + pl + 1, fsize - 1, enc);
        };

        if (std::memcmp(fid, "TIT2", 4) == 0) {
            md.title = read_text_frame(); found = true;
        } else if (std::memcmp(fid, "TPE1", 4) == 0) {
            md.artist = read_text_frame(); found = true;
        } else if (std::memcmp(fid, "TALB", 4) == 0) {
            md.album = read_text_frame(); found = true;
        } else if (std::memcmp(fid, "TYER", 4) == 0 || std::memcmp(fid, "TDRC", 4) == 0) {
            md.date = read_text_frame(); found = true;
        } else if (std::memcmp(fid, "TRCK", 4) == 0) {
            const auto w = read_text_frame();
            // std::stoi 接受 wstring 重载；之前 std::wstring(w.begin(), w.end()) 是冗余拷贝。
            try { md.track_no = std::stoi(w); found = true; }
            catch (...) {}
        } else if (std::memcmp(fid, "TXXX", 4) == 0) {
            // TXXX: enc(1) desc(NUL-term in encoding) value
            // desc 的 NUL 终止符宽度依赖于 enc:UTF-16 是双字节 NUL
            const uint8_t* payload = tag.data() + pl + 1;
            const size_t   plen    = fsize - 1;
            std::string desc;
            std::string value;
            size_t cut = 0;
            if (enc == 1 || enc == 2) {
                // UTF-16:寻找 [0,0] 终止
                for (size_t i = 0; i + 1 < plen; i += 2) {
                    if (payload[i] == 0 && payload[i+1] == 0) { cut = i; break; }
                }
            } else {
                for (size_t i = 0; i < plen; ++i) {
                    if (payload[i] == 0) { cut = i; break; }
                }
            }
            if (cut > 0 && cut < plen) {
                const std::wstring wdesc = decode_text(payload, cut, enc);
                std::string ascii_desc;
                ascii_desc.reserve(wdesc.size());
                for (wchar_t c : wdesc) {
                    ascii_desc.push_back((c < 0x80) ? static_cast<char>(c) : '?');
                }
                std::transform(ascii_desc.begin(), ascii_desc.end(), ascii_desc.begin(),
                               [](unsigned char c){ return static_cast<char>(std::toupper(c)); });

                const size_t vskip = (enc == 1 || enc == 2) ? cut + 2 : cut + 1;
                const std::wstring wval = (vskip < plen)
                    ? decode_text(payload + vskip, plen - vskip, enc)
                    : std::wstring{};
                // 把 wval 转 ASCII 用于 stod
                std::string sval; sval.reserve(wval.size());
                for (wchar_t c : wval) if (c < 0x80) sval.push_back(static_cast<char>(c));

                if      (ascii_desc == "REPLAYGAIN_TRACK_GAIN") { try { md.rg_track_gain_db = std::stod(sval); found = true; } catch (...) {} }
                else if (ascii_desc == "REPLAYGAIN_TRACK_PEAK") { try { md.rg_track_peak    = std::stod(sval); found = true; } catch (...) {} }
                else if (ascii_desc == "REPLAYGAIN_ALBUM_GAIN") { try { md.rg_album_gain_db = std::stod(sval); found = true; } catch (...) {} }
                else if (ascii_desc == "REPLAYGAIN_ALBUM_PEAK") { try { md.rg_album_peak    = std::stod(sval); found = true; } catch (...) {} }
            }
        } else if (std::memcmp(fid, "APIC", 4) == 0) {
            md.has_cover = true; found = true;
        }

        pos = pl + fsize;
    }

    if (!found) return std::nullopt;
    return md;
}

// ============================================================================
//  dispatch by extension
// ============================================================================
std::optional<TrackMetadata> MetadataReader::read(const std::wstring& path)
{
    if (endsWith(path, L".wav"))  return readWav(path);
    if (endsWith(path, L".flac")) return readFlac(path);
    if (endsWith(path, L".mp3"))  return readMp3(path);
    return std::nullopt;
}

std::optional<CoverImage> MetadataReader::readCover(const std::wstring& path)
{
    if (endsWith(path, L".flac")) return readFlacCover(path);
    return std::nullopt;
}

// ============================================================================
//  FLAC PICTURE block
// ============================================================================
std::optional<CoverImage> MetadataReader::readFlacCover(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return std::nullopt;
    auto close = [&]() { if (f) { std::fclose(f); f = nullptr; } };

    uint8_t marker[4];
    if (!fread_n(f, marker, 4)) { close(); return std::nullopt; }
    if (std::memcmp(marker, "fLaC", 4) != 0) { close(); return std::nullopt; }

    bool last = false;
    int blocks = 0;

    while (!last && blocks++ < 32) {
        uint8_t bh[4];
        if (!fread_n(f, bh, 4)) break;
        last = (bh[0] & 0x80) != 0;
        int type = bh[0] & 0x7F;
        uint32_t len = (uint32_t(bh[1]) << 16) | (uint32_t(bh[2]) << 8) | uint32_t(bh[3]);

        if (type == 6) {
            // PICTURE block 布局(全部大端):
            //   4  picture type (3 = front cover preferred)
            //   4  mime length
            //   N  mime ASCII
            //   4  desc length
            //   N  desc UTF-8
            //   4  width
            //   4  height
            //   4  color depth
            //   4  indexed colors
            //   4  picture data length
            //   N  picture data
            std::vector<uint8_t> buf(len);
            if (len > 0 && std::fread(buf.data(), 1, len, f) != len) { close(); return std::nullopt; }
            size_t off = 0;
            if (off + 4 > buf.size()) continue;
            // pic_type 暂不严格筛选,先实现取第一张
            off += 4;
            if (off + 4 > buf.size()) continue;
            uint32_t mimeLen = readU32BE(buf.data() + off); off += 4;
            if (off + mimeLen > buf.size()) continue;
            std::string mime(reinterpret_cast<const char*>(buf.data() + off), mimeLen);
            off += mimeLen;

            if (off + 4 > buf.size()) continue;
            uint32_t descLen = readU32BE(buf.data() + off); off += 4;
            if (off + descLen > buf.size()) continue;
            off += descLen;

            // 跳过 width/height/depth/indexed (16 字节)
            if (off + 16 > buf.size()) continue;
            off += 16;

            if (off + 4 > buf.size()) continue;
            uint32_t picLen = readU32BE(buf.data() + off); off += 4;
            if (off + picLen > buf.size()) continue;

            CoverImage cov;
            cov.mime = std::move(mime);
            cov.data.assign(buf.begin() + off, buf.begin() + off + picLen);
            close();
            return cov;
        } else {
            std::fseek(f, len, SEEK_CUR);
        }
    }

    close();
    return std::nullopt;
}

// ============================================================================
//  内嵌歌词
// ============================================================================
std::optional<std::string> MetadataReader::readEmbeddedLyrics(const std::wstring& path)
{
    if (endsWith(path, L".mp3"))  return readEmbeddedLyricsMp3(path);
    if (endsWith(path, L".flac")) return readEmbeddedLyricsFlac(path);
    if (endsWith(path, L".m4a") || endsWith(path, L".mp4") || endsWith(path, L".aac"))
        return readEmbeddedLyricsM4a(path);
    return std::nullopt;
}

// ----------------------------------------------------------------------------
// MP3 ID3v2 USLT(非同步歌词) + SYLT(同步歌词)
//   USLT payload = enc(1) lang(3) desc(enc-NUL) lyrics(enc, rest)
//   SYLT payload = enc(1) lang(3) ts_format(1) content_type(1)
//                  + desc(enc-NUL) + (text(enc-NUL) + ts(4 BE uint32))*
//   enc: 0=ISO-8859-1, 1=UTF-16 BOM, 2=UTF-16BE, 3=UTF-8 (v2.4)
//   SYLT 优先(带时间戳),USLT 兜底;ts_format 只支持 2(毫秒)。
// ----------------------------------------------------------------------------
std::optional<std::string> MetadataReader::readEmbeddedLyricsMp3(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return std::nullopt;
    auto close_f = [&]() { if (f) { std::fclose(f); f = nullptr; } };

    uint8_t hdr[10];
    if (!fread_n(f, hdr, 10)) { close_f(); return std::nullopt; }
    if (std::memcmp(hdr, "ID3", 3) != 0) { close_f(); return std::nullopt; }

    const int ver_major = hdr[3];
    const uint8_t flags = hdr[5];
    const uint32_t total =
        (uint32_t(hdr[6] & 0x7F) << 21) |
        (uint32_t(hdr[7] & 0x7F) << 14) |
        (uint32_t(hdr[8] & 0x7F) << 7)  |
        (uint32_t(hdr[9] & 0x7F));
    if (total == 0 || total > 64 * 1024 * 1024) { close_f(); return std::nullopt; }

    std::vector<uint8_t> tag(total);
    if (std::fread(tag.data(), 1, total, f) != total) { close_f(); return std::nullopt; }
    close_f();

    size_t pos = 0;
    if (flags & 0x40) {
        if (pos + 4 > tag.size()) return std::nullopt;
        uint32_t ext;
        if (ver_major >= 4) {
            ext = (uint32_t(tag[pos]&0x7F)<<21) | (uint32_t(tag[pos+1]&0x7F)<<14)
                | (uint32_t(tag[pos+2]&0x7F)<<7)  |  uint32_t(tag[pos+3]&0x7F);
        } else {
            ext = (uint32_t(tag[pos])<<24)|(uint32_t(tag[pos+1])<<16)
                | (uint32_t(tag[pos+2])<<8) | uint32_t(tag[pos+3]);
        }
        pos += ext;
        if (pos >= tag.size()) return std::nullopt;
    }

    auto decodeBytes = [](const uint8_t* p, size_t n, uint8_t enc) -> std::wstring {
        if (n == 0) return {};
        std::string s(reinterpret_cast<const char*>(p), n);
        while (!s.empty() && s.back() == '\0') s.pop_back();
        switch (enc) {
        case 0: return cp1252ToW(s);
        case 3: return utf8ToW(s);
        case 1: {
            if (s.size() < 2) return {};
            const uint8_t* b = reinterpret_cast<const uint8_t*>(s.data());
            bool be = false; size_t start = 0;
            if (b[0] == 0xFF && b[1] == 0xFE)      { be = false; start = 2; }
            else if (b[0] == 0xFE && b[1] == 0xFF) { be = true;  start = 2; }
            std::wstring w;
            for (size_t i = start; i + 1 < s.size(); i += 2) {
                uint16_t u = be ? (uint16_t(b[i])<<8) | b[i+1]
                                : (uint16_t(b[i+1])<<8) | b[i];
                w.push_back(static_cast<wchar_t>(u));
            }
            while (!w.empty() && w.back() == 0) w.pop_back();
            return w;
        }
        case 2: {
            std::wstring w;
            for (size_t i = 0; i + 1 < s.size(); i += 2) {
                uint16_t u = (uint16_t(static_cast<unsigned char>(s[i]))<<8)
                           | static_cast<unsigned char>(s[i+1]);
                w.push_back(static_cast<wchar_t>(u));
            }
            while (!w.empty() && w.back() == 0) w.pop_back();
            return w;
        }
        }
        return {};
    };

    // 两个候选:SYLT 带时间戳更精确,优先返回;USLT 兜底。
    std::optional<std::string> uslt_out;
    std::optional<std::string> sylt_out;

    while (pos + 10 <= tag.size()) {
        char fid[5] = {0};
        std::memcpy(fid, tag.data() + pos, 4);
        if (fid[0] == 0) break;
        uint32_t fsize;
        if (ver_major >= 4) {
            fsize = (uint32_t(tag[pos+4]&0x7F)<<21) | (uint32_t(tag[pos+5]&0x7F)<<14)
                  | (uint32_t(tag[pos+6]&0x7F)<<7)  |  uint32_t(tag[pos+7]&0x7F);
        } else {
            fsize = (uint32_t(tag[pos+4])<<24)|(uint32_t(tag[pos+5])<<16)
                  | (uint32_t(tag[pos+6])<<8) | uint32_t(tag[pos+7]);
        }
        const size_t pl = pos + 10;
        if (pl + fsize > tag.size()) break;
        if (fsize == 0) { pos = pl; continue; }

        if (!uslt_out && std::memcmp(fid, "USLT", 4) == 0 && fsize >= 5) {
            const uint8_t enc = tag[pl];
            const uint8_t* p   = tag.data() + pl + 4;   // 跳过 enc(1) + lang(3)
            size_t          n   = fsize - 4;
            // desc 由 enc 决定的 NUL 终止
            size_t cut = std::string::npos;
            if (enc == 1 || enc == 2) {
                for (size_t i = 0; i + 1 < n; i += 2) {
                    if (p[i] == 0 && p[i+1] == 0) { cut = i; break; }
                }
            } else {
                for (size_t i = 0; i < n; ++i) {
                    if (p[i] == 0) { cut = i; break; }
                }
            }
            size_t vskip = (cut == std::string::npos)
                            ? 0
                            : ((enc == 1 || enc == 2) ? cut + 2 : cut + 1);
            if (vskip > n) vskip = n;
            std::wstring w = decodeBytes(p + vskip, n - vskip, enc);
            if (!w.empty()) uslt_out = wToUtf8(w);
        } else if (!sylt_out && std::memcmp(fid, "SYLT", 4) == 0 && fsize >= 7) {
            const uint8_t enc        = tag[pl];
            const uint8_t ts_format  = tag[pl + 4];
            // tag[pl+5] = content_type;1=lyrics,这里宽松接受所有类型
            const uint8_t* p = tag.data() + pl + 6;
            size_t          n = fsize - 6;
            if (ts_format == 2) {
                // 跳过 desc
                size_t dend = std::string::npos;
                if (enc == 1 || enc == 2) {
                    for (size_t i = 0; i + 1 < n; i += 2) {
                        if (p[i] == 0 && p[i+1] == 0) { dend = i; break; }
                    }
                } else {
                    for (size_t i = 0; i < n; ++i) {
                        if (p[i] == 0) { dend = i; break; }
                    }
                }
                if (dend != std::string::npos) {
                    size_t cur = (enc == 1 || enc == 2) ? dend + 2 : dend + 1;
                    std::string lrc;
                    char tbuf[24];
                    while (cur < n) {
                        size_t tend = std::string::npos;
                        if (enc == 1 || enc == 2) {
                            for (size_t i = cur; i + 1 < n; i += 2) {
                                if (p[i] == 0 && p[i+1] == 0) { tend = i; break; }
                            }
                            if (tend == std::string::npos) break;
                            std::wstring w = decodeBytes(p + cur, tend - cur, enc);
                            size_t tsoff = tend + 2;
                            if (tsoff + 4 > n) break;
                            uint32_t ms = (uint32_t(p[tsoff])<<24)
                                        | (uint32_t(p[tsoff+1])<<16)
                                        | (uint32_t(p[tsoff+2])<<8)
                                        |  uint32_t(p[tsoff+3]);
                            cur = tsoff + 4;
                            std::wstring line;
                            line.reserve(w.size());
                            for (wchar_t c : w) {
                                if (c == 0x0A || c == 0x0D) line += L' ';
                                else                         line += c;
                            }
                            int mm = static_cast<int>(ms / 60000);
                            int ss = static_cast<int>((ms / 1000) % 60);
                            int xx = static_cast<int>((ms % 1000) / 10);
                            std::snprintf(tbuf, sizeof(tbuf), "[%02d:%02d.%02d]", mm, ss, xx);
                            lrc += tbuf;
                            lrc += wToUtf8(line);
                            lrc += '\n';
                        } else {
                            for (size_t i = cur; i < n; ++i) {
                                if (p[i] == 0) { tend = i; break; }
                            }
                            if (tend == std::string::npos) break;
                            std::wstring w = decodeBytes(p + cur, tend - cur, enc);
                            size_t tsoff = tend + 1;
                            if (tsoff + 4 > n) break;
                            uint32_t ms = (uint32_t(p[tsoff])<<24)
                                        | (uint32_t(p[tsoff+1])<<16)
                                        | (uint32_t(p[tsoff+2])<<8)
                                        |  uint32_t(p[tsoff+3]);
                            cur = tsoff + 4;
                            std::wstring line;
                            line.reserve(w.size());
                            for (wchar_t c : w) {
                                if (c == 0x0A || c == 0x0D) line += L' ';
                                else                         line += c;
                            }
                            int mm = static_cast<int>(ms / 60000);
                            int ss = static_cast<int>((ms / 1000) % 60);
                            int xx = static_cast<int>((ms % 1000) / 10);
                            std::snprintf(tbuf, sizeof(tbuf), "[%02d:%02d.%02d]", mm, ss, xx);
                            lrc += tbuf;
                            lrc += wToUtf8(line);
                            lrc += '\n';
                        }
                    }
                    if (!lrc.empty()) sylt_out = std::move(lrc);
                }
            }
        }
        pos = pl + fsize;
    }
    if (sylt_out) return sylt_out;
    if (uslt_out) return uslt_out;
    return std::nullopt;
}

// ----------------------------------------------------------------------------
// FLAC VORBIS_COMMENT 里 LYRICS / UNSYNCEDLYRICS / SYNCEDLYRICS
// 值就是 UTF-8 原文,直接返回。
// ----------------------------------------------------------------------------
std::optional<std::string> MetadataReader::readEmbeddedLyricsFlac(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return std::nullopt;
    auto close_f = [&]() { if (f) { std::fclose(f); f = nullptr; } };

    uint8_t marker[4];
    if (!fread_n(f, marker, 4)) { close_f(); return std::nullopt; }
    if (std::memcmp(marker, "fLaC", 4) != 0) { close_f(); return std::nullopt; }

    bool last = false;
    int blocks = 0;
    while (!last && blocks++ < 32) {
        uint8_t bh[4];
        if (!fread_n(f, bh, 4)) break;
        last = (bh[0] & 0x80) != 0;
        int type = bh[0] & 0x7F;
        uint32_t len = (uint32_t(bh[1]) << 16) | (uint32_t(bh[2]) << 8) | uint32_t(bh[3]);

        if (type == 4) {
            std::vector<uint8_t> buf(len);
            if (len > 0 && std::fread(buf.data(), 1, len, f) != len) break;
            if (buf.size() < 4) continue;
            uint32_t vlen = readU32LE(buf.data());
            size_t off = 4;
            if (off + vlen + 4 > buf.size()) continue;
            off += vlen;
            uint32_t n = readU32LE(buf.data() + off); off += 4;
            std::string best;
            int bestRank = 99;     // 越小越优先
            for (uint32_t i = 0; i < n; ++i) {
                if (off + 4 > buf.size()) break;
                uint32_t clen = readU32LE(buf.data() + off); off += 4;
                if (off + clen > buf.size()) break;
                std::string entry(reinterpret_cast<const char*>(buf.data() + off), clen);
                off += clen;
                auto eq = entry.find('=');
                if (eq == std::string::npos) continue;
                std::string key = entry.substr(0, eq);
                std::transform(key.begin(), key.end(), key.begin(),
                               [](unsigned char c){ return static_cast<char>(std::toupper(c)); });
                int rank = 99;
                if      (key == "SYNCEDLYRICS")   rank = 0;
                else if (key == "LYRICS")          rank = 1;
                else if (key == "UNSYNCEDLYRICS") rank = 2;
                if (rank < bestRank) {
                    best = entry.substr(eq + 1);
                    bestRank = rank;
                }
            }
            close_f();
            if (best.empty()) return std::nullopt;
            return best;
        } else {
            std::fseek(f, len, SEEK_CUR);
        }
    }
    close_f();
    return std::nullopt;
}

// ----------------------------------------------------------------------------
// MP4/M4A iTunes ©lyr atom (UTF-8 lyrics)
//   atom 树: moov → udta → meta(FullBox) → ilst → ©lyr → data
//   ©lyr type 字节 = 0xA9 'l' 'y' 'r'
//   data atom payload: type_flags(4 BE) + locale(4) + payload bytes
//     type_flags=1 表示 UTF-8。其它编码(0 reserved / 2 UTF-16 等)罕见,统一按
//     UTF-8 返回上层让 LyricsLoader::parseDoc 自己嗅探。
// ----------------------------------------------------------------------------
std::optional<std::string> MetadataReader::readEmbeddedLyricsM4a(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return std::nullopt;
    auto close_f = [&]() { if (f) { std::fclose(f); f = nullptr; } };

    if (_fseeki64(f, 0, SEEK_END) != 0) { close_f(); return std::nullopt; }
    long long fileSize = _ftelli64(f);
    if (fileSize <= 16) { close_f(); return std::nullopt; }
    _fseeki64(f, 0, SEEK_SET);

    // 在 [start, end) 范围内顺序遍历子 atom,匹配 want[0..3] 时返回 atom 内容区间。
    // want 用 4 字节 memcmp,允许首字节 = 0xA9 这种非 ASCII 类型。
    auto findChild = [&](long long start, long long end, const char want[4],
                         long long& outStart, long long& outEnd) -> bool {
        long long p = start;
        while (p + 8 <= end) {
            if (_fseeki64(f, p, SEEK_SET) != 0) return false;
            uint8_t h[8];
            if (std::fread(h, 1, 8, f) != 8) return false;
            uint64_t sz = (uint64_t(h[0])<<24) | (uint64_t(h[1])<<16)
                        | (uint64_t(h[2])<<8)  |  uint64_t(h[3]);
            uint8_t hdrLen = 8;
            if (sz == 1) {
                uint8_t e[8];
                if (std::fread(e, 1, 8, f) != 8) return false;
                sz = (uint64_t(e[0])<<56) | (uint64_t(e[1])<<48)
                   | (uint64_t(e[2])<<40) | (uint64_t(e[3])<<32)
                   | (uint64_t(e[4])<<24) | (uint64_t(e[5])<<16)
                   | (uint64_t(e[6])<<8)  |  uint64_t(e[7]);
                hdrLen = 16;
            } else if (sz == 0) {
                sz = static_cast<uint64_t>(end - p);
            }
            if (sz < hdrLen) return false;
            long long cs = p + hdrLen;
            long long ce = p + static_cast<long long>(sz);
            if (ce > end) ce = end;
            if (std::memcmp(h + 4, want, 4) == 0) {
                outStart = cs;
                outEnd   = ce;
                return true;
            }
            if (ce <= p) return false;       // 防止异常 size 死循环
            p = ce;
        }
        return false;
    };

    long long moovS, moovE;
    if (!findChild(0, fileSize, "moov", moovS, moovE)) { close_f(); return std::nullopt; }
    long long udtaS, udtaE;
    if (!findChild(moovS, moovE, "udta", udtaS, udtaE)) { close_f(); return std::nullopt; }
    long long metaS, metaE;
    if (!findChild(udtaS, udtaE, "meta", metaS, metaE)) { close_f(); return std::nullopt; }
    // meta 是 FullBox,跳过 4 字节 version+flags
    long long ilstS, ilstE;
    if (!findChild(metaS + 4, metaE, "ilst", ilstS, ilstE)) { close_f(); return std::nullopt; }

    const char lyrType[4] = { static_cast<char>(0xA9), 'l', 'y', 'r' };
    long long lyrS, lyrE;
    if (!findChild(ilstS, ilstE, lyrType, lyrS, lyrE)) { close_f(); return std::nullopt; }

    long long dataS, dataE;
    if (!findChild(lyrS, lyrE, "data", dataS, dataE)) { close_f(); return std::nullopt; }

    if (dataE - dataS < 8) { close_f(); return std::nullopt; }
    if (_fseeki64(f, dataS + 8, SEEK_SET) != 0) { close_f(); return std::nullopt; }
    long long payloadLen = dataE - dataS - 8;
    if (payloadLen <= 0 || payloadLen > 4 * 1024 * 1024) { close_f(); return std::nullopt; }

    std::string out(static_cast<size_t>(payloadLen), '\0');
    if (static_cast<long long>(std::fread(out.data(), 1, payloadLen, f)) != payloadLen) {
        close_f();
        return std::nullopt;
    }
    close_f();
    if (out.empty()) return std::nullopt;
    return out;
}

} // namespace apx
