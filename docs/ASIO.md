# ASIO 输出后端

本播放器内置 ASIO 输出后端的封装与设备枚举,但 Steinberg ASIO SDK 的源代码
由于许可证不允许重新分发,不入仓库。需要手动启用如下:

## 1. 下载 SDK

去 <https://www.steinberg.net/asiosdk> 注册并下载最新 ASIO SDK
(`asiosdk_X.X.X_YYYY-MM-DD.zip`),解压。

## 2. 复制到 third_party

把解压出的目录拷贝到本仓库:

```
audio_player_x86/
  third_party/
    asiosdk/
      common/            ← 含 asio.h / asio.cpp / asiosys.h
      host/              ← 含 asiodrivers.h / asiodrivers.cpp
      host/pc/           ← 含 asiolist.h / asiolist.cpp
```

只需 `common/`、`host/`、`host/pc/` 三个子目录,其他例子可不要。

## 3. 重新配置 CMake

```powershell
.\scripts\build_app.ps1
```

CMake 在配置阶段会检测 `third_party/asiosdk/common/asio.h`,自动:

- 定义编译宏 `APX_HAVE_ASIO_SDK=1`
- 把 SDK 中的 `asio.cpp` / `asiodrivers.cpp` / `asiolist.cpp` 编入 `apx_platform`
- 链接生成的 `AsioOutput` 类提供真正的 ASIO 后端

启动后日志会出现:

```
ASIO SDK found, enabling ASIO output backend
```

未提供 SDK 时:

```
ASIO SDK not present (stub backend used). See docs/ASIO.md
```

## 4. 使用

后续版本将在设置中提供"输出后端"切换。当前 `AsioOutput::enumerate()` 已经
可用于枚举系统中已安装的 ASIO 驱动(如 ASIO4ALL / FocusriteUSB / RME / ...)。

## 兼容性

- 仅 Windows
- 32-bit / 64-bit 均可,与主程序架构匹配即可
- 多数 DAC 在 ASIO 模式下默认输出 32-bit LSB(`ASIOSTInt32LSB`);
  本封装按此格式协商,DAC 不支持时 `open()` 返回 false
- 取样率 / 缓冲大小由 ASIO 驱动自行决定;PlayerController 会按 `OpenResult`
  返回的 `actual_format` 自动适配
