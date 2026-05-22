add_executable(wasapi_play_wav_demo main.cpp)

set_target_properties(wasapi_play_wav_demo PROPERTIES
    WIN32_EXECUTABLE OFF
    OUTPUT_NAME      "wasapi_play_wav_demo"
)

target_link_libraries(wasapi_play_wav_demo PRIVATE
    apx_core
    apx_platform
)

if(MSVC)
    target_compile_options(wasapi_play_wav_demo PRIVATE /W4)
endif()
