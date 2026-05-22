# examples/ — 独立示例(技术验证)
add_subdirectory(wasapi_exclusive_demo)
add_subdirectory(wasapi_callback_demo)
add_subdirectory(wasapi_play_wav_demo)
add_subdirectory(wasapi_devices_demo)

# cli_player_demo 需要 app 层
if(APX_BUILD_APP)
    add_subdirectory(cli_player_demo)
endif()
