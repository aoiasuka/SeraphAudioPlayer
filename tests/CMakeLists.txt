# tests/ — 单元测试占位
#
# 当前为空。如需启用：
#   1. 通过 vcpkg 提供 gtest（vcpkg.json features.tests 已声明依赖）
#   2. 在此添加 find_package(GTest CONFIG REQUIRED) + add_executable + add_test
#   3. 推荐先覆盖：RingBuffer / FormatConverter / Equalizer /
#                  PolyphaseResampler / CueSheet / PlaylistIO 等纯函数模块
#
# 此 CMakeLists 仅在 -DAPX_BUILD_TESTS=ON 时被根 CMakeLists 添加为子目录；
# 关闭选项时整个文件不会被处理。
message(STATUS "  [tests]    placeholder — 实际测试未实装；编辑 tests/CMakeLists.txt 以启用")
