// =============================================================================
//  examples/cli_player_demo
//
//  交互式 CLI 播放器,验证 PlayerController 的完整状态机与回调。
//
//  命令:
//      load <path>              加载文件
//      unload                   卸载
//      play | pause | stop
//      seek <seconds>
//      device list              列出渲染设备
//      device default           恢复默认设备
//      device <id|name-substr>  切换设备
//      info                     当前状态摘要
//      progress on|off          切换 [POS] 周期打印(默认 on)
//      help
//      quit | exit
//
//  也可直接命令行 load:cli_player_demo D:\Music\song.wav
// =============================================================================

#include "app/controller/PlayerController.h"
#include "app/controller/PlayerState.h"
#include "platform/mmdevice/DeviceEnumerator.h"

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>
#include <vector>

namespace {

std::mutex g_print_mu;

void say(const std::wstring& s)
{
    std::lock_guard<std::mutex> lk(g_print_mu);
    std::fputws(s.c_str(), stdout);
    std::fputwc(L'\n', stdout);
    std::fflush(stdout);
}

void sayf(const wchar_t* fmt, ...)
{
    wchar_t buf[1024];
    va_list a; va_start(a, fmt);
    std::vswprintf(buf, 1024, fmt, a);
    va_end(a);
    say(buf);
}

const wchar_t* state_name(apx::DeviceState s)
{
    switch (s) {
    case apx::DeviceState::Active:     return L"Active";
    case apx::DeviceState::Disabled:   return L"Disabled";
    case apx::DeviceState::NotPresent: return L"NotPresent";
    case apx::DeviceState::Unplugged:  return L"Unplugged";
    }
    return L"?";
}

// 切分命令为 verb + remainder(remainder 保留首个空格之后的全部内容,含空格)
void split_first(const std::wstring& s, std::wstring& verb, std::wstring& rest)
{
    const auto p = s.find_first_of(L" \t");
    if (p == std::wstring::npos) { verb = s; rest.clear(); return; }
    verb = s.substr(0, p);
    rest = s.substr(p + 1);
    while (!rest.empty() && (rest.front() == L' ' || rest.front() == L'\t')) rest.erase(0, 1);
}

void trim(std::wstring& s)
{
    while (!s.empty() && (s.back()  == L'\n' || s.back()  == L'\r' || s.back()  == L' ' || s.back()  == L'\t')) s.pop_back();
    while (!s.empty() && (s.front() == L' '  || s.front() == L'\t')) s.erase(0, 1);
}

void print_help()
{
    say(L"命令:");
    say(L"  load <path>               加载文件(支持空格)");
    say(L"  unload");
    say(L"  play | pause | stop");
    say(L"  seek <seconds>            如 seek 60.5");
    say(L"  device list");
    say(L"  device default");
    say(L"  device <id 或 name 子串>");
    say(L"  info");
    say(L"  progress on|off");
    say(L"  help | quit | exit");
}

} // namespace

int wmain(int argc, wchar_t** argv)
{
    using namespace apx;

    PlayerController player;
    std::atomic<bool> show_progress{true};

    // ---------- 回调 ----------
    player.setOnStateChanged([](PlayerState s) {
        sayf(L"[STATE]    %s", to_wstring(s));
    });
    player.setOnPositionChanged([&](double sec) {
        if (!show_progress.load()) return;
        // 限频 1s 一次
        static auto last = std::chrono::steady_clock::now();
        auto now = std::chrono::steady_clock::now();
        if (now - last < std::chrono::milliseconds(950)) return;
        last = now;
        sayf(L"[POS]      %6.2fs / %6.2fs", sec, player.duration());
    });
    player.setOnEnded([] {
        say(L"[ENDED]    自然播放结束");
    });
    player.setOnError([](const std::wstring& e) {
        sayf(L"[ERROR]    %s", e.c_str());
    });

    // ---------- 命令行直接 load ----------
    if (argc >= 2) {
        std::wstring path = argv[1];
        if (player.loadFile(path)) {
            sayf(L"[ OK ] Loaded: %s", path.c_str());
        } else {
            sayf(L"[FAIL] %s", player.lastError().c_str());
        }
    }

    say(L"==== Audio Player X86 — CLI ====");
    say(L"输入 help 查看命令,quit 退出");

    wchar_t line[2048];
    while (true) {
        std::fputws(L"> ", stdout);
        std::fflush(stdout);
        if (!std::fgetws(line, 2048, stdin)) break;

        std::wstring cmd = line;
        trim(cmd);
        if (cmd.empty()) continue;

        std::wstring verb, rest;
        split_first(cmd, verb, rest);

        if (verb == L"quit" || verb == L"exit" || verb == L"q") {
            break;
        } else if (verb == L"help" || verb == L"?") {
            print_help();
        } else if (verb == L"load") {
            if (rest.empty()) { say(L"用法: load <path>"); continue; }
            // 去掉可能的成对引号
            if (rest.size() >= 2 && rest.front() == L'"' && rest.back() == L'"') {
                rest = rest.substr(1, rest.size() - 2);
            }
            if (player.loadFile(rest)) {
                const auto fmt = player.format();
                sayf(L"[ OK ] Loaded: %s", rest.c_str());
                sayf(L"       Format: %s", fmt.to_wstring().c_str());
                sayf(L"       Duration: %.2fs", player.duration());
                sayf(L"       Device: %s", player.currentDeviceName().c_str());
            } else {
                sayf(L"[FAIL] %s", player.lastError().c_str());
            }
        } else if (verb == L"unload") {
            player.unloadFile();
            say(L"[ OK ] unloaded");
        } else if (verb == L"play") {
            if (!player.play()) sayf(L"[FAIL] %s", player.lastError().c_str());
        } else if (verb == L"pause") {
            if (!player.pause()) sayf(L"[FAIL] (only valid in Playing)");
        } else if (verb == L"stop") {
            if (!player.stop()) sayf(L"[FAIL] (no media)");
        } else if (verb == L"seek") {
            if (rest.empty()) { say(L"用法: seek <seconds>"); continue; }
            double sec = std::wcstod(rest.c_str(), nullptr);
            if (!player.seek(sec)) sayf(L"[FAIL] %s", player.lastError().c_str());
        } else if (verb == L"device") {
            std::wstring sub_verb, dev_rest;
            split_first(rest, sub_verb, dev_rest);
            if (sub_verb.empty() || sub_verb == L"list") {
                DeviceEnumerator de;
                const auto list = de.listRenderEndpoints(true);
                const std::wstring active = player.currentDeviceId();
                int idx = 0;
                for (const auto& d : list) {
                    const wchar_t* mark = (d.id == active) ? L" ← active"
                                       : (d.is_default_console() ? L"   (DEFAULT)" : L"");
                    sayf(L"  [%02d] %-10s %s%s", idx++, state_name(d.state),
                         d.friendly_name.c_str(), mark);
                }
            } else if (sub_verb == L"default") {
                if (!player.setDevice(L"")) sayf(L"[FAIL] %s", player.lastError().c_str());
                else say(L"[ OK ] 已切换到默认设备");
            } else {
                // 当作 id 或 name 子串
                DeviceEnumerator de;
                std::wstring full = rest;          // sub_verb 本身可能就是值
                auto info = de.findById(full);
                if (!info) info = de.findByNameSubstring(full);
                if (!info) { sayf(L"[FAIL] 没找到设备: %s", full.c_str()); continue; }
                if (info->state != DeviceState::Active) {
                    sayf(L"[FAIL] 设备 %s 状态 %s,不可用",
                         info->friendly_name.c_str(), state_name(info->state));
                    continue;
                }
                if (!player.setDevice(info->id)) {
                    sayf(L"[FAIL] %s", player.lastError().c_str());
                } else {
                    sayf(L"[ OK ] 设备切换为: %s", info->friendly_name.c_str());
                }
            }
        } else if (verb == L"info") {
            sayf(L"  state    : %s", to_wstring(player.state()));
            sayf(L"  file     : %s",
                 player.currentFile().empty() ? L"<none>" : player.currentFile().c_str());
            sayf(L"  format   : %s", player.format().to_wstring().c_str());
            sayf(L"  duration : %.2fs", player.duration());
            sayf(L"  position : %.2fs", player.position());
            sayf(L"  device   : %s",
                 player.currentDeviceName().empty() ? L"<none>" : player.currentDeviceName().c_str());
            if (!player.lastError().empty())
                sayf(L"  lastErr  : %s", player.lastError().c_str());
        } else if (verb == L"progress") {
            if (rest == L"on")  show_progress.store(true);
            else if (rest == L"off") show_progress.store(false);
            else say(L"用法: progress on|off");
        } else {
            sayf(L"未知命令:%s(help 查看)", verb.c_str());
        }
    }

    player.unloadFile();
    say(L"再见");
    return 0;
}
