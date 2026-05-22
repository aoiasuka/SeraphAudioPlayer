// =============================================================================
//  core/decoder/DecoderFactory.h
//
//  按扩展名(或将来:魔数)创建解码器实例。
//  当前注册:.wav / .wave
// =============================================================================
#pragma once

#include "core/decoder/IDecoder.h"

#include <memory>
#include <string>

namespace apx {

class DecoderFactory {
public:
    // 失败返回 nullptr(无匹配解码器)。返回的实例尚未 open。
    static std::unique_ptr<IDecoder> createForFile(const std::wstring& path);

    // 提取扩展名并转小写,例如 "Foo.WAV" → ".wav"。无扩展返回 L""。
    static std::wstring extensionLower(const std::wstring& path);
};

} // namespace apx
