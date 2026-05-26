# core/ — 核心引擎(无 UI / 无平台依赖)
#
# 当前编入:
#   format/AudioFormat.{h,cpp}
#   buffer/RingBuffer.{h,cpp}
#   output/IAudioOutput.h        (interface,header-only)
#   decoder/IDecoder.h           (interface)
#   decoder/WavDecoder.{h,cpp}
#   decoder/FlacDecoder.{h,cpp}  (wraps third_party/dr_libs/dr_flac.h)
#   decoder/DecoderFactory.{h,cpp}

file(GLOB_RECURSE APX_CORE_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/format/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/format/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/buffer/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/buffer/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/output/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/decoder/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/decoder/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/metadata/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/metadata/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/lyrics/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/lyrics/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/dsp/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/dsp/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/dsd/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/dsd/*.cpp"
)

add_library(apx_core STATIC ${APX_CORE_SOURCES})

# AVX2 内核单独编译 (其余文件仍按全局 /arch 默认 — x64 是 SSE2 baseline)
set(_AVX2_FILE "${CMAKE_CURRENT_SOURCE_DIR}/dsp/PolyphaseResampler_avx2.cpp")
if(EXISTS "${_AVX2_FILE}" AND MSVC AND (CMAKE_SIZEOF_VOID_P EQUAL 8))
    set_source_files_properties("${_AVX2_FILE}"
        PROPERTIES COMPILE_OPTIONS "/arch:AVX2")
endif()

# 项目根作为 include 根,允许 #include "core/buffer/RingBuffer.h" 等绝对感的路径
target_include_directories(apx_core
    PUBLIC
        ${CMAKE_SOURCE_DIR}
    PRIVATE
        ${CMAKE_SOURCE_DIR}/third_party    # dr_libs/dr_flac.h 仅在 FlacDecoder.cpp 内使用
)

target_compile_features(apx_core PUBLIC cxx_std_17)

# Media Foundation 系统库（MfMediaDecoder.cpp 通过 #pragma comment(lib,...) 隐式链接，
# 这里显式列出以避免 obj 被剥离或开 /NODEFAULTLIB 时链接失败）
if(WIN32)
    target_link_libraries(apx_core PUBLIC
        mfplat
        mfreadwrite
        mfuuid
        propsys
    )
endif()

# Opus 解码可选依赖
if(APX_HAVE_OPUS)
    # 按 vcpkg / 系统提供的 target 名称依次尝试
    foreach(_tgt OpusFile::opusfile Opusfile::opusfile opusfile::opusfile PkgConfig::OPUSFILE)
        if(TARGET ${_tgt})
            target_link_libraries(apx_core PRIVATE ${_tgt})
            message(STATUS "  apx_core linking against ${_tgt}")
            break()
        endif()
    endforeach()
    if(OPUSFILE_INCLUDE_DIRS)
        target_include_directories(apx_core PRIVATE ${OPUSFILE_INCLUDE_DIRS})
    endif()
endif()

list(LENGTH APX_CORE_SOURCES _n)
message(STATUS "  [core]     STATIC library, sources=${_n}")
