// =============================================================================
//  core/buffer/RingBuffer.cpp
// =============================================================================
#include "RingBuffer.h"

#include <algorithm>
#include <cstring>
#include <new>

namespace {

inline std::size_t next_pow2(std::size_t v) noexcept
{
    if (v < 64) return 64;
    --v;
    v |= v >> 1;  v |= v >> 2;  v |= v >> 4;
    v |= v >> 8;  v |= v >> 16;
#if SIZE_MAX > 0xFFFFFFFFu
    v |= v >> 32;
#endif
    return v + 1;
}

} // namespace

namespace apx {

RingBuffer::RingBuffer(std::size_t capacity_bytes)
    : cap_(next_pow2(capacity_bytes))
    , mask_(cap_ - 1)
{
    data_ = new std::uint8_t[cap_];
}

RingBuffer::~RingBuffer()
{
    delete[] data_;
}

std::size_t RingBuffer::readable() const noexcept
{
    const std::size_t w = write_pos_.load(std::memory_order_acquire);
    const std::size_t r = read_pos_.load (std::memory_order_relaxed);
    return w - r;
}

std::size_t RingBuffer::writable() const noexcept
{
    const std::size_t w = write_pos_.load(std::memory_order_relaxed);
    const std::size_t r = read_pos_.load (std::memory_order_acquire);
    // 留 1 字节作为"满/空"的区分,避免 w==r 二义
    return cap_ - (w - r) - 1;
}

std::size_t RingBuffer::write(const void* src, std::size_t bytes) noexcept
{
    if (!src || bytes == 0) return 0;

    const std::size_t w = write_pos_.load(std::memory_order_relaxed);
    const std::size_t r = read_pos_.load (std::memory_order_acquire);
    const std::size_t free_bytes = cap_ - (w - r) - 1;
    const std::size_t n = std::min(bytes, free_bytes);
    if (n == 0) return 0;

    const std::size_t idx   = w & mask_;
    const std::size_t first = std::min(n, cap_ - idx);

    std::memcpy(data_ + idx, src, first);
    if (n > first) {
        std::memcpy(data_, static_cast<const std::uint8_t*>(src) + first, n - first);
    }

    write_pos_.store(w + n, std::memory_order_release);
    return n;
}

std::size_t RingBuffer::read(void* dst, std::size_t bytes) noexcept
{
    if (!dst || bytes == 0) return 0;

    const std::size_t r = read_pos_.load (std::memory_order_relaxed);
    const std::size_t w = write_pos_.load(std::memory_order_acquire);
    const std::size_t avail = w - r;
    const std::size_t n = std::min(bytes, avail);
    if (n == 0) return 0;

    const std::size_t idx   = r & mask_;
    const std::size_t first = std::min(n, cap_ - idx);

    std::memcpy(dst, data_ + idx, first);
    if (n > first) {
        std::memcpy(static_cast<std::uint8_t*>(dst) + first, data_, n - first);
    }

    read_pos_.store(r + n, std::memory_order_release);
    return n;
}

std::size_t RingBuffer::discard(std::size_t bytes) noexcept
{
    const std::size_t r = read_pos_.load (std::memory_order_relaxed);
    const std::size_t w = write_pos_.load(std::memory_order_acquire);
    const std::size_t avail = w - r;
    const std::size_t n = std::min(bytes, avail);
    if (n == 0) return 0;
    read_pos_.store(r + n, std::memory_order_release);
    return n;
}

void RingBuffer::fill_silence(void* dst, std::size_t bytes) noexcept
{
    if (dst && bytes) std::memset(dst, 0, bytes);
}

void RingBuffer::clear() noexcept
{
    write_pos_.store(0, std::memory_order_relaxed);
    read_pos_.store (0, std::memory_order_relaxed);
}

} // namespace apx
