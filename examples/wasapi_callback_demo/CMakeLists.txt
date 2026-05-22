add_executable(wasapi_callback_demo main.cpp)

set_target_properties(wasapi_callback_demo PROPERTIES
    WIN32_EXECUTABLE OFF
    OUTPUT_NAME      "wasapi_callback_demo"
)

target_link_libraries(wasapi_callback_demo PRIVATE
    apx_core
    apx_platform
)

if(MSVC)
    target_compile_options(wasapi_callback_demo PRIVATE /W4)
endif()
