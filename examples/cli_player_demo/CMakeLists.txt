add_executable(cli_player_demo main.cpp)

set_target_properties(cli_player_demo PROPERTIES
    WIN32_EXECUTABLE OFF
    OUTPUT_NAME      "cli_player_demo"
)

target_link_libraries(cli_player_demo PRIVATE
    apx_app
    apx_core
    apx_platform
)

if(MSVC)
    target_compile_options(cli_player_demo PRIVATE /W4)
endif()
