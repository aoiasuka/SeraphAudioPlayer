// =============================================================================
//  core/lyrics/LyricsLoader.cpp
// =============================================================================
#include "LyricsLoader.h"

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstring>
#include <unordered_map>

#ifdef _WIN32
#  ifndef WIN32_LEAN_AND_MEAN
#    define WIN32_LEAN_AND_MEAN
#  endif
#  include <windows.h>
#endif

namespace apx {

namespace {

// ---- 编码工具 ----

std::wstring utf8ToW(const char* p, size_t n)
{
#ifdef _WIN32
    if (n == 0) return {};
    int wn = MultiByteToWideChar(CP_UTF8, 0, p, static_cast<int>(n), nullptr, 0);
    if (wn <= 0) return {};
    std::wstring out(wn, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, p, static_cast<int>(n), out.data(), wn);
    return out;
#else
    return std::wstring(p, p + n);
#endif
}
std::wstring gbkToW(const char* p, size_t n)
{
#ifdef _WIN32
    if (n == 0) return {};
    int wn = MultiByteToWideChar(936, 0, p, static_cast<int>(n), nullptr, 0);
    if (wn <= 0) return {};
    std::wstring out(wn, L'\0');
    MultiByteToWideChar(936, 0, p, static_cast<int>(n), out.data(), wn);
    return out;
#else
    return std::wstring(p, p + n);
#endif
}

bool looksLikeUtf8(const char* p, size_t n)
{
    int multi = 0;
    for (size_t i = 0; i < n;) {
        unsigned char c = static_cast<unsigned char>(p[i]);
        if (c < 0x80) { ++i; continue; }
        int extra;
        if      ((c & 0xE0) == 0xC0) extra = 1;
        else if ((c & 0xF0) == 0xE0) extra = 2;
        else if ((c & 0xF8) == 0xF0) extra = 3;
        else return false;
        if (i + extra >= n) return false;
        for (int k = 1; k <= extra; ++k) {
            if ((static_cast<unsigned char>(p[i + k]) & 0xC0) != 0x80) return false;
        }
        i += extra + 1;
        ++multi;
    }
    return multi > 0;
}

// 把原始字节流(含可能的 BOM)转成 UTF-8 std::string,返回内容
std::string normalizeToUtf8(const std::string& raw)
{
    if (raw.empty()) return {};
    auto* p = reinterpret_cast<const unsigned char*>(raw.data());
    size_t n = raw.size();

    // UTF-8 BOM
    if (n >= 3 && p[0] == 0xEF && p[1] == 0xBB && p[2] == 0xBF) {
        return raw.substr(3);
    }
    // UTF-16 LE BOM
    if (n >= 2 && p[0] == 0xFF && p[1] == 0xFE) {
#ifdef _WIN32
        size_t wcount = (n - 2) / 2;
        const wchar_t* wp = reinterpret_cast<const wchar_t*>(raw.data() + 2);
        int u8n = WideCharToMultiByte(CP_UTF8, 0, wp, static_cast<int>(wcount),
                                      nullptr, 0, nullptr, nullptr);
        if (u8n <= 0) return {};
        std::string out(u8n, '\0');
        WideCharToMultiByte(CP_UTF8, 0, wp, static_cast<int>(wcount),
                            out.data(), u8n, nullptr, nullptr);
        return out;
#else
        return raw.substr(2);
#endif
    }
    // UTF-16 BE BOM — 先 swap
    if (n >= 2 && p[0] == 0xFE && p[1] == 0xFF) {
#ifdef _WIN32
        size_t wcount = (n - 2) / 2;
        std::wstring tmp(wcount, L'\0');
        for (size_t i = 0; i < wcount; ++i) {
            uint16_t hi = p[2 + i * 2];
            uint16_t lo = p[2 + i * 2 + 1];
            tmp[i] = static_cast<wchar_t>((hi << 8) | lo);
        }
        int u8n = WideCharToMultiByte(CP_UTF8, 0, tmp.data(), static_cast<int>(wcount),
                                      nullptr, 0, nullptr, nullptr);
        if (u8n <= 0) return {};
        std::string out(u8n, '\0');
        WideCharToMultiByte(CP_UTF8, 0, tmp.data(), static_cast<int>(wcount),
                            out.data(), u8n, nullptr, nullptr);
        return out;
#else
        return raw;
#endif
    }
    // 无 BOM:启发式 UTF-8 / 否则 GBK → UTF-8
    if (looksLikeUtf8(raw.data(), n)) return raw;
#ifdef _WIN32
    std::wstring w = gbkToW(raw.data(), n);
    int u8n = WideCharToMultiByte(CP_UTF8, 0, w.data(), static_cast<int>(w.size()),
                                  nullptr, 0, nullptr, nullptr);
    if (u8n <= 0) return raw;
    std::string out(u8n, '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.data(), static_cast<int>(w.size()),
                        out.data(), u8n, nullptr, nullptr);
    return out;
#else
    return raw;
#endif
}

// ---- IO ----

std::string readAll(const std::wstring& path)
{
    FILE* f = nullptr;
    if (_wfopen_s(&f, path.c_str(), L"rb") != 0 || !f) return {};
    _fseeki64(f, 0, SEEK_END);
    std::int64_t n = _ftelli64(f);
    _fseeki64(f, 0, SEEK_SET);
    if (n <= 0) { std::fclose(f); return {}; }
    if (n > 4 * 1024 * 1024) n = 4 * 1024 * 1024;
    std::string buf(static_cast<std::size_t>(n), '\0');
    std::fread(buf.data(), 1, static_cast<std::size_t>(n), f);
    std::fclose(f);
    return buf;
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

// 拼接候选歌词路径(按命中优先级)
std::vector<std::wstring> lrcCandidates(const std::wstring& audio_path)
{
    std::vector<std::wstring> out;
    auto dot   = audio_path.find_last_of(L'.');
    auto slash = audio_path.find_last_of(L"/\\");
    std::wstring stem = (dot == std::wstring::npos)
                            ? audio_path
                            : audio_path.substr(0, dot);
    std::wstring dir  = (slash == std::wstring::npos)
                            ? std::wstring()
                            : audio_path.substr(0, slash + 1);
    std::wstring base = stem.substr(dir.size());

    out.push_back(stem + L".lrc");        // 同名 .lrc
    out.push_back(stem + L".LRC");        // 大小写兼容(NTFS 不敏感, 但其它 FS / Wine 敏感)
    if (!dir.empty()) {
        out.push_back(dir + L"lyrics\\" + base + L".lrc");
        out.push_back(dir + L"lyrics/"  + base + L".lrc");
        out.push_back(dir + L"Lyrics\\" + base + L".lrc");
    }
    return out;
}

// ---- 时间戳解析 ----

// 解析 "mm:ss.xx" / "mm:ss:xx" / "h:mm:ss.xx";成功返回秒,失败返回 false
bool parseClock(const char* s, size_t n, double& out)
{
    if (n == 0) return false;
    // 全部为数字 / '.' / ':'
    for (size_t i = 0; i < n; ++i) {
        char c = s[i];
        if (!(std::isdigit(static_cast<unsigned char>(c)) || c == '.' || c == ':')) return false;
    }
    // 按 ':' 拆分,最后一段允许小数
    int parts[4] = {0,0,0,0};
    double frac = 0.0;
    int pc = 0;
    size_t i = 0;
    while (i < n && pc < 4) {
        size_t j = i;
        while (j < n && s[j] != ':') ++j;
        if (j == i) return false;
        // 这一段内可能含一个 '.'
        size_t dot = j;
        for (size_t k = i; k < j; ++k) if (s[k] == '.') { dot = k; break; }
        try {
            parts[pc] = std::stoi(std::string(s + i, dot - i));
            if (dot < j) {
                std::string fracStr(s + dot, j - dot);
                if (fracStr.size() == 1) return false;   // 只有点
                frac = std::stod(fracStr);
            }
        } catch (...) { return false; }
        ++pc;
        i = (j < n) ? j + 1 : j;
    }
    if (pc == 0) return false;
    double sec = 0.0;
    if (pc == 1)      sec = parts[0] + frac;
    else if (pc == 2) sec = parts[0] * 60.0 + parts[1] + frac;
    else              sec = parts[0] * 3600.0 + parts[1] * 60.0 + parts[2] + frac;
    out = sec;
    return true;
}

// 解析元数据标签 "ti:foo" → key=ti, val=foo;返回 false 表示不是 key:value
bool parseMetaTag(const char* s, size_t n, std::string& key, std::string& val)
{
    // 找冒号
    size_t col = std::string::npos;
    for (size_t i = 0; i < n; ++i) if (s[i] == ':') { col = i; break; }
    if (col == std::string::npos || col == 0) return false;
    // key 只允许字母
    for (size_t i = 0; i < col; ++i) {
        char c = s[i];
        if (!std::isalpha(static_cast<unsigned char>(c))) return false;
    }
    key.assign(s, col);
    val.assign(s + col + 1, n - col - 1);
    // 去 val 首尾空白
    while (!val.empty() && (val.back() == ' ' || val.back() == '\t')) val.pop_back();
    size_t k = 0;
    while (k < val.size() && (val[k] == ' ' || val[k] == '\t')) ++k;
    if (k > 0) val.erase(0, k);
    for (auto& c : key) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    return true;
}

void applyMeta(LyricMetadata& m, const std::string& key, const std::string& val)
{
    if (val.empty()) return;
    auto setW = [](std::wstring& dst, const std::string& s) {
        dst = utf8ToW(s.data(), s.size());
    };
    if      (key == "ti")     setW(m.title,  val);
    else if (key == "ar")     setW(m.artist, val);
    else if (key == "al")     setW(m.album,  val);
    else if (key == "by")     setW(m.by,     val);
    else if (key == "offset") {
        try { m.offset_ms = std::stod(val); } catch (...) {}
    } else if (key == "length") {
        double s = 0.0;
        if (parseClock(val.data(), val.size(), s)) m.length_sec = static_cast<int>(s);
    }
}

// 从主文本里抽出 <mm:ss.xx>word 序列,产出 (清洗后的文本, word_times)
// word_times 中 second 字段 = 该词起点在清洗后文本里的字符偏移
void extractInlineTimes(const std::string& utf8In,
                        std::wstring&      cleanedW,
                        std::vector<std::pair<double, int>>& word_times)
{
    word_times.clear();
    // 先扫一遍找所有 <...>
    std::string cleaned; cleaned.reserve(utf8In.size());
    size_t i = 0;
    while (i < utf8In.size()) {
        if (utf8In[i] == '<') {
            size_t end = utf8In.find('>', i);
            if (end != std::string::npos) {
                double sec = 0.0;
                if (parseClock(utf8In.data() + i + 1, end - i - 1, sec)) {
                    // 当前在 cleaned 末尾的字符偏移(以 wchar 数为单位)
                    std::wstring w = utf8ToW(cleaned.data(), cleaned.size());
                    word_times.emplace_back(sec, static_cast<int>(w.size()));
                    i = end + 1;
                    continue;
                }
            }
        }
        cleaned.push_back(utf8In[i]);
        ++i;
    }
    cleanedW = utf8ToW(cleaned.data(), cleaned.size());
}

void trimAscii(std::string& s)
{
    while (!s.empty() && (s.back() == ' ' || s.back() == '\t' || s.back() == '\r')) s.pop_back();
    size_t k = 0;
    while (k < s.size() && (s[k] == ' ' || s[k] == '\t')) ++k;
    if (k > 0) s.erase(0, k);
}

} // namespace

// ---- LyricsLoader ----

LyricsDoc LyricsLoader::parseDoc(const std::string& raw_in)
{
    LyricsDoc doc;
    if (raw_in.empty()) return doc;
    std::string raw = normalizeToUtf8(raw_in);
    if (raw.empty()) return doc;

    // 同时间戳聚合:第 1 次 → text, 第 2+ 次 → translation(用 \n 拼)
    // 因为时间戳可能精度不同(eg 12.30 vs 12.300),用四舍五入到 ms 做 key
    std::unordered_map<long long, size_t> ts_to_index;

    size_t lineStart = 0;
    for (size_t i = 0; i <= raw.size(); ++i) {
        bool eol = (i == raw.size() || raw[i] == '\n' || raw[i] == '\r');
        if (!eol) continue;
        std::string line = raw.substr(lineStart, i - lineStart);
        lineStart = i + 1;
        if (line.empty()) continue;

        // 行首所有 [...]:可能是 timestamp,或 ti/ar/al/offset/by/length 等
        std::vector<double> times;
        size_t pos = 0;
        while (pos < line.size() && line[pos] == '[') {
            size_t end = line.find(']', pos);
            if (end == std::string::npos) break;
            const char* inside = line.data() + pos + 1;
            size_t      inlen  = end - pos - 1;
            double      sec    = 0.0;
            if (parseClock(inside, inlen, sec)) {
                times.push_back(sec);
            } else {
                std::string k, v;
                if (parseMetaTag(inside, inlen, k, v)) applyMeta(doc.meta, k, v);
            }
            pos = end + 1;
        }
        if (times.empty()) continue;

        std::string textRaw = line.substr(pos);
        trimAscii(textRaw);

        // 抽取词级时间戳,得到 cleaned 文本 + word_times
        std::wstring                                cleanedW;
        std::vector<std::pair<double, int>>         words;
        extractInlineTimes(textRaw, cleanedW, words);

        for (double t : times) {
            long long key = static_cast<long long>(t * 1000.0 + 0.5);
            auto it = ts_to_index.find(key);
            if (it == ts_to_index.end()) {
                LyricLine ln;
                ln.time_sec   = t;
                ln.text       = cleanedW;
                ln.word_times = words;
                doc.lines.push_back(std::move(ln));
                ts_to_index.emplace(key, doc.lines.size() - 1);
            } else {
                LyricLine& ln = doc.lines[it->second];
                if (ln.translation.empty()) ln.translation = cleanedW;
                else { ln.translation.append(L"\n"); ln.translation.append(cleanedW); }
            }
        }
    }

    // offset:正值 → 歌词提前显示 → time -= offset/1000
    if (doc.meta.offset_ms != 0.0) {
        const double off = doc.meta.offset_ms / 1000.0;
        for (auto& ln : doc.lines) {
            ln.time_sec -= off;
            for (auto& w : ln.word_times) w.first -= off;
        }
    }

    std::sort(doc.lines.begin(), doc.lines.end(),
              [](const LyricLine& a, const LyricLine& b) { return a.time_sec < b.time_sec; });
    // 钳到 ≥ 0(行级 + 词级一起;否则负的词级会卡住高亮永远到不了)
    for (auto& ln : doc.lines) {
        if (ln.time_sec < 0) ln.time_sec = 0;
        for (auto& w : ln.word_times) if (w.first < 0) w.first = 0;
    }

    return doc;
}

LyricsDoc LyricsLoader::loadDoc(const std::wstring& lrc_path)
{
    return parseDoc(readAll(lrc_path));
}

LyricsDoc LyricsLoader::loadDocFor(const std::wstring& audio_path)
{
    for (const auto& p : lrcCandidates(audio_path)) {
        if (!fileExists(p)) continue;
        auto doc = loadDoc(p);
        if (!doc.empty()) return doc;
    }
    return {};
}

std::vector<LyricLine> LyricsLoader::loadFor(const std::wstring& audio_path)
{
    return loadDocFor(audio_path).lines;
}
std::vector<LyricLine> LyricsLoader::load(const std::wstring& lrc_path)
{
    return loadDoc(lrc_path).lines;
}

} // namespace apx
