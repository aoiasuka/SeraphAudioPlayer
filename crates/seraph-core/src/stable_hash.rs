//! 算法固定的 64 位哈希，给**要落盘的标识**用（本地曲目 ID、封面文件名、旧设备 ID）。
//!
//! LIB-01：这些标识此前用 `std::collections::hash_map::DefaultHasher` 算。标准库文档明确
//! 写着它的算法「不保证跨版本不变」，`Hash` 实现往哈希器里喂什么字节同样没有承诺——哪天
//! 升级工具链后算法变了，所有本地曲目的 ID 会整体改变，收藏、歌单、歌词一次全部对不上。
//!
//! 这里把现状钉死：SipHash-1-3、密钥全零，输入的分帧方式也照搬标准库当前的做法
//! （字符串后面补一个 `0xFF`，字节串前面加 8 字节小端长度）。所以输出与此前
//! `DefaultHasher` 算出来的**逐位相同**，已有曲库不需要任何迁移。
//!
//! 进程内用完即弃的哈希（去重、`HashMap`）不需要稳定，继续用标准库的就好。

/// 字符串的稳定哈希。等价于此前的 `s.hash(&mut DefaultHasher::new())`。
pub fn hash_str(value: &str) -> u64 {
    let mut hasher = SipHasher13::new();
    hasher.write(value.as_bytes());
    // 标准库给 `str` 补的结束符：`0xFF` 不会出现在 UTF-8 里，保证前缀无歧义
    hasher.write(&[0xff]);
    hasher.finish()
}

/// 字节串的稳定哈希。等价于此前的 `bytes.hash(&mut DefaultHasher::new())`
/// （`[u8]` / `Vec<u8>`，64 位小端平台）。
pub fn hash_bytes(value: &[u8]) -> u64 {
    let mut hasher = SipHasher13::new();
    // 标准库给切片加的长度前缀是 `usize` 的本机字节序；这里固定成 8 字节小端，
    // 与唯一支持的目标平台（x86_64 Windows）上的旧行为一致。
    hasher.write(&(value.len() as u64).to_le_bytes());
    hasher.write(value);
    hasher.finish()
}

/// SipHash-1-3（每个消息块 1 轮压缩、收尾 3 轮），密钥为零。
struct SipHasher13 {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
    /// 还没凑满 8 字节的尾巴
    tail: [u8; 8],
    tail_len: usize,
    /// 已写入的总字节数（只有低 8 位参与收尾）
    length: u64,
}

impl SipHasher13 {
    const fn new() -> Self {
        // 初始状态 = 密钥（全零）异或四个固定常量 "somepseudorandomlygeneratedbytes"
        Self {
            v0: 0x736f_6d65_7073_6575,
            v1: 0x646f_7261_6e64_6f6d,
            v2: 0x6c79_6765_6e65_7261,
            v3: 0x7465_6462_7974_6573,
            tail: [0; 8],
            tail_len: 0,
            length: 0,
        }
    }

    fn write(&mut self, mut bytes: &[u8]) {
        self.length = self.length.wrapping_add(bytes.len() as u64);
        if self.tail_len > 0 {
            let take = (8 - self.tail_len).min(bytes.len());
            self.tail[self.tail_len..self.tail_len + take].copy_from_slice(&bytes[..take]);
            self.tail_len += take;
            bytes = &bytes[take..];
            if self.tail_len < 8 {
                return;
            }
            self.compress(u64::from_le_bytes(self.tail));
            self.tail_len = 0;
        }
        let (blocks, rest) = bytes.as_chunks::<8>();
        for block in blocks {
            self.compress(u64::from_le_bytes(*block));
        }
        self.tail[..rest.len()].copy_from_slice(rest);
        self.tail_len = rest.len();
    }

    fn finish(mut self) -> u64 {
        // 最后一块：剩余字节在低位，总长度的低 8 位放在最高字节
        let mut last = [0_u8; 8];
        last[..self.tail_len].copy_from_slice(&self.tail[..self.tail_len]);
        last[7] = self.length as u8;
        self.compress(u64::from_le_bytes(last));
        self.v2 ^= 0xff;
        for _ in 0..3 {
            self.round();
        }
        self.v0 ^ self.v1 ^ self.v2 ^ self.v3
    }

    fn compress(&mut self, block: u64) {
        self.v3 ^= block;
        self.round();
        self.v0 ^= block;
    }

    fn round(&mut self) {
        self.v0 = self.v0.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(13);
        self.v1 ^= self.v0;
        self.v0 = self.v0.rotate_left(32);
        self.v2 = self.v2.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(16);
        self.v3 ^= self.v2;
        self.v0 = self.v0.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(21);
        self.v3 ^= self.v0;
        self.v2 = self.v2.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(17);
        self.v1 ^= self.v2;
        self.v2 = self.v2.rotate_left(32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn std_hash<T: Hash + ?Sized>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    /// 这条对照证明「换成自己的实现不改变任何已有 ID」。
    ///
    /// 如果它在**升级 Rust 工具链之后**变红，说明标准库的算法或分帧方式真的变了——那正是
    /// 本模块要防的情况：曲库里存的 ID 以本模块的输出为准（由下面的固定向量锁住），
    /// 此时把这条对照删掉即可，**不要**去改本模块的实现。
    #[test]
    fn matches_the_std_default_hasher_it_replaces() {
        let text = "D:\\Music\\专辑 Album\\01 - 曲目 Track (Live).flac";
        for end in 0..=text.len() {
            if text.is_char_boundary(end) {
                assert_eq!(hash_str(&text[..end]), std_hash(&text[..end]), "{end}");
            }
        }
        let bytes: Vec<u8> = (0..300_u32).map(|value| (value * 7 + 3) as u8).collect();
        for end in 0..=bytes.len() {
            assert_eq!(hash_bytes(&bytes[..end]), std_hash(&bytes[..end]), "{end}");
        }
        // `Vec<u8>` 与 `String` 走的是同一套切片 / 字符串实现
        assert_eq!(hash_bytes(&bytes), std_hash(&bytes));
        assert_eq!(hash_str(text), std_hash(&text.to_string()));
    }

    #[test]
    fn output_does_not_depend_on_how_the_input_is_split_across_writes() {
        let bytes: Vec<u8> = (0..64_u8).collect();
        let mut whole = SipHasher13::new();
        whole.write(&bytes);
        let expected = whole.finish();
        for split in 0..=bytes.len() {
            let mut hasher = SipHasher13::new();
            hasher.write(&bytes[..split]);
            hasher.write(&bytes[split..]);
            assert_eq!(hasher.finish(), expected, "{split}");
        }
        let mut bytewise = SipHasher13::new();
        for byte in &bytes {
            bytewise.write(std::slice::from_ref(byte));
        }
        assert_eq!(bytewise.finish(), expected);
    }

    /// 固定输入的输出（2026-11-25 用 Rust 1.98.1 的 `DefaultHasher` 核对过）。
    /// 这些值就是落在用户磁盘上的标识的算法定义：**任何让它们变化的改动都会让已有曲库的
    /// 收藏、歌单、歌词全部脱钩**，变红时改实现、不改这里的期望值。
    #[test]
    fn fixed_vectors_never_change() {
        for (text, expected) in [
            ("", 0x3040_6ea5_23c5_3def_u64),
            ("a", 0x719b_50b9_a4f0_e9f3),
            ("d:\\music\\song.flac", 0xee63_3971_b3d0_9441),
            ("扬声器 (USB DAC)", 0x0dbb_a07c_4af6_d6b6),
        ] {
            assert_eq!(hash_str(text), expected, "{text:?}");
        }
        for (bytes, expected) in [
            (&b""[..], 0xbd60_acb6_58c7_9e45_u64),
            (&b"\x89PNG\r\n\x1a\n"[..], 0x86b3_22c8_dde2_bcc0),
            (&[0xff_u8; 17][..], 0x5ab7_10cc_6613_0f45),
        ] {
            assert_eq!(hash_bytes(bytes), expected, "{bytes:02x?}");
        }
    }
}
