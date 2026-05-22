// =============================================================================
//  core/lyrics/LyricsLoader.cpp
// =============================================================================
#include "LyricsLoader.h"

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstring>

#ifdef _WIN32
#  ifndef WIN32_LEAN_AND_MEAN
#    define WIN32_LEAN_AND_MEAN
#  endif
#  include <windows.h>
#endif

namespace apx {

namespace {

std::wstring utf8ToW(const std::string& s)
{
#ifdef _WIN32
    if (s.empty()) return {};
    int n = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS,
                                s.data(), static_cast<int>(s.size()), nullptr, 0);
    if (n <= 0) return {};
    std::wstring out(n, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.data(), static_cast<int>(s.size()),
                        out.data(), n);
    return out;
#else
    // 简化:非 Windows 直接按字节扩展
    return std::wstring(s.begin(), s.end());
#endif
}

std::wstring gbkToW(const std::string& s)
{
#ifdef _WIN32
    if (s.empty()) return {};
    int n = MultiByteToWideChar(936 /* GBK */, 0, s.data(), static_cast<int>(s.size()),
                                nullptr, 0);
    if (n <= 0) return {};
    std::wstring out(n, L'\0');
    MultiByteToWideChar(936, 0, s.data(), static_cast<int>(s.size()), out.data(), n);
    return out;
#else
    return std::wstring(s.begin(), s.end());
#endif
}

bool looksLikeUtf8(const std::string& s)
{
    int multi = 0;
    for (size_t i = 0; i < s.size();) {
        unsigned char c = static_cast<unsigned char>(s[i]);
        if (c < 0x80) { ++i; continue; }
        int extra;
        if      ((c & 0xE0) == 0xC0) extra = 1;
        else if ((c & 0xF0) == 0xE0) extra = 2;
        else if ((c & 0xF8) == 0xF0) extra = 3;
        else return false;
        if (i + extra >= s.size()) return false;
        for (int k = 1; k <= extra; ++k) {
            if ((static_cast<unsigned char>(s[i + k]) & 0xC0) != 0x80) return false;
        }
        i += extra + 1;
        ++multi;
    }
    return multi > 0;
}

std::wstring decode(const std::string& s)
{
    if (s.empty()) return {};
    if (looksLikeUtf8(s)) return utf8ToW(s);
    return gbkToW(s);
}

std::string readAll(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return {};
    std::fseek(f, 0, SEEK_END);
    long n = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    if (n <= 0) { std::fclose(f); return {}; }
    if (n > 4 * 1024 * 1024) n = 4 * 1024 * 1024;     // 4MB 上限
    std::string buf(n, '\0');
    std::fread(buf.data(), 1, n, f);
    std::fclose(f);
    // 去 BOM
    if (buf.size() >= 3 &&
        static_cast<unsigned char>(buf[0]) == 0xEF &&
        static_cast<unsigned char>(buf[1]) == 0xBB &&
        static_cast<unsigned char>(buf[2]) == 0xBF) {
        buf.erase(0, 3);
    }
    return buf;
}

// 把 audio_path 同目录同名 lrc 路径
std::wstring lrcPathFor(const std::wstring& audio_path)
{
    auto dot = audio_path.find_last_of(L'.');
    if (dot == std::wstring::npos) return audio_path + L".lrc";
    return audio_path.substr(0, dot) + L".lrc";
}

bool fileExists(const std::wstring& path)
{
#ifdef _WIN32
    DWORD attr = GetFileAttributesW(path.c_str());
    return attr != INVALID_FILE_ATTRIBUTES && !(attr & FILE_ATTRIBUTE_DIRECTORY);
#else
    FILE* f = std::fopen(reinterpret_cast<const char*>(path.c_str()), "rb");
    if (!f) return false;
    std::fclose(f);
    return true;
#endif
}

} // namespace

std::vector<LyricLine> LyricsLoader::loadFor(const std::wstring& audio_path)
{
    std::wstring lrc = lrcPathFor(audio_path);
    if (!fileExists(lrc)) return {};
    return load(lrc);
}

std::vector<LyricLine> LyricsLoader::load(const std::wstring& lrc_path)
{
    std::string raw = readAll(lrc_path);
    if (raw.empty()) return {};

    std::vector<LyricLine> out;

    // 逐行解析
    // 行形如:[mm:ss.xx]文本 ([mm:ss.xx])*
    auto tryParseTimestamp = [](const std::string& inside, double& outSec) -> bool {
        auto col = inside.find(':');
        if (col == std::string::npos || col == 0 || col >= inside.size() - 1) return false;
        // mm 全数字
        for (size_t i = 0; i < col; ++i) {
            if (!std::isdigit(static_cast<unsigned char>(inside[i]))) return false;
        }
        std::string rest = inside.substr(col + 1);
        // rest 仅含数字 / . / : (兼容 mm:ss:xx 写法)
        for (char c : rest) {
            if (!(std::isdigit(static_cast<unsigned char>(c)) || c == '.' || c == ':')) return false;
        }
        try {
            int mm = std::stoi(inside.substr(0, col));
            for (auto& c : rest) if (c == ':') c = '.';
            double ss = std::stod(rest);
            outSec = mm * 60.0 + ss;
            return true;
        } catch (...) { return false; }
    };

    size_t lineStart = 0;
    for (size_t i = 0; i <= raw.size(); ++i) {
        bool eol = (i == raw.size() || raw[i] == '\n' || raw[i] == '\r');
        if (!eol) continue;
        std::string line = raw.substr(lineStart, i - lineStart);
        lineStart = i + 1;
        if (line.empty()) continue;

        // 收集行首所有 [mm:ss.xx] 时间戳
        std::vector<double> times;
        size_t pos = 0;
        while (pos < line.size() && line[pos] == '[') {
            auto end = line.find(']', pos);
            if (end == std::string::npos) break;
            std::string inside = line.substr(pos + 1, end - pos - 1);
            double sec = 0.0;
            if (tryParseTimestamp(inside, sec)) {
                times.push_back(sec);
            }
            // 不是时间戳的(如 ti/ar/al) 会被跳过,但仍然要前进 pos
            pos = end + 1;
        }
        if (times.empty()) continue;

        std::string textRaw = line.substr(pos);
        // 去首尾空白
        while (!textRaw.empty() && (textRaw.back() == ' ' || textRaw.back() == '\t'
                                  || textRaw.back() == '\r')) textRaw.pop_back();
        size_t start = 0;
        while (start < textRaw.size() && (textRaw[start] == ' ' || textRaw[start] == '\t')) ++start;
        textRaw = textRaw.substr(start);

        std::wstring text = decode(textRaw);
        for (double t : times) {
            out.push_back({ t, text });
        }
    }

    std::sort(out.begin(), out.end(), [](const LyricLine& a, const LyricLine& b) {
        return a.time_sec < b.time_sec;
    });
    return out;
}

} // namespace apx
