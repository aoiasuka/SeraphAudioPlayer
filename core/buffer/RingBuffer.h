// =============================================================================
//  core/buffer/RingBuffer.h
//
//  字节级单生产者单消费者 (SPSC) 无锁环形缓冲区。
//
//  正确性来源:
//    - 容量始终是 2 的幂,head/tail 用 size_t 单调递增,索引 = pos & mask_
//    - 生产者只写 write_pos_,消费者只写 read_pos_
//    - release / acquire 内存序在两个 atomic 之间建立 happens-before 关系
//    - alignas(64) 隔离两个原子,避免 false sharing
//
//  使用约定:
//    - 仅允许"一个写线程 + 一个读线程"
//    - clear() 只能在双端都停时调用(没有线程在 write/read)
//    - 写满时 write() 返回 < bytes;读空时 read() 返回 < bytes —— 调用方自己处理欠载
// =============================================================================
#pragma once

#include <atomic>
#include <cstddef>
#include <cstdint>

namespace apx {

class RingBuffer {
public:
    // capacity 会被向上取整到 2 的幂(下限 64 字节)
    explicit RingBuffer(std::size_t capacity_bytes);
    ~RingBuffer();

    RingBuffer(const RingBuffer&)            = delete;
    RingBuffer& operator=(const RingBuffer&) = delete;
    RingBuffer(RingBuffer&&)                 = delete;
    RingBuffer& operator=(RingBuffer&&)      = delete;

    // 实际容量(向上取整后的字节数);可写上限 = capacity - 1(留 1 字节判满)
    std::size_t capacity() const noexcept { return cap_; }

    // 消费者可读取的字节数
    std::size_t readable() const noexcept;

    // 生产者可写入的字节数
    std::size_t writable() const noexcept;

    // 写入,返回实际写入的字节数(0 ~ bytes)
    std::size_t write(const void* src, std::size_t bytes) noexcept;

    // 读取,返回实际读出的字节数(0 ~ bytes)
    std::size_t read(void* dst, std::size_t bytes) noexcept;

    // 跳过(丢弃)前 n 字节,返回实际丢弃数(消费端)
    std::size_t discard(std::size_t bytes) noexcept;

    // 静音填充(消费端):用 0 覆盖目标缓冲剩余部分,常用于欠载时避免噪音
    static void fill_silence(void* dst, std::size_t bytes) noexcept;

    // 清空(必须在双端都停时调用,否则未定义)
    void clear() noexcept;

private:
    std::uint8_t* data_ = nullptr;
    std::size_t   cap_  = 0;     // 2 的幂
    std::size_t   mask_ = 0;     // cap_ - 1

    // 缓存行隔离,避免生产者/消费者互踩(alignas(64) 会让结构尾部填充,这是预期行为)
#if defined(_MSC_VER)
#  pragma warning(push)
#  pragma warning(disable: 4324)  // structure was padded due to alignment specifier
#endif
    alignas(64) std::atomic<std::size_t> write_pos_{0};
    alignas(64) std::atomic<std::size_t> read_pos_{0};
#if defined(_MSC_VER)
#  pragma warning(pop)
#endif
};

} // namespace apx
