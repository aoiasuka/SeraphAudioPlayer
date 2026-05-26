# platform/ — Windows 平台实现
#
# 当前编入:
#   wasapi/WasapiExclusiveOutput.{h,cpp}

file(GLOB_RECURSE APX_PLATFORM_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/wasapi/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/wasapi/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/mmdevice/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/mmdevice/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/smtc/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/smtc/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/taskbar/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/taskbar/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/asio/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/asio/*.cpp"
)

add_library(apx_platform STATIC ${APX_PLATFORM_SOURCES})

target_include_directories(apx_platform
    PUBLIC
        ${CMAKE_SOURCE_DIR}
)

target_compile_features(apx_platform PUBLIC cxx_std_17)

# SMTC 用 C++/WinRT,需要异常 + 至少 C++17
target_compile_options(apx_platform PRIVATE /EHsc)

# ASIO SDK 头文件路径 (用户复制到 third_party/asiosdk 之后启用)
if(EXISTS "${CMAKE_SOURCE_DIR}/third_party/asiosdk/common/asio.h")
    target_include_directories(apx_platform PRIVATE
        ${CMAKE_SOURCE_DIR}/third_party
    )
    # 这些是 SDK 提供的 .cpp,只在 SDK 存在时编入
    target_sources(apx_platform PRIVATE
        ${CMAKE_SOURCE_DIR}/third_party/asiosdk/common/asio.cpp
        ${CMAKE_SOURCE_DIR}/third_party/asiosdk/host/asiodrivers.cpp
        ${CMAKE_SOURCE_DIR}/third_party/asiosdk/host/pc/asiolist.cpp
    )
    # 关键：把启用宏挂到 target 上，避免依赖根 CMake 的全局宏作用域（自包含）
    target_compile_definitions(apx_platform PRIVATE APX_HAVE_ASIO_SDK=1)
endif()

target_link_libraries(apx_platform
    PUBLIC
        apx_core
    PRIVATE
        ole32
        avrt
        ksuser
        uuid
        winmm
        runtimeobject   # SystemMediaTransportControls (WinRT)
        shell32         # Jump List / ICustomDestinationList
        shlwapi
        propsys         # IPropertyStore (Jump List shell item)
        comctl32        # ITaskbarList3 + image lists
)

list(LENGTH APX_PLATFORM_SOURCES _n)
message(STATUS "  [platform] STATIC library, sources=${_n}")
