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
            try { md.track_no = std::stoi(std::wstring(w.begin(), w.end())); found = true; }
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

} // namespace apx
