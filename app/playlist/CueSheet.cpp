// =============================================================================
//  app/playlist/CueSheet.cpp
// =============================================================================
#include "CueSheet.h"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <sstream>

namespace apx {

namespace {

// 把 std::string (UTF-8 / 多字节) 粗略转 wstring。CUE 文件常用 UTF-8 (BOM) 或本地 ANSI;
// 这里只做 UTF-8 假设,真 ANSI 时可能拿到乱码 title — 但 path 名通常是 ASCII,够用。
std::wstring widen_utf8(const std::string& s)
{
    std::wstring w;
    w.reserve(s.size());
    size_t i = 0;
    while (i < s.size()) {
        unsigned char c = static_cast<unsigned char>(s[i]);
        if (c < 0x80) { w.push_back(static_cast<wchar_t>(c)); i += 1; }
        else if ((c & 0xE0) == 0xC0 && i + 1 < s.size()) {
            wchar_t cp = ((c & 0x1F) << 6)
                       | (static_cast<unsigned char>(s[i+1]) & 0x3F);
            w.push_back(cp); i += 2;
        }
        else if ((c & 0xF0) == 0xE0 && i + 2 < s.size()) {
            wchar_t cp = ((c & 0x0F) << 12)
                       | ((static_cast<unsigned char>(s[i+1]) & 0x3F) << 6)
                       | (static_cast<unsigned char>(s[i+2]) & 0x3F);
            w.push_back(cp); i += 3;
        }
        else if ((c & 0xF8) == 0xF0 && i + 3 < s.size()) {
            // 4-byte UTF-8 → surrogate pair;CUE 罕见,保守跳过
            w.push_back(L'?'); i += 4;
        }
        else { w.push_back(L'?'); i += 1; }
    }
    return w;
}

std::string trim(const std::string& s)
{
    size_t a = 0, b = s.size();
    while (a < b && (s[a] == ' ' || s[a] == '\t' || s[a] == '\r')) ++a;
    while (b > a && (s[b-1] == ' ' || s[b-1] == '\t' || s[b-1] == '\r')) --b;
    return s.substr(a, b - a);
}

// 解析 "TITLE "abc"" / "PERFORMER "abc"" / "FILE "abc.wav" WAVE" 中的双引号字符串
std::string extractQuoted(const std::string& line)
{
    auto a = line.find('"');
    if (a == std::string::npos) return {};
    auto b = line.find('"', a + 1);
    if (b == std::string::npos) return {};
    return line.substr(a + 1, b - a - 1);
}

// 从 path 拿目录(末尾不带 /\\),无目录则返回空串
std::wstring dirOf(const std::wstring& p)
{
    auto pos = p.find_last_of(L"\\/");
    if (pos == std::wstring::npos) return L"";
    return p.substr(0, pos);
}

std::wstring joinPath(const std::wstring& dir, const std::wstring& name)
{
    if (dir.empty()) return name;
    // name 已经是绝对路径就直接返回 (Windows 检查盘符 / UNC / 反斜杠开头)
    if (name.size() >= 2 && (name[1] == L':' || name[0] == L'\\' || name[0] == L'/'))
        return name;
    return dir + L'\\' + name;
}

} // namespace

double CueSheet::parseTimecode(const std::string& tc)
{
    int mm = 0, ss = 0, ff = 0;
    // 标准格式 MM:SS:FF
    if (sscanf_s(tc.c_str(), "%d:%d:%d", &mm, &ss, &ff) == 3) {
        if (mm < 0 || ss < 0 || ff < 0 || ss >= 60 || ff >= 75) return -1.0;
        return mm * 60.0 + ss + ff / 75.0;
    }
    // 兜底：两段 MM:SS（部分手写 cue 文件遗漏 FF 段）
    if (sscanf_s(tc.c_str(), "%d:%d", &mm, &ss) == 2) {
        if (mm < 0 || ss < 0 || ss >= 60) return -1.0;
        return mm * 60.0 + ss;
    }
    return -1.0;
}

std::vector<PlaylistItem> CueSheet::parse(const std::wstring& cue_path,
                                          std::wstring* err)
{
    std::vector<PlaylistItem> out;
    std::ifstream fs(cue_path);
    if (!fs) { if (err) *err = L"cannot open cue file"; return out; }

    const std::wstring base_dir = dirOf(cue_path);

    // 全局元数据(出现在 TRACK 之前的 TITLE/PERFORMER)
    std::wstring album_title;
    std::wstring album_artist;

    // 当前 FILE
    std::wstring cur_file_abs;

    // 当前正在累积的 TRACK (track_index>0 表示"激活中")
    PlaylistItem cur;
    bool         cur_active = false;

    auto flush = [&]() {
        if (!cur_active) return;
        // 在 push 之前回填一些缺失字段
        if (cur.album.empty())  cur.album  = album_title;
        if (cur.artist.empty()) cur.artist = album_artist;
        if (cur.path.empty())   cur.path   = cur_file_abs;
        out.push_back(std::move(cur));
        cur = PlaylistItem{};
        cur_active = false;
    };

    std::string line;
    while (std::getline(fs, line)) {
        line = trim(line);
        if (line.empty()) continue;
        if (line.size() >= 3 && line.compare(0, 3, "REM") == 0) continue;

        // 取首词
        std::string head;
        std::size_t sp = line.find_first_of(" \t");
        if (sp == std::string::npos) head = line;
        else                          head = line.substr(0, sp);
        // 大写头
        std::transform(head.begin(), head.end(), head.begin(),
            [](char c){ return static_cast<char>(::toupper(static_cast<unsigned char>(c))); });

        if (head == "FILE") {
            // 新 FILE 出现 → flush 前一个 TRACK
            flush();
            const std::string name = extractQuoted(line);
            cur_file_abs = joinPath(base_dir, widen_utf8(name));
        }
        else if (head == "TRACK") {
            flush();
            // "TRACK 01 AUDIO"
            int num = 0; char kind[16] = {0};
            sscanf_s(line.c_str(), "%*s %d %15s", &num,
                     kind, static_cast<unsigned>(sizeof(kind)));
            cur = PlaylistItem{};
            cur.track_index = static_cast<std::uint32_t>(num);
            cur.path        = cur_file_abs;
            cur_active = true;
        }
        else if (head == "TITLE") {
            const std::string q = extractQuoted(line);
            if (cur_active) cur.title = widen_utf8(q);
            else            album_title = widen_utf8(q);
        }
        else if (head == "PERFORMER") {
            const std::string q = extractQuoted(line);
            if (cur_active) cur.artist = widen_utf8(q);
            else            album_artist = widen_utf8(q);
        }
        else if (head == "INDEX") {
            // "INDEX 01 mm:ss:ff" (INDEX 00 是 pre-gap,忽略)
            int idxnum = 0; char tc[32] = {0};
            if (sscanf_s(line.c_str(), "%*s %d %31s", &idxnum,
                         tc, static_cast<unsigned>(sizeof(tc))) == 2) {
                if (idxnum == 1 && cur_active) {
                    const double t = parseTimecode(tc);
                    if (t >= 0) cur.cue_start_sec = t;
                }
            }
        }
        // 其它 (PREGAP/POSTGAP/ISRC/CATALOG/FLAGS/...) 忽略
    }
    flush();

    // 计算 cue_end_sec:每个 TRACK 的 end = 下一 TRACK 的 start(同 FILE 时);
    // 跨 FILE 或最后一项 → 留 0,表示 "到 EOF"
    for (std::size_t i = 0; i + 1 < out.size(); ++i) {
        if (out[i].path == out[i+1].path) {
            out[i].cue_end_sec = out[i+1].cue_start_sec;
        }
    }

    if (out.empty() && err) *err = L"cue parsed but no tracks found";
    return out;
}

} // namespace apx
