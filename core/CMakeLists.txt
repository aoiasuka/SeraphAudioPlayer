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
)

add_library(apx_core STATIC ${APX_CORE_SOURCES})

# 项目根作为 include 根,允许 #include "core/buffer/RingBuffer.h" 等绝对感的路径
target_include_directories(apx_core
    PUBLIC
        ${CMAKE_SOURCE_DIR}
    PRIVATE
        ${CMAKE_SOURCE_DIR}/third_party    # dr_libs/dr_flac.h 仅在 FlacDecoder.cpp 内使用
)

target_compile_features(apx_core PUBLIC cxx_std_17)

list(LENGTH APX_CORE_SOURCES _n)
message(STATUS "  [core]     STATIC library, sources=${_n}")
