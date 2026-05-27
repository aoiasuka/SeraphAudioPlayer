# ui/ — Qt 6 Quick 主程序
#
# 产物:AudioPlayerX86.exe (WIN32 子系统,无控制台窗口)

set(APX_UI_SOURCES
    main.cpp
    bridge/PlayerViewModel.h
    bridge/PlayerViewModel.cpp
    bridge/PlaylistViewModel.h
    bridge/PlaylistViewModel.cpp
    bridge/ShortcutsViewModel.h
    bridge/ShortcutsViewModel.cpp
    bridge/DeviceBridge.h
    bridge/DeviceBridge.cpp
    bridge/CoverImageProvider.h
    bridge/CoverImageProvider.cpp
)

set(APX_UI_RESOURCES
    resources/resources.qrc
    resources/qml.qrc
)

add_executable(AudioPlayerX86 WIN32
    ${APX_UI_SOURCES}
    ${APX_UI_RESOURCES}
)

set_target_properties(AudioPlayerX86 PROPERTIES
    OUTPUT_NAME "SeraphAudioPlayer"
    # WIN32_EXECUTABLE 已在 add_executable 中显式声明
)

target_include_directories(AudioPlayerX86 PRIVATE ${CMAKE_SOURCE_DIR})
target_compile_features(AudioPlayerX86 PRIVATE cxx_std_17)

target_link_libraries(AudioPlayerX86 PRIVATE
    apx_app
    apx_core
    apx_platform
    Qt6::Qml
    Qt6::Quick
    Qt6::QuickControls2
)

# Win11 圆角窗口需要 DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE)
if(WIN32)
    target_link_libraries(AudioPlayerX86 PRIVATE dwmapi)
endif()

# 安装目标(可选,后续打包用)
install(TARGETS AudioPlayerX86 RUNTIME DESTINATION bin)

# 配置后输出帮助提示
message(STATUS "  [ui]       AudioPlayerX86.exe (Qt Quick)")
