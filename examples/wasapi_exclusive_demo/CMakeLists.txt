add_executable(wasapi_exclusive_demo main.cpp)

set_target_properties(wasapi_exclusive_demo PROPERTIES
    WIN32_EXECUTABLE OFF          # 控制台子系统,看得到日志
    OUTPUT_NAME      "wasapi_exclusive_demo"
)

target_link_libraries(wasapi_exclusive_demo PRIVATE
    ole32       # COM
    avrt        # AvSetMmThreadCharacteristics
    ksuser      # KSDATAFORMAT_SUBTYPE_*
    uuid        # IID 常量(部分编译器/SDK 需要)
)

# MSVC: 启用最高警告,但 demo 是教学用,容忍部分 C 风格写法
if(MSVC)
    target_compile_options(wasapi_exclusive_demo PRIVATE /W4)
endif()
