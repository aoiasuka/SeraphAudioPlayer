cmake_minimum_required(VERSION 3.20)

project(AudioPlayerX86
    VERSION 0.1.0
    DESCRIPTION "Windows WASAPI Exclusive-mode Hi-Fi Audio Player"
    LANGUAGES CXX
)

# 【核心修复】：将项目根目录加入到全局头文件搜索路径中
# 这样在 ui 模块下也能通过 "app/controller/..." 找到头文件
include_directories(${CMAKE_CURRENT_SOURCE_DIR})

# -------- 全局设置 --------
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# 仅支持 Windows
if(NOT WIN32)
    message(FATAL_ERROR "AudioPlayerX86 仅支持 Windows 平台")
endif()

# 默认 Release,且默认 32-bit (x86) 通过命令行 -A Win32 指定
if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

# Unicode (Windows)
add_compile_definitions(UNICODE _UNICODE NOMINMAX WIN32_LEAN_AND_MEAN)

# MSVC 选项
if(MSVC)
    # 注意：这里已经包含了 /utf-8，所以 C4819 警告在配置成功后会自动消失
    add_compile_options(/W4 /permissive- /utf-8 /Zc:__cplusplus)
    # Qt 6 二进制按动态 CRT 链接,我们必须与之一致,否则 LNK4098 冲突。
    # Windows 10/11 默认带 VC++ Redist 2015-2022,目标机一般无需额外安装。
    set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")
endif()

# 架构检测
if(CMAKE_SIZEOF_VOID_P EQUAL 4)
    set(APX_ARCH "x86")
else()
    set(APX_ARCH "x64")
endif()
message(STATUS "AudioPlayerX86: Target architecture = ${APX_ARCH}")

# 输出目录统一
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)

# -------- 选项 --------
option(APX_BUILD_EXAMPLES "Build standalone examples (WASAPI demo etc.)" ON)
option(APX_BUILD_TESTS    "Build unit tests"                              OFF)
option(APX_BUILD_APP      "Build app layer (controller/playlist/...)"     ON)
option(APX_BUILD_UI       "Build Qt 6 Widgets UI (AudioPlayerX86.exe)"    ON)

# -------- Qt --------
if(APX_BUILD_UI)
    # 用户需通过 -DCMAKE_PREFIX_PATH=<Qt>/6.5.x/msvc2019_32 指定 Qt
    find_package(Qt6 6.5 COMPONENTS Qml Quick QuickControls2 REQUIRED)
    set(CMAKE_AUTOMOC ON)
    set(CMAKE_AUTORCC ON)
    set(CMAKE_AUTOUIC ON)
    message(STATUS "Found Qt6 ${Qt6_VERSION} at ${Qt6_DIR}")
endif()

# -------- QWindowKit: 已弃用,改为纯 Qt frameless (见 ui/qml/components/TitleBar.qml) --------
# 如需恢复请取消以下注释并重新加回 ui/CMakeLists.txt 的 QWindowKit::Quick 链接
# if(APX_BUILD_UI)
#     include(FetchContent)
#     FetchContent_Declare(
#         QWindowKit
#         GIT_REPOSITORY https://github.com/stdware/qwindowkit.git
#         GIT_TAG        main
#     )
#     set(QWINDOWKIT_BUILD_QUICK ON CACHE BOOL "" FORCE)
#     set(QWINDOWKIT_BUILD_WIDGETS OFF CACHE BOOL "" FORCE)
#     FetchContent_MakeAvailable(QWindowKit)
# endif()

# -------- 第三方单头文件自动下载 --------
# dr_mp3.h (MP3 解码) — 不在仓库;首次配置时从 GitHub 拉取
set(_DR_MP3_PATH "${CMAKE_CURRENT_SOURCE_DIR}/third_party/dr_libs/dr_mp3.h")
if(NOT EXISTS "${_DR_MP3_PATH}")
    message(STATUS "Downloading dr_mp3.h ...")
    file(DOWNLOAD
        "https://raw.githubusercontent.com/mackron/dr_libs/master/dr_mp3.h"
        "${_DR_MP3_PATH}"
        SHOW_PROGRESS
        TIMEOUT 120
        STATUS _dl_status
    )
    list(GET _dl_status 0 _dl_code)
    if(NOT _dl_code EQUAL 0)
        file(REMOVE "${_DR_MP3_PATH}")
        message(WARNING "无法下载 dr_mp3.h: ${_dl_status}\n"
                        "  MP3 解码将不可用。手动放置到 ${_DR_MP3_PATH} 后重新配置。")
    endif()
endif()
if(EXISTS "${_DR_MP3_PATH}")
    set(APX_HAVE_DR_MP3 ON)
    add_compile_definitions(APX_HAVE_DR_MP3=1)
else()
    set(APX_HAVE_DR_MP3 OFF)
endif()

# stb_vorbis.c (OGG Vorbis 解码) — 首次配置时从 GitHub 拉取
set(_STB_VORBIS_PATH "${CMAKE_CURRENT_SOURCE_DIR}/third_party/stb/stb_vorbis.c")
if(NOT EXISTS "${_STB_VORBIS_PATH}")
    message(STATUS "Downloading stb_vorbis.c ...")
    file(DOWNLOAD
        "https://raw.githubusercontent.com/nothings/stb/master/stb_vorbis.c"
        "${_STB_VORBIS_PATH}"
        SHOW_PROGRESS
        TIMEOUT 120
        STATUS _dl_status2
    )
    list(GET _dl_status2 0 _dl_code2)
    if(NOT _dl_code2 EQUAL 0)
        file(REMOVE "${_STB_VORBIS_PATH}")
        message(WARNING "无法下载 stb_vorbis.c: ${_dl_status2}\n"
                        "  OGG Vorbis 解码将不可用。")
    endif()
endif()
if(EXISTS "${_STB_VORBIS_PATH}")
    set(APX_HAVE_STB_VORBIS ON)
    add_compile_definitions(APX_HAVE_STB_VORBIS=1)
else()
    set(APX_HAVE_STB_VORBIS OFF)
endif()

# ASIO SDK 检测 (用户须手动放置;不自动下载,因协议要求接受 EULA)
set(_ASIO_SDK_PATH "${CMAKE_CURRENT_SOURCE_DIR}/third_party/asiosdk/common/asio.h")
if(EXISTS "${_ASIO_SDK_PATH}")
    set(APX_HAVE_ASIO_SDK ON)
    add_compile_definitions(APX_HAVE_ASIO_SDK=1)
    message(STATUS "ASIO SDK found, enabling ASIO output backend")
else()
    set(APX_HAVE_ASIO_SDK OFF)
    message(STATUS "ASIO SDK not present (stub backend used). See docs/ASIO.md")
endif()

# -------- 子模块 --------
add_subdirectory(core)
add_subdirectory(platform)

if(APX_BUILD_APP)
    add_subdirectory(app)
endif()

if(APX_BUILD_UI)
    add_subdirectory(ui)
endif()

if(APX_BUILD_EXAMPLES)
    add_subdirectory(examples)
endif()

if(APX_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

message(STATUS "")
message(STATUS "AudioPlayerX86 configuration:")
message(STATUS "  Version       : ${PROJECT_VERSION}")
message(STATUS "  Arch          : ${APX_ARCH}")
message(STATUS "  Build type    : ${CMAKE_BUILD_TYPE}")
message(STATUS "  Build app     : ${APX_BUILD_APP}")
message(STATUS "  Build UI      : ${APX_BUILD_UI}")
message(STATUS "  Build examples: ${APX_BUILD_EXAMPLES}")
message(STATUS "  Build tests   : ${APX_BUILD_TESTS}")
message(STATUS "")