// =============================================================================
//  app/playlist/PlaylistIO.cpp
// =============================================================================
#include "PlaylistIO.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <sstream>

namespace apx {

namespace {

// ----- wstring <-> UTF-8 -----
std::string wToUtf8(const std::wstring& w)
{
    std::string s;
    s.reserve(w.size());
    for (std::size_t i = 0; i < w.size(); ++i) {
        std::uint32_t cp = static_cast<std::uint16_t>(w[i]);
        // 处理 surrogate pair → 4-byte UTF-8
        if (cp >= 0xD800 && cp <= 0xDBFF && i + 1 < w.size()) {
            std::uint32_t lo = static_cast<std::uint16_t>(w[i+1]);
            if (lo >= 0xDC00 && lo <= 0xDFFF) {
                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                ++i;
            }
        }
        if (cp < 0x80) {
            s.push_back(static_cast<char>(cp));
        } else if (cp < 0x800) {
            s.push_back(static_cast<char>(0xC0 | (cp >> 6)));
            s.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else if (cp < 0x10000) {
            s.push_back(static_cast<char>(0xE0 | (cp >> 12)));
            s.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            s.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else {
            s.push_back(static_cast<char>(0xF0 | (cp >> 18)));
            s.push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
            s.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            s.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        }
    }
    return s;
}

std::wstring utf8ToW(const std::string& s)
{
    std::wstring w;
    w.reserve(s.size());
    for (std::size_t i = 0; i < s.size(); ) {
        unsigned char c = static_cast<unsigned char>(s[i]);
        std::uint32_t cp = 0; int extra = 0;
        if (c < 0x80)        { cp = c;       extra = 0; }
        else if ((c & 0xE0) == 0xC0) { cp = c & 0x1F; extra = 1; }
        else if ((c & 0xF0) == 0xE0) { cp = c & 0x0F; extra = 2; }
        else if ((c & 0xF8) == 0xF0) { cp = c & 0x07; extra = 3; }
        else { ++i; continue; }
        ++i;
        for (int k = 0; k < extra; ++k) {
            if (i >= s.size()) { extra = -1; break; }
            unsigned char cc = static_cast<unsigned char>(s[i++]);
            if ((cc & 0xC0) != 0x80) { extra = -1; break; }
            cp = (cp << 6) | (cc & 0x3F);
        }
        if (extra < 0) continue;
        if (cp < 0x10000) {
            w.push_back(static_cast<wchar_t>(cp));
        } else {
            cp -= 0x10000;
            w.push_back(static_cast<wchar_t>(0xD800 | (cp >> 10)));
            w.push_back(static_cast<wchar_t>(0xDC00 | (cp & 0x3FF)));
        }
    }
    return w;
}

bool isAbsPath(const std::wstring& p)
{
    if (p.size() >= 2 && p[1] == L':') return true;
    if (!p.empty() && (p[0] == L'\\' || p[0] == L'/')) return true;
    return false;
}

std::wstring dirOf(const std::wstring& p)
{
    auto pos = p.find_last_of(L"\\/");
    return pos == std::wstring::npos ? L"" : p.substr(0, pos);
}

std::wstring trim(const std::wstring& s)
{
    std::size_t a = 0, b = s.size();
    while (a < b && (s[a] == L' ' || s[a] == L'\t' || s[a] == L'\r')) ++a;
    while (b > a && (s[b-1] == L' ' || s[b-1] == L'\t' || s[b-1] == L'\r')) --b;
    return s.substr(a, b - a);
}

// ----- JSON 最小解析器 (只支持 PlaylistIO 自己生成的结构) -----

class JsonReader {
public:
    explicit JsonReader(const std::string& src) : s_(src) {}
    bool ok() const { return ok_; }

    void skipWs() {
        while (i_ < s_.size()) {
            char c = s_[i_];
            if (c == ' ' || c == '\t' || c == '\r' || c == '\n') ++i_;
            else break;
        }
    }
    bool consume(char c) {
        skipWs();
        if (i_ < s_.size() && s_[i_] == c) { ++i_; return true; }
        ok_ = false; return false;
    }
    bool peek(char c) {
        skipWs();
        return i_ < s_.size() && s_[i_] == c;
    }
    // 解析 "..." 字符串(含基本转义);写入 utf8 字节
    bool readString(std::string& out) {
        skipWs();
        if (i_ >= s_.size() || s_[i_] != '"') { ok_ = false; return false; }
        ++i_;
        out.clear();
        while (i_ < s_.size()) {
            char c = s_[i_++];
            if (c == '"') return true;
            if (c == '\\' && i_ < s_.size()) {
                char e = s_[i_++];
                switch (e) {
                case '"': out.push_back('"'); break;
                case '\\': out.push_back('\\'); break;
                case '/': out.push_back('/'); break;
                case 'b': out.push_back('\b'); break;
                case 'f': out.push_back('\f'); break;
                case 'n': out.push_back('\n'); break;
                case 'r': out.push_back('\r'); break;
                case 't': out.push_back('\t'); break;
                case 'u': {
                    if (i_ + 4 > s_.size()) { ok_ = false; return false; }
                    char hex[5] = { s_[i_], s_[i_+1], s_[i_+2], s_[i_+3], 0 };
                    i_ += 4;
                    unsigned cp = std::strtoul(hex, nullptr, 16);
                    // 仅 BMP;surrogate pair 不展开(写出方也用 \uXXXX 形式)
                    if (cp < 0x80) out.push_back(static_cast<char>(cp));
                    else if (cp < 0x800) {
                        out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
                        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
                    } else {
                        out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
                        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
                        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
                    }
                    break;
                }
                default: out.push_back(e); break;
                }
            } else {
                out.push_back(c);
            }
        }
        ok_ = false; return false;
    }
    bool readDouble(double& out) {
        skipWs();
        std::size_t start = i_;
        while (i_ < s_.size() &&
               (s_[i_] == '-' || s_[i_] == '+' || s_[i_] == '.' ||
                s_[i_] == 'e' || s_[i_] == 'E' ||
                (s_[i_] >= '0' && s_[i_] <= '9'))) ++i_;
        if (start == i_) { ok_ = false; return false; }
        out = std::strtod(s_.c_str() + start, nullptr);
        return true;
    }
    bool readInt(long long& out) {
        double d = 0; if (!readDouble(d)) return false;
        out = static_cast<long long>(d);
        return true;
    }

private:
    const std::string& s_;
    std::size_t i_ = 0;
    bool ok_ = true;
};

// ----- JSON 序列化:转义字符串 -----
void jsonEscape(std::ostringstream& os, const std::string& s)
{
    os << '"';
    for (char c : s) {
        switch (c) {
        case '"': os << "\\\""; break;
        case '\\': os << "\\\\"; break;
        case '\b': os << "\\b";  break;
        case '\f': os << "\\f";  break;
        case '\n': os << "\\n";  break;
        case '\r': os << "\\r";  break;
        case '\t': os << "\\t";  break;
        default:
            if (static_cast<unsigned char>(c) < 0x20) {
                char buf[8];
                std::snprintf(buf, sizeof(buf), "\\u%04x", c & 0xFF);
                os << buf;
            } else {
                os << c;
            }
        }
    }
    os << '"';
}

void jsonField(std::ostringstream& os, const char* name, const std::wstring& v, bool first)
{
    if (!first) os << ',';
    os << '"' << name << "\":";
    jsonEscape(os, wToUtf8(v));
}
void jsonField(std::ostringstream& os, const char* name, double v, bool first)
{
    if (!first) os << ',';
    os << '"' << name << "\":" << v;
}
void jsonField(std::ostringstream& os, const char* name, long long v, bool first)
{
    if (!first) os << ',';
    os << '"' << name << "\":" << v;
}

const char* modeName(PlaybackMode m)
{
    switch (m) {
    case PlaybackMode::LoopList:   return "LoopList";
    case PlaybackMode::LoopOne:    return "LoopOne";
    case PlaybackMode::Shuffle:    return "Shuffle";
    case PlaybackMode::Sequential:
    default:                       return "Sequential";
    }
}
PlaybackMode modeFromName(const std::string& s)
{
    if (s == "LoopList")   return PlaybackMode::LoopList;
    if (s == "LoopOne")    return PlaybackMode::LoopOne;
    if (s == "Shuffle")    return PlaybackMode::Shuffle;
    return PlaybackMode::Sequential;
}

} // namespace

// ============================================================================
// M3U
// ============================================================================

bool PlaylistIO::loadM3U(const std::wstring& path, Playlist& out, std::wstring* err)
{
    std::ifstream fs(path);
    if (!fs) { if (err) *err = L"cannot open m3u file"; return false; }

    const std::wstring base = dirOf(path);
    out.clear();

    std::string line_u8;
    // 头部 BOM(UTF-8) 略过
    if (std::getline(fs, line_u8)) {
        if (line_u8.size() >= 3
            && static_cast<unsigned char>(line_u8[0]) == 0xEF
            && static_cast<unsigned char>(line_u8[1]) == 0xBB
            && static_cast<unsigned char>(line_u8[2]) == 0xBF) {
            line_u8.erase(0, 3);
        }
        fs.seekg(0);
        std::string discard;
        std::getline(fs, discard);   // 跳过同一行
        if (line_u8.find("#EXTM3U") != std::string::npos) {
            // 已消耗,继续
        } else if (!line_u8.empty() && line_u8[0] != '#') {
            // 首行就是 path
            std::wstring p = utf8ToW(line_u8);
            if (!isAbsPath(p) && !base.empty()) p = base + L'\\' + p;
            out.appendPath(p);
        }
    }

    // 上一行 EXTINF 缓存的 (duration, title)
    double pending_dur = 0.0;
    std::wstring pending_title;
    bool         have_pending = false;

    while (std::getline(fs, line_u8)) {
        std::wstring line = trim(utf8ToW(line_u8));
        if (line.empty()) continue;

        if (line[0] == L'#') {
            // #EXTINF:<sec>,<title>
            const std::wstring head = L"#EXTINF:";
            if (line.compare(0, head.size(), head) == 0) {
                std::wstring rest = line.substr(head.size());
                auto comma = rest.find(L',');
                if (comma != std::wstring::npos) {
                    try {
                        pending_dur = std::stod(std::wstring(rest, 0, comma));
                    } catch (...) { pending_dur = 0.0; }
                    pending_title = rest.substr(comma + 1);
                } else {
                    try { pending_dur = std::stod(rest); }
                    catch (...) { pending_dur = 0.0; }
                    pending_title.clear();
                }
                have_pending = true;
            }
            continue;
        }

        PlaylistItem it;
        it.path = isAbsPath(line) ? line :
                  (base.empty() ? line : base + L'\\' + line);
        if (have_pending) {
            it.title = pending_title;
            it.duration_sec = pending_dur;
            have_pending = false;
        }
        out.append(std::move(it));
    }
    return true;
}

bool PlaylistIO::saveM3U(const Playlist& in, const std::wstring& path, std::wstring* err)
{
    // 用宽字符 ofstream + UTF-8 字节;M3U8 约定 UTF-8 编码 + BOM
    FILE* fp = nullptr;
    if (_wfopen_s(&fp, path.c_str(), L"wb") != 0 || !fp) {
        if (err) *err = L"cannot open m3u for writing";
        return false;
    }
    // BOM
    const unsigned char bom[3] = { 0xEF, 0xBB, 0xBF };
    std::fwrite(bom, 1, 3, fp);
    std::fputs("#EXTM3U\n", fp);
    for (const auto& it : in.items()) {
        // #EXTINF:<duration>,<title>
        std::ostringstream os;
        os << "#EXTINF:" << static_cast<long long>(it.duration_sec) << ',';
        if (!it.artist.empty()) os << wToUtf8(it.artist) << " - ";
        os << wToUtf8(it.title.empty() ? it.path : it.title);
        os << '\n';
        std::fputs(os.str().c_str(), fp);
        // path
        std::fputs(wToUtf8(it.path).c_str(), fp);
        std::fputc('\n', fp);
    }
    std::fclose(fp);
    return true;
}

// ============================================================================
// JSON
// ============================================================================

bool PlaylistIO::saveJson(const Playlist& in, const std::wstring& path, std::wstring* err)
{
    std::ostringstream os;
    os << "{\"version\":1";
    os << ",\"mode\":\"" << modeName(in.mode()) << "\"";
    os << ",\"current\":" << in.currentIndex();
    os << ",\"items\":[";
    bool first_item = true;
    for (const auto& it : in.items()) {
        if (!first_item) os << ',';
        first_item = false;
        os << '{';
        jsonField(os, "path",       it.path, true);
        jsonField(os, "title",      it.title, false);
        jsonField(os, "artist",     it.artist, false);
        jsonField(os, "album",      it.album, false);
        jsonField(os, "track",      static_cast<long long>(it.track_index), false);
        jsonField(os, "duration",   it.duration_sec, false);
        jsonField(os, "cue_start",  it.cue_start_sec, false);
        jsonField(os, "cue_end",    it.cue_end_sec, false);
        os << '}';
    }
    os << "]}";

    FILE* fp = nullptr;
    if (_wfopen_s(&fp, path.c_str(), L"wb") != 0 || !fp) {
        if (err) *err = L"cannot open json for writing";
        return false;
    }
    const std::string s = os.str();
    std::fwrite(s.data(), 1, s.size(), fp);
    std::fclose(fp);
    return true;
}

bool PlaylistIO::loadJson(const std::wstring& path, Playlist& out, std::wstring* err)
{
    FILE* fp = nullptr;
    if (_wfopen_s(&fp, path.c_str(), L"rb") != 0 || !fp) {
        if (err) *err = L"cannot open json for reading";
        return false;
    }
    std::string buf;
    char chunk[4096];
    while (true) {
        std::size_t r = std::fread(chunk, 1, sizeof(chunk), fp);
        if (r == 0) break;
        buf.append(chunk, r);
    }
    std::fclose(fp);

    JsonReader jr(buf);
    out.clear();
    if (!jr.consume('{')) { if (err) *err = L"expected '{'"; return false; }
    while (true) {
        std::string key;
        if (!jr.readString(key)) { if (err) *err = L"key error"; return false; }
        if (!jr.consume(':'))    { if (err) *err = L"expected ':'"; return false; }
        if (key == "version") {
            long long v = 0; if (!jr.readInt(v)) return false;
        } else if (key == "mode") {
            std::string m; if (!jr.readString(m)) return false;
            out.setMode(modeFromName(m));
        } else if (key == "current") {
            long long v = -1; if (!jr.readInt(v)) return false;
            out.setCurrentIndex(static_cast<int>(v));
        } else if (key == "items") {
            if (!jr.consume('[')) { if (err) *err = L"expected '['"; return false; }
            while (true) {
                if (jr.peek(']')) { jr.consume(']'); break; }
                if (!jr.consume('{')) { if (err) *err = L"expected '{' in items"; return false; }
                PlaylistItem it;
                while (true) {
                    std::string fkey;
                    if (!jr.readString(fkey)) return false;
                    if (!jr.consume(':'))    return false;
                    if      (fkey == "path")      { std::string v; if (jr.readString(v)) it.path     = utf8ToW(v); }
                    else if (fkey == "title")     { std::string v; if (jr.readString(v)) it.title    = utf8ToW(v); }
                    else if (fkey == "artist")    { std::string v; if (jr.readString(v)) it.artist   = utf8ToW(v); }
                    else if (fkey == "album")     { std::string v; if (jr.readString(v)) it.album    = utf8ToW(v); }
                    else if (fkey == "track")     { long long v=0; if (jr.readInt(v))    it.track_index = static_cast<std::uint32_t>(v); }
                    else if (fkey == "duration")  { double v=0; if (jr.readDouble(v))    it.duration_sec = v; }
                    else if (fkey == "cue_start") { double v=0; if (jr.readDouble(v))    it.cue_start_sec = v; }
                    else if (fkey == "cue_end")   { double v=0; if (jr.readDouble(v))    it.cue_end_sec   = v; }
                    else { std::string discard; jr.readString(discard); /* 也可能不是 string,先忽略 */ }
                    if (jr.peek(',')) { jr.consume(','); continue; }
                    if (!jr.consume('}')) { if (err) *err = L"expected '}' in item"; return false; }
                    break;
                }
                out.append(std::move(it));
                if (jr.peek(',')) { jr.consume(','); continue; }
                if (!jr.consume(']')) { if (err) *err = L"expected ']' after items"; return false; }
                break;
            }
        } else {
            // 未知字段:跳过其值(粗暴方式:数器括号/引号到下一个 ',' 或 '}')
            // 这里不做严格解析;skip 到下一 key
            std::string discard;
            if (jr.peek('"')) jr.readString(discard);
            else { double d=0; jr.readDouble(d); }
        }
        if (jr.peek(',')) { jr.consume(','); continue; }
        if (!jr.consume('}')) { if (err) *err = L"expected '}'"; return false; }
        break;
    }
    return jr.ok();
}

} // namespace apx
