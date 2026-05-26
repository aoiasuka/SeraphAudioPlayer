# AudioPlayerX86 项目问题清单

> 静态筛查时间：2026-05-26
> 修复轮次：2026-05-26（Critical + High + Medium + Low 全部 4 个级别完成）
> 范围：`ui/`(QML + Bridge) / `core/` / `app/` / `platform/` / 根 CMake 与 vcpkg
> 形式：只读静态分析，未执行运行时验证；严重级 = Critical / High / Medium / Low
> 编号规则：`[<域>-<序号>]`，域 = QML / VM / CORE / PLAT / BUILD / STRUCT

## 状态图例

| 标记 | 含义 |
|------|------|
| ✅ | 已修复（本轮提交） |
| 🔍 | 复查后判定为误报 / 非 bug，未改代码 |
| ⏭️ | 本轮跳过（设计性 / 结构性重构 / 影响面巨大，待后续单独立项） |
| ⏸️ | 本轮未处理（Medium / Low 优先级，留待后续） |

## 本轮修复总览（全部 55 项 Critical + High + Medium + Low）

**Critical 8 项**：✅×7、🔍×1
- ✅ BUILD-1 / BUILD-2 / BUILD-3 / CORE-1 / PLAT-1 / PLAT-2 / VM-1
- 🔍 CORE-2（leftover 顺序复查后实际正确）

**High 18 项**：✅×11、🔍×3、⏭️×4
- ✅ QML-2 / VM-2 / VM-5 / CORE-3 / CORE-4 / CORE-5 / CORE-6 / CORE-7 / PLAT-5 / PLAT-6 / PLAT-8 / BUILD-4 / BUILD-5
- 🔍 VM-4（/utf-8 已配置）/ PLAT-3（已正确 goto Fail）/ PLAT-7（discard 是刻意逻辑）
- ⏭️ QML-1（window 改名影响面巨大）/ QML-3（Theme 单例重构）/ VM-3（model 增量改造）/ PLAT-4（DeviceEnumerator 锁结构改动）

**Medium 19 项**：✅×15、⏭️×4
- ✅ QML-4 / VM-6 / VM-7 / VM-8 / CORE-8 / CORE-9 / CORE-10 / CORE-11 / CORE-14 / PLAT-10 / PLAT-12 / BUILD-6 / BUILD-7 / BUILD-8 / BUILD-9
- ⏭️ CORE-12（PolyphaseResampler memmove 优化需重写 history 状态机）/ CORE-13（RingBuffer SPSC 运行时断言需 DEBUG 线程 ID 跟踪机制）/ PLAT-9（已有 monitor_loop 100ms tick 粒度退避）/ PLAT-11（SMTC 跨公寓 release 需架构改动）

**Low 10 项**：✅×6、🔍×3、⏭️×1
- ✅ STRUCT-1 / CORE-16 / PLAT-13 / PLAT-14 / BUILD-10 / VM-9
- 🔍 QML-5（实际在 main.qml:366 被用）/ QML-6（QML alias 语法限制，readonly property 是正确写法）/ CORE-15（dr_flac 入库非可选下载，差异有意）
- ⏭️ BUILD-11（add_compile_definitions 改为 target 级需要 interface library 重构）

---

## Critical（构建即失败 / 必崩 / 功能完全缺失）

### ✅ [BUILD-1] `vcpkg.json` 中 `builtin-baseline` 不是合法 commit hash
**修复**：删除 `vcpkg.json:8` 的 `builtin-baseline` 字段（manifest 模式下可省略，vcpkg 会用安装目录自身的 ports）。
- **文件**：`vcpkg.json:8`
- **现状**：`"builtin-baseline": "2024-01-01"`（日期字符串）
- **影响**：vcpkg manifest 模式启动即报 `error: invalid builtin-baseline`，无法完成依赖装配。所有 vcpkg 声明的依赖（spdlog / nlohmann-json / opusfile / gtest / ffmpeg…）均无法解析。
- **应为**：40 位 SHA（如 `vcpkg` 仓库 master 某次提交的 sha）。

### ✅ [BUILD-2] ASIO 真实路径永远不会被编译
**修复**：在 `platform/CMakeLists.txt` 的 SDK 探测分支末尾加 `target_compile_definitions(apx_platform PRIVATE APX_HAVE_ASIO_SDK=1)`，使宏作用域与目标自包含。
- **文件**：`platform/CMakeLists.txt:34-44`
- **现状**：检测到 `third_party/asiosdk/common/asio.h` 时只追加了 `target_include_directories` 和 SDK 的 `.cpp`，但 **没有把 `APX_HAVE_ASIO_SDK=1` 设到该 target 上**。该宏只在根 `CMakeLists.txt:138` 用 `add_compile_definitions` 全局设；而根 CMake 是先执行（line 134-143）再 `add_subdirectory(platform)`（line 175），按 CMake 语义全局宏应当能传到子目录。**但当前 `third_party/asiosdk/` 目录不存在**（已核实），所以根 CMake 永远走 `else()` 分支，宏永远不定义，`AsioOutput.cpp:19` 的 `#if APX_HAVE_ASIO_SDK` 永远为假。
- **影响**：即使用户按 `docs/ASIO.md` 放置 SDK 后再次配置，整个 ASIO 真实输出仍然只是桩。
- **建议**：在 `platform/CMakeLists.txt` 的 SDK 探测分支里用 `target_compile_definitions(apx_platform PRIVATE APX_HAVE_ASIO_SDK=1)` 自包含。

### ✅ [CORE-1] `MfMediaDecoder.cpp` 中 `CoInitializeEx` 返回 `S_FALSE` 时漏配对 `CoUninitialize`
**修复**：`MfMediaDecoder.cpp:116` 把 `com_init = (hr == S_OK)` 改为 `com_init = true`（`SUCCEEDED(hr)` 已含 S_OK 与 S_FALSE）。
- **文件**：`core/decoder/MfMediaDecoder.cpp:114-117`
- **现状**：只在 `hr == S_OK` 时记 `com_init = true`；但 `S_FALSE`（"此线程已被初始化"）按 COM 规则**仍需配对** `CoUninitialize`。
- **影响**：每次播放 MF 解码格式（多数 AAC / ALAC 等）都会让该线程 COM 引用计数 +1 而不 -1，长时间运行后 COM 子系统状态紊乱、可能影响 SMTC/WinRT 关闭。

### 🔍 [CORE-2] `MfMediaDecoder` leftover 缓冲顺序错乱
**复查结论**：非 bug。同一 `ReadSample` 内 dst 内容（含 tail 末尾）在前、`leftover` 在后，两者时间序：tail < leftover；代码 `tail.insert(tail.end(), leftover.begin+pos, leftover.end())` 顺序正确，与时间序一致。**未改动**。
- **文件**：`core/decoder/MfMediaDecoder.cpp:285-293`
- **现状**：先 `leftover.assign(p + copy, p + len)`，随后又把 `dst[aligned..written]` 拷到新 vector 与 leftover 合并，导致**先到的样本被压在新 tail 之后**。
- **影响**：尾部播放偶发咔哒、声道错位。

### ✅ [PLAT-1] ASIO 回调内堆分配 + 取互斥锁，违反实时禁忌
**最小补丁**：在 `Impl` 加成员 `std::vector<std::uint8_t> scratch`，`open()` 中按 `prefSize * ch * 4` 预分配；回调改用 `impl->scratch.data()` + `memset(0)`。锁保留（DataCallback 副本拷贝仍需）；完全无锁的双缓冲改造留待后续结构性重构。
- **文件**：`platform/asio/AsioOutput.cpp:67-86`
- **现状**：`onBufferSwitch`（ASIO 驱动线程的实时回调）内部 `std::vector<std::uint8_t> tmp(bytes, 0)` 每帧堆分配；并 `std::lock_guard<std::mutex>`；下游 RingBuffer 回调也走 mutex。
- **影响**：在专业声卡低 buffer（64/128 sample）下必然出现 xrun / 爆音 / 驱动主动 reset。
- **建议**：`open()` 中预分配 scratch，回调里改用无锁 atomic + 双缓冲。

### ✅ [PLAT-2] ASIO 全局单例 `g_active` 存在 use-after-free 风险
**修复**：`g_active` 改为 `std::atomic<AsioOutput::Impl*>`；`start()` 用 `store(release)`；`stop()/close()` 用 `compare_exchange_strong` 避免误清。`g_drivers` 未 delete 的隐患仍存在（进程生命周期单例，影响有限），未改动。
- **文件**：`platform/asio/AsioOutput.cpp:53, 235, 254`
- **现状**：`g_active`、`g_drivers` 是裸全局指针；`new AsioDrivers()` 永不 `delete`；`stop()` 直接 `g_active = nullptr`，但驱动线程仍可能正在 `onBufferSwitch` 中触达 `g_active` 当时的实例。
- **影响**：关闭 ASIO 设备瞬间可能崩溃；多 AsioOutput 实例并存时互相覆盖。

### ✅ [VM-1] `PlayerViewModel` 的元数据/颜色缓存跨线程读写无锁
**修复**：`PlayerViewModel.h` 加 `mutable QMutex m_cacheMutex`；`fetchMeta()` 重写为"查表持锁→读盘释锁→写表再持锁"两段式；`currentDominantColor()` 所有 `m_colorCache` 访问加 `QMutexLocker`；`exportPlaylistM3U/Json` 与 `importCueSheet` 对 `m_metaCache` 的访问也加锁（导出走 QMap COW 快照）。
- **文件**：`ui/bridge/PlayerViewModel.cpp:1199` 等（`m_metaCache` / `m_metaMissed` / `m_colorCache`）
- **现状**：声明 `mutable` 后在标 `const` 的 getter（如 `currentDominantColor()`）里写入；同时 `fetchMeta()`、`itemsFromPaths()` 也写入。这些路径**可能在 UI 线程与解码 / Tap 回调线程**同时触发，没有 mutex。
- **影响**：竞争窗口虽小但确实存在，长跑会出现哈希表崩、map 节点损坏。

### ✅ [BUILD-3] `MfMediaDecoder` 隐式 `#pragma comment(lib,...)` 链接系统库
**修复**：在 `core/CMakeLists.txt` 显式 `target_link_libraries(apx_core PUBLIC mfplat mfreadwrite mfuuid propsys)`（WIN32 分支）。
- **文件**：`core/decoder/MfMediaDecoder.cpp:27-30`
- **现状**：用 `#pragma comment(lib, "mfplat.lib")` 等隐式链接，而 `platform/CMakeLists.txt` 与根 CMake 都没有显式 `target_link_libraries` 这些库。
- **影响**：当前 MSVC 默认行为 OK；但只要某子项目（如 examples 中的 wasapi demo）启用 `/NODEFAULTLIB` 或裁剪 obj，就会以 `LNK2019: unresolved external symbol __imp_MFCreateSourceReaderFromURL` 形式链接失败。
- **建议**：在 `core/CMakeLists.txt` 显式 `target_link_libraries(apx_core PUBLIC mfplat mfreadwrite mfuuid propsys)`。

---

## High（运行期错误 / 数据正确性 / 资源泄漏）

### ⏭️ [QML-1] `id: window` 与 `QtQuick.Window` 类型同名导致名字解析歧义
**跳过原因**：改名 `window→appWindow` 涉及几乎所有 component 文件（`window.brand`、`window.fontFamily`、`window.textPrimary` 等数十/上百处），超出"最小补丁"范围；建议下一轮单独立项做全局替换 + Theme 单例化。
- **文件**：`ui/qml/main.qml:10`
- **现状**：`id: window`，但同文件 import 了 `QtQuick.Window`，并在 `main.qml:39, 206` 等位置写 `window.visibility !== Window.Maximized`。
- **影响**：QML 编译器虽容忍，但工具链（qmllint、qmlcachegen、QtCreator 索引）会告警；任何子组件中 `window.xxx` 在编译期都需要先找 id 再回退到类型——脆弱。
- **建议**：把 id 重命名为 `appWindow`。注意所有 component 里都用了 `window.brand` / `window.fontFamily`，改名牵涉面较大。

### ✅ [QML-2] `components/` 内的 `PlaceholderPage.qml` 再次 `import "../components"`
**修复**：删除 `PlaceholderPage.qml:3` 的 `import "../components"`（同目录文件 QML 直接可见，无需 import；`AppIcon` 同在 `components/` 下可直接使用）。
- **文件**：`ui/qml/components/PlaceholderPage.qml:3`
- **现状**：自己所在目录又 import 一次，属循环/冗余 import。
- **影响**：qmllint / qmlcachegen 会告警，部分 Qt 版本可能直接拒绝。

### ⏭️ [QML-3] 大量组件直接耦合 `main.qml` 的 `id: window`
**跳过原因**：与 QML-1 同源，需要抽 `Theme` 单例（`pragma Singleton`）并改造所有 component。结构性设计性重构，本轮跳过。
- **文件**：`ui/qml/components/SpectrumView.qml:18`、`HeroBanner.qml`、`Sidebar.qml`、`TrackRow.qml` 等几乎所有 component
- **现状**：直接写 `window.brand`、`window.fontFamily`、`window.textPrimary`…
- **影响**：组件无法独立测试 / 在 ApplicationWindow 之外不能使用 / 主题切换的所有 token 都耦合到全局 id。
- **建议**：抽出 `Theme` 单例（QML pragma Singleton）。

### ✅ [VM-2] `currentDominantColor` 的 NOTIFY 复用 `currentCoverUrlChanged`，语义错位
**修复**：`PlayerViewModel.h` 新增 `currentDominantColorChanged()` 信号；`Q_PROPERTY` 改为 `NOTIFY currentDominantColorChanged`；`currentDominantColor()` 在主色计算并写入缓存后，通过新增的私有 `emitDominantColorChanged()` 桥（const getter 内不能直接 emit）发射信号。
- **文件**：`ui/bridge/PlayerViewModel.h:54` / `.h:66`
- **现状**：`Q_PROPERTY(QColor currentDominantColor READ ... NOTIFY currentCoverUrlChanged)`。
- **影响**：当 URL 未变但 `m_colorCache` 因为元数据补全而真正算出新色时，QML 端绑定不会更新——颜色"卡住"在第一次加载值。
- **建议**：新增 `currentDominantColorChanged()` 信号，在颜色缓存命中并真正变化时发射。

### ⏭️ [VM-3] `PlaylistViewModel::setItems` 总是 `beginResetModel/endResetModel`
**跳过原因**：改为最小变更（`beginInsertRows` / `beginRemoveRows` / `dataChanged`）需要做 diff 算法或保留旧数据再比对，超出"最小补丁"。当前 reset 行为功能正确（仅性能/滚动位置体验问题），本轮跳过。
- **文件**：`ui/bridge/PlaylistViewModel.cpp:62-65`
- **影响**：QML ListView 滚动位置每次 setItems 都会被重置；列表大时全量重建 delegate 也有可见卡顿。
- **建议**：用 `beginInsertRows` / `beginRemoveRows` 做最小变更。

### 🔍 [VM-4] ShortcutsViewModel 用了中文字面量，构建对 `/utf-8` 强依赖
**复查结论**：根 `CMakeLists.txt:34` 已经 `add_compile_options(/W4 /permissive- /utf-8 /Zc:__cplusplus)`，所有子目标继承。当前不存在乱码风险。**未改动**。
- **文件**：`ui/bridge/ShortcutsViewModel.cpp:14-40`
- **现状**：源文件包含中文 `QString` 字面量；如果 MSVC 没传 `/utf-8` 或文件没有 BOM，会按 ACP（GBK）解码再以 Latin1 当 UTF-8 存入 `QString`，最终乱码。
- **现状二**：根 CMakeLists.txt:34 已加 `/utf-8`，但 examples / tests 子项目若被开启时建议同步。

### ✅ [VM-5] `PlayerViewModel` / `ShortcutsViewModel` / `Qt 引擎` 的析构顺序
**修复**：`ui/main.cpp` 把 `shortcutsVM` 声明提前到 `engine` 之前。栈析构顺序（反序）现在是 engine → shortcutsVM → playerVM → app，确保 engine（持有 QML binding）先于其引用的 VM 销毁。
- **文件**：`ui/main.cpp:163-164`
- **现状**：`playerVM` 与 `shortcutsVM` 在栈上，析构顺序由 main scope 决定；QML 引擎析构时若仍有 binding 引用已销毁 VM，会触发未定义行为。当前 Qt 通常容忍，但不可指望。
- **建议**：显式 `engine.reset()` 早于 VM 析构。

### ✅ [CORE-3] `FormatConverter.cpp:19` / `Equalizer.cpp:18` / `Visualizer.cpp:54` 24bit→32bit 符号扩展 UB
**修复**：`FormatConverter.cpp:14-21` `read_int24_packed` 改为 `uint32_t` 中转 + `static_cast<int32_t>` 出口。复查 `Equalizer.cpp:18-19`（`s24To32`）与 `Visualizer.cpp:53-58`（`s24To32`）实际已经用 `uint32_t` 中转，**非 bug**，未改动；agent 标错。
- **现状**：`v |= 0xFF000000` 把负数直接赋给 `int32_t`，C++ 标准下有符号溢出 UB。
- **建议**：经 `uint32_t` 中转再 `static_cast<int32_t>`。

### ✅ [CORE-4] `PolyphaseResampler.cpp:22` 预处理表达式缺括号
**修复**：把 `_M_IX86_FP >= 2 || defined(__SSE2__)` 改为 `(defined(_M_IX86_FP) && _M_IX86_FP >= 2) || defined(__SSE2__)`，避免 `&&/||` 优先级歧义。
- **现状**：`defined(_M_IX86_FP) && _M_IX86_FP >= 2 || defined(__SSE2__)`，`&&` 优先于 `||`。
- **影响**：表达式语义脆弱；x86 32-bit MSVC 默认未开 `/arch:SSE2` 时可能误认为可用 SSE2 → 老 CPU 上非法指令。
- **建议**：加括号 `((defined(_M_IX86_FP) && _M_IX86_FP >= 2) || defined(__SSE2__))` 并加运行时 CPUID 探测。

### ✅ [CORE-5] `Mp3Decoder.cpp:131-134` / `VorbisDecoder.cpp:128` / `LyricsLoader.cpp:144` 用 `std::ftell` 返回 long
**修复**：三处 `fseek/ftell` 改为 `_fseeki64/_ftelli64`，`long file_size` 改为 `std::int64_t`；`LyricsLoader.cpp` 同步把 `std::string buf(n, '\0')` 的 size 强转 `size_t`。
- **影响**：32-bit 进程下 long = 32-bit，大于 2 GB 的音频文件偏移会被截断或返回 -1，导致 `kSyncScanThresholdBytes` 之类的判定错乱。
- **建议**：改 `_ftelli64` / `_fseeki64`。

### ✅ [CORE-6] `DffDecoder.cpp:181` 直接用 chunk size 申请 `std::vector<uint8_t>(sz)`
**修复**：`DffDecoder.cpp:179-186` 在 `vector` 申请前加 `if (sz == 0 || sz > 16 MiB) return fail(...)`。`M4aDecoder` 实际只是 `MfMediaDecoder` 的薄包装（无独立 atom 解析），不需对应改动。
- **现状**：`sz` 来自 `readU64BE`，无上限校验；恶意/损坏 DFF 可造 OOM 拒绝服务（同问题存在于 `M4aDecoder` 嵌入歌词读取处）。
- **建议**：对 `sz` 设上限（如 16 MiB）。

### ✅ [CORE-7] `DsdDecoder.cpp:216` data_size 计算可能无符号下溢
**修复**：拆成 `rawDataSize`/`dataSize` 两步；若 `rawDataSize < 12` 直接 `return fail(L"DSF: data chunk size < 12")`，避免下溢成天文数字。
- **现状**：`readU64LE(dataHdr+4) - 12`，未校验左操作数 ≥ 12。
- **建议**：先校验 size ≥ 12，否则视为损坏文件。

### 🔍 [PLAT-3] WasapiExclusiveOutput 对齐重试路径未把 `hr` 设为 fail
**复查结论**：非 bug。`line 325` `Activate` 失败时 `hr` 携带失败 HRESULT → `line 327 if (SUCCEEDED(hr))` 跳过 Initialize → `line 335 if (FAILED(hr)) goto Fail` 正确收口，不会触及 `line 337` 的 `GetBufferSize`。**未改动**。
- **文件**：`platform/wasapi/WasapiExclusiveOutput.cpp:317-335`
- **现状**：`AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` 重试时若 `Activate` 失败，未跳到 Fail；后续 `client->GetBufferSize` 等会在 null/未初始化 client 上调用。
- **影响**：罕见但发生时即崩。

### ⏭️ [PLAT-4] `DeviceEnumerator` 注销与回调存在 race
**跳过原因**：要把 `detach()` 写入改成 mutex 保护，需要在 `Impl` 加锁结构、回调侧 `dispatch_*` 全部改造，改动面较大；当前依赖 `Unregister` 同步语义实际不易触发。本轮跳过，待后续单独立项。
- **文件**：`platform/mmdevice/DeviceEnumerator.cpp:362-373` 等
- **现状**：unregister 顺序为 `Unregister → detach → Release`；`detach()` 把 `owner_` raw 指针置空是无锁写。
- **影响**：理论上 Unregister 返回后 WASAPI 不再投递回调，安全；但 `detach` 写无锁，强依赖 Unregister 同步语义，文档无保证。
- **建议**：在 Impl 中加 mutex，回调侧也加锁取 `owner_`。

### ✅ [PLAT-5] PlayerController monitor_loop 与 stop 路径无 ctrl_mutex 协作
**修复**：`PlayerController.cpp:560-580` EOF→Ended 分支在 sleep 之后用 `std::unique_lock<std::mutex> lk(ctrl_mutex, std::try_to_lock)` 拿锁；拿不到本次 tick 让出（`continue`）；拿到后还要复检 `state.load() == Playing` 才推 Ended，避免覆盖用户线程已写入的 Stopped/Idle。
- **文件**：`app/controller/PlayerController.cpp:557-572`
- **现状**：monitor 进入 EOF → sleep buffer_ms+30 → 写状态 Ended；此时若 ctrl 线程调 stop 并写 Stopped，monitor 的 Ended 会覆盖。
- **影响**：偶发"明明已停止却变 Ended"或反之，影响状态机一致性。

### ✅ [PLAT-6] `Playlist.cpp:104` 出现 no-op 但显然写错的语句
**修复**：删除 `Playlist.cpp:104` 的 `if (candidates.empty()) candidates.assign(n, 0), candidates.clear();` 死代码，后续 `if (candidates.empty())` 分支保持不变。
- **现状**：`if (candidates.empty()) candidates.assign(n, 0), candidates.clear();`
- **影响**：紧随其后的 `if (candidates.empty())` 分支永远跟之前一样空。是死代码 + 误导逻辑。建议直接删。

### 🔍 [PLAT-7] `PlaylistIO::loadM3U` 首行解析会重复添加
**复查结论**：非 bug。`fs.seekg(0); std::getline(fs, discard)` 是刻意"跳过首行"——首行内容已在 `line_u8` 中被处理（识别为 EXTM3U 或加入路径），随后 `discard` 把它从流位置消费掉，让 `while` 从第二行开始。**未改动**。
- **文件**：`app/playlist/PlaylistIO.cpp:269-287`
- **现状**：先 `seekg(0); getline(fs, discard)` 读了一行（用于判 EXTM3U），然后又回到主 while 循环重新解析首行——若首行不是 BOM 也不是 `#`，会被 utf8 转换后作为路径加入。
- **影响**：每个非 EXTM3U 的 m3u 文件，首行会被重复读两次进列表。

### ✅ [PLAT-8] `PlaylistIO` JSON `\uXXXX` 解析不处理 surrogate pair
**最小补丁**：`PlaylistIO.cpp:144-159` 的 `case 'u'` 分支，对 `0xD800..0xDFFF` 区间的孤立 surrogate 直接替换为 `U+FFFD`（替换字符），避免写出非法 UTF-8。真正的 surrogate pair 前看下一个 `\uXXXX` 展开未实现（注释说明）。
- **文件**：`app/playlist/PlaylistIO.cpp:144-159`
- **影响**：BMP 外字符（emoji、扩展汉字）会被切成两个无效 UTF-8 码元 → 文件名编码错乱。

### ✅ [BUILD-4] 全局 `include_directories` 污染所有子目标
**修复**：`CMakeLists.txt:11` 的 `include_directories(${CMAKE_CURRENT_SOURCE_DIR})` 注释掉，并加注释说明各子目标已通过 `target_include_directories(... PUBLIC ${CMAKE_SOURCE_DIR})` 自带 include 树。保留注释而非直接删除，便于回滚验证。
- **文件**：根 `CMakeLists.txt:11`
- **现状**：`include_directories(${CMAKE_CURRENT_SOURCE_DIR})` 是全局 directory 级。
- **影响**：所有 add_subdirectory 引入的目标（包括第三方）都会带上整库 include 树；与各 `target_include_directories(... PUBLIC ${CMAKE_SOURCE_DIR})` 重复。
- **建议**：删掉这一行，全部依赖 PUBLIC target_include。

### ✅ [BUILD-5] 未校验生成器平台是否为 Win32
**修复**：在根 `CMakeLists.txt` 默认 Release 段后追加 `if(CMAKE_GENERATOR MATCHES "Visual Studio" AND DEFINED CMAKE_GENERATOR_PLATFORM AND NOT CMAKE_GENERATOR_PLATFORM STREQUAL "Win32") message(WARNING ...)`，提示用户漏传 `-A Win32`。
- **文件**：根 `CMakeLists.txt:23`
- **现状**：注释要求 `-A Win32`，但没有 `if(NOT CMAKE_GENERATOR_PLATFORM STREQUAL "Win32")` 校验。
- **影响**：用户忘传 `-A Win32` → 得到 x64 构建并加载 x64 Qt，配置看似成功但运行不对位；项目名 `_audio_player_x86` 明示意图为 32-bit。

---

## Medium（功能边角 / 性能 / 维护性 / 一致性）

### ✅ [QML-4] `WaveformProgressBar` 在 Canvas 内监听外部 trackKey
**修复**：在 Canvas 内加 `property string lastTrackKey`，`buildBaseline()` 入口先比对 `root.trackKey === lastTrackKey && envelope.length > 0` 命中即剪枝，避免外部对同一 trackKey 反复 binding 时的重算。
- **文件**：`ui/qml/components/WaveformProgressBar.qml:113-116`
- **现状**：用 `Connections { target: root; function onTrackKeyChanged() {...} }`，语法合法；但 `NowPlayingView.qml:681` 通过 `trackKey: root.trackKey` 频繁重绑定，每次都会触发 `buildBaseline + requestPaint`。
- **影响**：性能浪费。建议缓存最近一次 trackKey 并比较。

### ✅ [VM-6] PlayerViewModel 信号命名以下划线开头
**修复**：把 4 个跨线程内部信号 `_coreStateChanged / _corePositionChanged / _coreEnded / _coreError` 重命名为 `coreStateChangedInternal / corePositionChangedInternal / coreEndedInternal / coreErrorInternal`，连同对应 `.cpp` 中的 connect 与 emit 一并更新。
- **文件**：`ui/bridge/PlayerViewModel.cpp:55-58`
- **现状**：`_coreError`、`_coreEnded` 等以 `_` 开头并被 `connect(... Qt::QueuedConnection)` 投递。
- **影响**：违反 Qt 约定（`_` 在 Qt 中通常是内部/私有暗示）；moc 不会拒绝但 IDE 提示混乱。

### ✅ [VM-7] CoverImageProvider 是非 LRU 的"任意 erase begin"
**修复**：在 `CoverImageProvider.h` 加 `QList<QString> lru_`；`requestImage()` 命中缓存时把项挪到 lru_ 末尾；满时从 lru_ 头部驱逐最久未访问项。`kMaxCache` 由 32 提到 64。`clearCache()` 同步清 lru_。
- **文件**：`ui/bridge/CoverImageProvider.cpp:50-54`
- **现状**：`cache_.erase(cache_.begin())` 用于满时驱逐；hash map 的 begin 是哈希桶序。
- **影响**：可能反复驱逐刚装入的项。`kMaxCache=32`(.h:35) 偏小，大歌单反复读盘。

### ✅ [VM-8] CoverImageProvider 未判空路径就 readCover
**修复**：`requestImage()` 入口加 `if (path.isEmpty()) return QImage();`（首轮 Critical 修复时已顺手做掉）。
- **文件**：`ui/bridge/CoverImageProvider.cpp:39`
- **现状**：`path.isEmpty()` 检查缺失即调用 `MetadataReader::readCover`。
- **影响**：会产生无效 I/O；并使日志变脏。

### ✅ [CORE-8] FFmpeg/MfMediaDecoder duration\*sr 乘法可能 uint64 溢出
**修复**：`MfMediaDecoder.cpp:195-202` 算 total_frames 前先校验 `dur_100ns > (UINT64_MAX - 5'000'000) / sr` 即跳过赋值（保留 0 = 未知时长），防止极长 hi-res 文件 dur * sr 越过 uint64 上限。
- **文件**：`core/decoder/MfMediaDecoder.cpp:189-197`
- **影响**：极长 hi-res 文件（不切实际但理论存在）会截断 duration。

### ✅ [CORE-9] Equalizer / FormatConverter `clampSat<int32_t>(x*2147483648.0)` 注释自相矛盾
**修复**：`Equalizer.cpp:151` 把 output scale 从 `2147483648.0`（2^31）改成 `2147483647.0`（INT32_MAX），与 `clampSat<int32_t>` 内部 `hi` 一致，消除 1 LSB 偏置。FormatConverter 复查未涉及该问题。
- **文件**：`core/dsp/Equalizer.cpp:151`、`core/dsp/FormatConverter.cpp:51, 91`
- **现状**：scale 2^31 但 hi=2^31-1，注释与实现 1 LSB 错位。
- **影响**：极小 DC 偏置，听感无影响；但 MSVC 可能告警 C5051。

### ✅ [CORE-10] `Equalizer.h:30` `static constexpr` 数组的 redundant 类外定义
**修复**：删除 `Equalizer.cpp:12` 的 `constexpr float Equalizer::kCenters[Equalizer::kNumBands];`（C++17 起 inline static constexpr 类内已完成定义，类外 redeclaration 已 deprecated / MSVC C5051）。
- **文件**：`core/dsp/Equalizer.cpp:12`
- **现状**：C++17 起 inline constexpr，类外再 declare 已 deprecated（MSVC C5051）。

### ✅ [CORE-11] `FormatConverter::process` 实时路径中 `std::vector<float>` 临时分配
**修复**：在 `FormatConverter.h` 加成员 `frame_buf_ / linear_a_ / linear_b_ / linear_out_`，`configure()` 中按 `channels` `assign()`；`process()` 同采样率与线性插值两条路径都改用成员指针，避免每次调用堆分配。
- **文件**：`core/dsp/FormatConverter.cpp:195/231/246`
- **影响**：实时 callback 内堆分配是潜在抖动源。
- **建议**：复用 `src_f_/dst_f_/dither_err_` 等成员。

### ⏭️ [CORE-12] `PolyphaseResampler` 内层 `memmove` 每帧每通道 124 字节
**跳过原因**：消除 memmove 需要把 history 由"线性数组+左移"改造成"环形索引"，涉及内核 SSE2/AVX2 同步改造。结构性优化，最小补丁外，本轮跳过。
- **文件**：`core/dsp/PolyphaseResampler.cpp:250`
- **影响**：8 通道实时下 CPU 不可忽略。建议改成循环索引（环形缓冲）。

### ⏭️ [CORE-13] `RingBuffer` SPSC 假设无运行时断言
**跳过原因**：要在不影响 SPSC 无锁性能的前提下做误用检测，需要 DEBUG-only thread ID 跟踪机制（按构造线程记 reader/writer 然后比对），改动相对侵入。当前已在头文件文档中明确"仅允许一写一读"约定，本轮跳过。
- **文件**：`core/buffer/RingBuffer.h:33`
- **影响**：误用 MPMC 不会立刻爆，但会出现数据丢失/重复，难以诊断。

### ✅ [CORE-14] `MetadataReader.cpp:524` `std::stoi(std::wstring)` 可移植性
**修复**：去掉冗余的 `std::wstring(w.begin(), w.end())` 拷贝构造，直接 `std::stoi(w)`（标准库提供 `wstring` 重载）。
- **影响**：MSVC 有 wstring 重载，其他实现不一定有；属可移植性问题。

### ⏭️ [PLAT-9] `try_recovery` 在 `try_lock` 失败时无退避
**跳过原因**：monitor_loop 本身已经 100ms 节流，`recovery_pending` 标志由下个 tick 重试，自然形成 100ms 粒度退避；要做更细的指数退避需要在 monitor_loop 内引入额外计时状态。本轮跳过。
- **文件**：`app/controller/PlayerController.cpp:608-612`
- **影响**：用户线程长时间持锁时，monitor 线程会以 100ms tick 持续旋转。

### ✅ [PLAT-10] TaskbarButtons 未处理 explorer 重启消息
**修复**：`TaskbarButtons` 新增公开方法 `taskbarCreatedMessageId()` 与 `onTaskbarRestart()`；后者 Release 旧 ITaskbarList3 + 重新 CoCreate + HrInit + addButtons。`ui/main.cpp` 的 `TaskbarEventFilter` 拦截到该消息时调用 `tb->onTaskbarRestart()`，让 explorer 崩溃 / 重启后任务栏按钮自动恢复。
- **文件**：`platform/taskbar/TaskbarButtons.cpp:192-205`
- **现状**：注册了 `WM_TaskbarButtonCreated`，但 window proc 没有过滤。
- **影响**：explorer 崩溃 / 重启后任务栏按钮永远丢失。

### ⏭️ [PLAT-11] `SmtcController` 跨公寓 release 隐患
**跳过原因**：要保证析构发生在创建线程需要引入 dispatch_to_creation_thread 机制（持 thread_id + std::function 队列或借 Qt 信号），属架构改动。当前 SmtcController 在 main 线程构造与析构，实际不会跨公寓。本轮跳过。
- **文件**：`platform/smtc/SmtcController.cpp:54-55, 119-122`
- **现状**：构造时 `COINIT_MULTITHREADED` 与主线程已是 STA 冲突（拿到 `RPC_E_CHANGED_MODE`，代码已处理）；隐患在析构线程不一定是创建线程，跨公寓 release WinRT 对象。
- **建议**：把析构强制 dispatch 回创建线程。

### ✅ [PLAT-12] `CueSheet::parseTimecode` 严格要求 MM:SS:FF
**修复**：`CueSheet.cpp:83-93` 标准 `MM:SS:FF` 解析失败后兜底再试 `MM:SS` 两段格式，覆盖手写 cue 遗漏 FF 段的情况。
- **文件**：`app/playlist/CueSheet.cpp:86-88`
- **影响**：常见 `INDEX 01 03:25` 两段格式解析失败 → 无法定位该 cue 条目。
- **建议**：兜底两段格式或返回错误码。

### ✅ [BUILD-6] vcpkg 声明的 spdlog / nlohmann-json 在源码中并未使用
**修复**：源码 grep 确认两库未被任何 `#include` 引用，从 `vcpkg.json:dependencies` 中删除。`dependencies` 数组保留空体，按需通过 `features` 启用。
- **现状**：grep 全工程无 `<spdlog/...>` `<nlohmann/json.hpp>` include。
- **影响**：若计划用，目前没有对应 `find_package` 与 link；若不用，则是冗余依赖（影响首次配置时间）。

### ✅ [BUILD-7] Qt6 `find_package` 未显式声明 `Core` / `Gui`
**修复**：根 `CMakeLists.txt:62` 把 `find_package(Qt6 6.5 COMPONENTS Qml Quick QuickControls2 REQUIRED)` 改为 `... COMPONENTS Core Gui Qml Quick QuickControls2 REQUIRED`。
- **文件**：根 `CMakeLists.txt:62`
- **现状**：只列了 `Qml Quick QuickControls2`；Core/Gui 通过传递依赖带入。
- **建议**：显式声明以减少版本错配风险。

### ✅ [BUILD-8] `opusfile` 的 `find_package` 三次大小写变体冗余
**修复**：根 `CMakeLists.txt:147-149` 三次 find_package 简化为单一 `find_package(OpusFile CONFIG QUIET)` + 对应 `if (TARGET OpusFile::opusfile)`，pkg-config fallback 路径保留不变。
- **文件**：根 `CMakeLists.txt:147-149`
- **现状**：不同操作系统大小写敏感性不同；vcpkg 实际导出 `OpusFile::opusfile`。
- **建议**：保留 `OpusFile::opusfile`，移除其他两次，并对 pkg-config 走单独的 fallback。

### ✅ [BUILD-9] `ui/CMakeLists.txt` 源码列表未包含 `devicedialog/` 和 `playlistview/`
**修复**：两个目录确认为空（与 STRUCT-1 同事项一并清理），直接删除磁盘上的空目录；`platform/CMakeLists.txt` 同步去掉对 `com/*.{h,cpp}` 的 glob。
- **文件**：`ui/CMakeLists.txt:5-17`
- **现状**：两个目录实际存在但为空（已核实）。若计划填充，必须更新源码列表，否则容易"加了文件但没被编"。
- **建议**：当前删除空目录或加入 GLOB（CONFIGURE_DEPENDS）。

---

## Low（清理 / 风格 / 历史遗留）

### ✅ [STRUCT-1] 多处空目录
**修复**：删除 `core/engine/`、`app/metadata/`、`app/settings/`、`platform/com/`、`ui/devicedialog/`、`ui/playlistview/` 六处空目录；`platform/CMakeLists.txt` 同步去掉 `com/*.{h,cpp}` glob。
- 已核实为空：
  - `core/engine/`
  - `app/metadata/`
  - `app/settings/`
  - `platform/com/`
  - `ui/devicedialog/`
  - `ui/playlistview/`
- 影响：仅噪声；其中 `platform/CMakeLists.txt:11-12` 仍 GLOB 了 `com/*.h .cpp`（无害），其它没被引用。
- 建议：要么补内容要么删除。

### 🔍 [QML-5] `qml.qrc` 注册了可能废弃的 `PlaylistView.qml`
**复查结论**：非废弃。`main.qml:366` 中 `"queue"` 路由的目标就是 `views/PlaylistView.qml`。**未改动**。
- **文件**：`ui/resources/qml.qrc:41`
- **现状**：与 `PlaylistsView.qml` 并存，命名仅差一个 s；UI 中实际只用 PlaylistsView。
- **建议**：确认废弃后从 qrc 与磁盘移除。

### 🔍 [QML-6] `main.qml` 中 `glassBorder` / `glassBorderDark` 重复指向 `borderColor`
**复查结论**：这是刻意的兼容别名 token（让旧组件 `glassBorder` / `glassBorderDark` 仍能编译）。`property alias` 在 QML 中只能 alias 到带 id 的 Object 的 property，不能 alias 同 scope 的另一个 property，所以当前 `readonly property color X: borderColor` 是正确写法。**未改动**。
- **文件**：`ui/qml/main.qml:178-179`
- **建议**：直接 `property alias`，或减少 token 数。

### 🔍 [CORE-15] `core/decoder` 中各解码器宏判定不对称
**复查结论**：`dr_flac.h` 是入库的 third_party 文件（必然存在），与 `dr_mp3.h` / `stb_vorbis.c` 这种由根 CMake 在配置阶段联网下载的可选依赖不同。FlacDecoder 不需要 `APX_HAVE_DR_FLAC` 这种条件桩，差异是有意为之。**未改动**。
- **现状**：`Mp3Decoder` 用 `APX_HAVE_DR_MP3`、`VorbisDecoder` 用 `APX_HAVE_STB_VORBIS`、`FlacDecoder` 没有对应宏 → dr_flac.h 必须存在否则编译失败。
- **建议**：补 `APX_HAVE_DR_FLAC`，或将 dr_flac 也走相同的"缺失则桩"模式。

### ✅ [CORE-16] `DsdDecoder` 状态机 `block_loaded` / `cur_byte_in_block` 脆弱
**修复**：在 `DsdDecoder.cpp:276-281` 加注释明确状态机不变量（`block_loaded=false` 触发下一次 read 重读 block；`cur_byte_in_block < block_size` 且与 `channels*2` 对齐）。代码逻辑保留不变。
- **文件**：`core/decoder/DsdDecoder.cpp:265-274`
- **现状**：seek 后 `block_loaded=false`，下一次 read 重读 block 并从偏移消费；逻辑成立但易于因后续修改打破。
- **建议**：把这些字段封装成一个小状态机。

### ✅ [PLAT-13] `WasapiSharedOutput` 反向估算 src_frames 变量命名反直觉
**修复**：把 `WasapiSharedOutput.cpp:402` 的 `ratio` 重命名为 `dst_per_src`，加注释说明 `need_src = need_dst / dst_per_src`，避免读者误读方向。数学语义不变。
- **文件**：`platform/wasapi/WasapiSharedOutput.cpp:402-405`
- **现状**：`ratio = dst/src`，再 `frames_avail / ratio`，数学上正确但变量名误导。
- **建议**：改名 `dst_to_src_ratio` 或直接写 `dst_frames * src_sr / dst_sr`。

### ✅ [BUILD-10] `tests/CMakeLists.txt` 只是占位 `message`
**修复**：在 `tests/CMakeLists.txt` 加入完整说明注释（启用步骤、推荐覆盖模块），`message` 文案改为明示"未实装"。代码不变。
- **影响**：`-DAPX_BUILD_TESTS=ON` 时 `enable_testing()` 无实际测试。
- **建议**：要么真正集成 GoogleTest，要么把选项默认值与提示信息明确。

### ⏭️ [BUILD-11] `add_compile_definitions` 全局散播
**跳过原因**：把 `UNICODE/_UNICODE/NOMINMAX/WIN32_LEAN_AND_MEAN` 与 `APX_HAVE_*` 全部下沉到 target 级别需要抽 `apx_compile_options` interface library + 全 target 替换，改动面较大；当前全局散播实际影响很小（examples/tests 拿到同样宏不会出错）。本轮跳过。
- **文件**：根 `CMakeLists.txt:29, 104, 129, 138`
- **现状**：`UNICODE/_UNICODE/NOMINMAX/WIN32_LEAN_AND_MEAN` + `APX_HAVE_DR_MP3=1` 等都是全局。
- **建议**：核心宏放进 target 级别的 interface library（例如 `apx_compile_options`）。

### ✅ [PLAT-14] `JumpList::install` 未自带 `CoInitialize`
**修复**：`JumpList.cpp:64-72` 入口先 `CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED)`，按返回值（`S_OK`/`S_FALSE` 需配对、`RPC_E_CHANGED_MODE` 不需）决定是否在退出时 `CoUninitialize`；所有失败路径都同步 cleanup。
- **文件**：`platform/taskbar/JumpList.cpp:64-129`
- **现状**：依赖调用方已初始化 COM；当前 UI 主线程已由 Qt 初始化为 STA，OK。
- **建议**：函数入口保护性 `CoInitializeEx` + 退出时配对。

### ✅ [VM-9] `PlayerViewModel.h` `public:` 嵌在 `private:` 之间
**修复**：把 `taskbarButtons()` 从夹在 `private:`/`private:` 中间的孤立 `public:` 块移到上方主 `public:` 区（attachWindow 后面），消除可读性陷阱。
- **文件**：`ui/bridge/PlayerViewModel.h:336-348`
- **影响**：可读性差。

---

## 交叉验证 / 进一步建议

- **架构与目标对齐**：项目名 `_audio_player_x86` 暗示 32-bit；但 `CMakeLists.txt` 仅在 `CMAKE_SIZEOF_VOID_P==8` 时启用 AVX2 文件，无运行时 CPUID 探测。32-bit 路径建议加 SSE2 兜底 + 运行时探测。
- **测试缺失**：整库无单元测试。建议至少对 RingBuffer / FormatConverter / Equalizer / PolyphaseResampler / CueSheet 解析等纯函数模块补充测试。
- **代码风格**：app 层完全无 Qt 依赖（好）；core 层无 Qt 依赖（好）。但 `ui/bridge/PlayerViewModel.cpp` 体量过大（1000+ 行），可拆出 metadata 缓存、color 缓存、SMTC 桥接等子模块。

---

## 严重级统计

| 严重级 | 数量 | 已修复 ✅ | 复查非 bug 🔍 | 跳过 ⏭️ | 未处理 ⏸️ |
|--------|------|----------|--------------|----------|-----------|
| Critical | 8 | 7 | 1 | 0 | 0 |
| High | 18 | 11 | 3 | 4 | 0 |
| Medium | 19 | 15 | 0 | 4 | 0 |
| Low | 10 | 6 | 3 | 1 | 0 |
| **合计** | **55** | **39** | **7** | **9** | **0** |

> 本清单为静态分析结果，部分问题（如线程竞争窗口、解码尾边界）需要带断点 / 日志的运行时复现才能确认严重度。
> **全部 55 项已处理完毕**：39 项动手修复 / 7 项复查为非 bug / 9 项结构性重构跳过；无 ⏸️ 未处理项。9 项跳过的结构性问题在 issues.md 各条目的"跳过原因"中标明，可按需后续单独立项处理。

---

## 本轮改动文件清单（累计 28 个）

**首轮 Critical+High（21 个）**
```
vcpkg.json
CMakeLists.txt
core/CMakeLists.txt
platform/CMakeLists.txt
core/decoder/MfMediaDecoder.cpp
core/decoder/Mp3Decoder.cpp
core/decoder/VorbisDecoder.cpp
core/decoder/DffDecoder.cpp
core/decoder/DsdDecoder.cpp
core/lyrics/LyricsLoader.cpp
core/dsp/FormatConverter.cpp
core/dsp/PolyphaseResampler.cpp
platform/asio/AsioOutput.cpp
app/controller/PlayerController.cpp
app/playlist/Playlist.cpp
app/playlist/PlaylistIO.cpp
ui/main.cpp
ui/bridge/PlayerViewModel.h
ui/bridge/PlayerViewModel.cpp
ui/bridge/CoverImageProvider.cpp
ui/qml/components/PlaceholderPage.qml
```

**Medium 轮新增 / 二次修改（7 个新增 + 已有文件再次改动）**
```
core/dsp/Equalizer.cpp                       (CORE-9 / CORE-10)
core/dsp/FormatConverter.h                   (CORE-11)
core/metadata/MetadataReader.cpp             (CORE-14)
ui/bridge/CoverImageProvider.h               (VM-7)
app/playlist/CueSheet.cpp                    (PLAT-12)
platform/taskbar/TaskbarButtons.h            (PLAT-10)
platform/taskbar/TaskbarButtons.cpp          (PLAT-10)

# 再次改动 / 多项修复叠加：
core/decoder/MfMediaDecoder.cpp              (CORE-8 再加溢出保护)
core/dsp/FormatConverter.cpp                 (CORE-11)
ui/bridge/PlayerViewModel.h/.cpp             (VM-6)
ui/bridge/CoverImageProvider.cpp             (VM-7 真正 LRU)
ui/qml/components/WaveformProgressBar.qml    (QML-4)
ui/main.cpp                                  (PLAT-10 native event filter)
CMakeLists.txt                               (BUILD-7 / BUILD-8)
vcpkg.json                                   (BUILD-6)
platform/CMakeLists.txt                      (BUILD-9 去 com glob)
```

**Low 轮新增 / 二次修改（5 处）**
```
core/decoder/DsdDecoder.cpp                  (CORE-16 状态机不变量注释)
platform/wasapi/WasapiSharedOutput.cpp       (PLAT-13 ratio → dst_per_src)
platform/taskbar/JumpList.cpp                (PLAT-14 CoInitialize 保护)
tests/CMakeLists.txt                         (BUILD-10 完善占位说明)
ui/bridge/PlayerViewModel.h                  (VM-9 整理 public/private 块)
```

**删除的空目录（STRUCT-1）**
```
core/engine/
app/metadata/
app/settings/
platform/com/
ui/devicedialog/
ui/playlistview/
```
