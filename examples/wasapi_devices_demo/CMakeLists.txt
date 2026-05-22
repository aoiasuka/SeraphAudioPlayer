add_executable(wasapi_devices_demo main.cpp)

set_target_properties(wasapi_devices_demo PROPERTIES
    WIN32_EXECUTABLE OFF
    OUTPUT_NAME      "wasapi_devices_demo"
)

target_link_libraries(wasapi_devices_demo PRIVATE
    apx_core
    apx_platform
)

if(MSVC)
    target_compile_options(wasapi_devices_demo PRIVATE /W4)
endif()
