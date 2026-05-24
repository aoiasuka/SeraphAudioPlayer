// =============================================================================
//  core/decoder/IDecoder.h
//
//  解码器抽象。所有格式(WAV/FLAC/MP3/DSD/...)都实现此接口。
//
//  线程模型:
//    - 单线程使用(producer 线程独占)。
//    - 不要求实现内部加锁。
//
//  数据约定:
//    - read() 返回的字节流严格符合 format() 描述的 PCM 排布
//    - read() 写入字节数总是 frame_bytes 的整数倍
//    - read() 返回 0 表示到达流末尾 (EOF)
// =============================================================================
#pragma once

#include "core/format/AudioFormat.h"
#include "core/dsd/DopMode.h"

#include <cstddef>
#include <cstdint>
#include <string>

namespace apx {

class IDecoder {
public:
    virtual ~IDecoder() = default;

    // 打开文件。失败返回 false,可通过 lastError() 取消息。
    virtual bool open(const std::wstring& path) = 0;

    // 释放文件句柄与内部状态。
    virtual void close() = 0;

    virtual bool isOpen() const = 0;

    // 已打开时返回有效格式;否则 valid()==false。
    virtual AudioFormat format() const = 0;

    // 文件总帧数(单声道单样本计 1 帧?不,1 帧 = 所有通道一组样本)
    virtual std::int64_t totalFrames() const = 0;

    // 当前读指针(帧单位)
    virtual std::int64_t currentFrame() const = 0;

    // 跳转到指定帧。frame 会被夹到 [0, totalFrames]。
    virtual bool seek(std::int64_t frame) = 0;

    // 读取 PCM 字节流。返回实际读入字节数(必为 frame_bytes 倍数);0=EOF。
    virtual std::size_t read(std::uint8_t* dst, std::size_t bytes) = 0;

    // 最近一次错误的描述(供日志/UI 展示)
    virtual std::wstring lastError() const = 0;

    // 可选 DSD 钩子。仅 DSD 解码器 (Dsd/Dff) 有意义,其它实现默认空操作。
    // 调用时机不限,但实际生效时点是下一次 read。
    virtual void setDopMarkerMode(DopMarkerMode /*mode*/) {}

    // DSD 输出模式切换。默认 DoP (false);true = raw native LSB8 packed。
    // 必须在 open 之后、首次 read 之前调用;返回 false 表示实现不支持。
    // PCM 解码器默认返回 false (无意义)。
    virtual bool setNativeDsd(bool /*native*/) { return false; }
};

} // namespace apx
