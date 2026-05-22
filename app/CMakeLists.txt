# app/ — 应用层(控制器、播放列表、设置、元数据)
#
# 当前编入:
#   controller/PlayerState.h
#   controller/PlayerController.{h,cpp}
# 后续:playlist/、settings/、metadata/(M2+)

file(GLOB_RECURSE APX_APP_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/controller/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/controller/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/playlist/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/playlist/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/settings/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/settings/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/metadata/*.h"
    "${CMAKE_CURRENT_SOURCE_DIR}/metadata/*.cpp"
)

add_library(apx_app STATIC ${APX_APP_SOURCES})

target_include_directories(apx_app
    PUBLIC
        ${CMAKE_SOURCE_DIR}
)

target_compile_features(apx_app PUBLIC cxx_std_17)

target_link_libraries(apx_app
    PUBLIC
        apx_core
        apx_platform
)

list(LENGTH APX_APP_SOURCES _n)
message(STATUS "  [app]      STATIC library, sources=${_n}")
