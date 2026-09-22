//! QQ 音乐 QRC 歌词用的「DES」变体（本地 `.qrc` 与云端 `GetPlayLyricInfo` 载荷共用）。
//!
//! 2026-09-19 实测：QQ 的实现**不是**标准 DES，与 FIPS 46-3 相比有三类差异——
//!
//! 1. S 盒两处取值不同（S2 第 2 行第 8 列 14 → 15、S4 第 4 行第 6 列 1 → 10）；
//! 2. PC-2 压缩置换的 D 半部分整体**偏移一位**（取第 n+1 位而非第 n 位），偏移后越界的那一位
//!    恒为 0——即 16 个子密钥里各有 1 位永远是 0；
//! 3. 8 字节数据块与 8 字节子密钥都按「每 4 字节一组、组内字节序反转」读写
//!    （原实现把块当成两个小端 32 位字处理，IP / FP / PC-1 的位号都建立在这个字节序上）。
//!
//! 用标准 3DES（此前的 `des` crate）解真实密文只会得到乱码、随后 zlib 报 corrupt。其余部分
//! （IP / E / P / PC-1 / PC-2 的 C 半部分 / 轮结构、3DES EDE 三把子密钥顺序）与标准一致。本文件按
//! FIPS 46-3 的公开表独立实现（表驱动，u64 位运算）并保留上述差异，不依赖任何第三方密码库；
//! 只提供解密（歌词只读）。
//!
//! 已知答案向量见测试：既有独立实现生成的多块向量，也有一块取自真实 QQ 响应的密文 → zlib 头。

/// 初始置换 IP（FIPS 表，1 起始位号，从最高位数起）。
const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

/// 末置换 IP⁻¹。
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];

/// 扩展置换 E：32 位 → 48 位。
const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];

/// 轮函数末尾的 P 置换。
const P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];

/// 密钥置换 PC-1：64 位密钥 → 56 位（去奇偶位）。
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];

/// 密钥压缩置换 PC-2 的 C 半部分：标准表前 24 项（1 起始，取自 28 位 C）。
const PC2_C: [u8; 24] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2,
];

/// PC-2 的 D 半部分（1 起始，取自 28 位 D）：标准表后 24 项减 28 后**再加 1**——QQ 的偏移一位；
/// 由标准 56 → 29 得到的 29 超出 28 位 D 的范围，该位恒为 0。
const PC2_D_QQ: [u8; 24] = [
    14, 25, 4, 10, 20, 28, 3, 13, 24, 18, 6, 21, 17, 22, 12, 29, 7, 26, 19, 15, 23, 9, 2, 5,
];

/// 每轮 C / D 半密钥的循环左移位数。
const SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

/// S 盒，FIPS 布局（每盒 4 行 × 16 列，行由 6 位输入的首尾两位选，列由中间四位选）。
/// 与标准表的差异只有两处，见文件头；写测试对照标准表时别把它们「修正」回去。
const SBOXES: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        // 第 2 行第 8 列：标准 DES 为 14，QQ 为 15
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        // 第 4 行第 6 列：标准 DES 为 1，QQ 为 10
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10,
        10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// 按 FIPS 表做位置换：`table` 是 1 起始的输入位号（从最高位数起），输出按表序从高位向低位填。
/// 只在建表与密钥展开时逐位跑；块变换走下面的查表实现（置换对 XOR 线性，可按字节拆分）。
fn permute(input: u64, input_bits: u32, table: &[u8]) -> u64 {
    table.iter().fold(0, |out, &pos| {
        (out << 1) | ((input >> (input_bits - u32::from(pos))) & 1)
    })
}

/// 查表：IP / FP 按 8 个输入字节各一张 256 项表，E 按 4 个字节，S 盒 + P 合成 8 张 64 项表。
/// 一块 3DES 从 ≈4600 次逐位取放降到 ≈400 次查表；一首歌几千块，QQ 在线取词与本地 `.qrc`
/// 的解密时间随之下降一个量级（2026-09-22 基准）。
struct Tables {
    ip: [[u64; 256]; 8],
    fp: [[u64; 256]; 8],
    e: [[u64; 256]; 4],
    sp: [[u32; 64]; 8],
}

fn tables() -> &'static Tables {
    static TABLES: std::sync::OnceLock<Box<Tables>> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let mut tables = Box::new(Tables {
            ip: [[0; 256]; 8],
            fp: [[0; 256]; 8],
            e: [[0; 256]; 4],
            sp: [[0; 64]; 8],
        });
        for byte_index in 0..8 {
            for value in 0..256 {
                let input = (value as u64) << (56 - 8 * byte_index);
                tables.ip[byte_index][value] = permute(input, 64, &IP);
                tables.fp[byte_index][value] = permute(input, 64, &FP);
            }
        }
        for byte_index in 0..4 {
            for value in 0..256 {
                let input = (value as u64) << (24 - 8 * byte_index);
                tables.e[byte_index][value] = permute(input, 32, &E);
            }
        }
        for (box_index, sbox) in SBOXES.iter().enumerate() {
            for six in 0..64 {
                let row = ((six & 0x20) >> 4) | (six & 1);
                let column = (six >> 1) & 0x0f;
                let nibble = u64::from(sbox[row * 16 + column]) << (28 - 4 * box_index);
                tables.sp[box_index][six] = permute(nibble, 32, &P) as u32;
            }
        }
        tables
    })
}

fn permute_64(tables: &[[u64; 256]; 8], input: u64) -> u64 {
    let bytes = input.to_be_bytes();
    tables
        .iter()
        .zip(bytes)
        .fold(0, |out, (table, byte)| out ^ table[usize::from(byte)])
}

fn rotate_left_28(value: u32, shift: u8) -> u32 {
    ((value << shift) | (value >> (28 - u32::from(shift)))) & 0x0fff_ffff
}

/// QQ 字节序：8 字节按两个 4 字节组读入，组内字节反转（数据块与密钥同一约定）。
fn load_block(bytes: &[u8; 8]) -> u64 {
    u64::from_be_bytes([
        bytes[3], bytes[2], bytes[1], bytes[0], bytes[7], bytes[6], bytes[5], bytes[4],
    ])
}

fn store_block(value: u64) -> [u8; 8] {
    let b = value.to_be_bytes();
    [b[3], b[2], b[1], b[0], b[7], b[6], b[5], b[4]]
}

/// 单把 DES 密钥展开成 16 个 48 位子密钥（加密顺序）。密钥按 QQ 字节序读入。
fn subkeys(key: [u8; 8]) -> [u64; 16] {
    let permuted = permute(load_block(&key), 64, &PC1);
    let mut c = ((permuted >> 28) & 0x0fff_ffff) as u32;
    let mut d = (permuted & 0x0fff_ffff) as u32;
    let mut keys = [0_u64; 16];
    for (round, shift) in SHIFTS.iter().enumerate() {
        c = rotate_left_28(c, *shift);
        d = rotate_left_28(d, *shift);
        let c_bits = permute(u64::from(c), 28, &PC2_C);
        // D 半部分：越界位（29）恒 0
        let d_bits = PC2_D_QQ.iter().fold(0_u64, |out, &pos| {
            let bit = if pos > 28 {
                0
            } else {
                u64::from((d >> (28 - u32::from(pos))) & 1)
            };
            (out << 1) | bit
        });
        keys[round] = (c_bits << 24) | d_bits;
    }
    keys
}

fn feistel(tables: &Tables, right: u32, subkey: u64) -> u32 {
    let bytes = right.to_be_bytes();
    let expanded = tables
        .e
        .iter()
        .zip(bytes)
        .fold(0_u64, |out, (table, byte)| out ^ table[usize::from(byte)]);
    let mixed = expanded ^ subkey;
    tables
        .sp
        .iter()
        .enumerate()
        .fold(0_u32, |out, (index, table)| {
            out ^ table[((mixed >> (42 - 6 * index)) & 0x3f) as usize]
        })
}

/// 一次 DES 块变换；`keys` 按加密顺序传入即加密，反序传入即解密。
fn des_block(block: u64, keys: impl Iterator<Item = u64>) -> u64 {
    let tables = tables();
    let permuted = permute_64(&tables.ip, block);
    let (mut left, mut right) = ((permuted >> 32) as u32, permuted as u32);
    for key in keys {
        let next = left ^ feistel(tables, right, key);
        left = right;
        right = next;
    }
    permute_64(&tables.fp, (u64::from(right) << 32) | u64::from(left))
}

/// QQ 变体 3DES（EDE，24 字节密钥）解密器：`D_K1(E_K2(D_K3(密文)))`。
pub(crate) struct QqTripleDes {
    k1: [u64; 16],
    k2: [u64; 16],
    k3: [u64; 16],
}

/// `QRC_KEY` 的解密器只展开一次（密钥是常量；此前每解一份歌词都重做三把密钥的 16 轮展开）。
pub(crate) fn qrc_cipher() -> &'static QqTripleDes {
    static CIPHER: std::sync::OnceLock<QqTripleDes> = std::sync::OnceLock::new();
    CIPHER.get_or_init(|| QqTripleDes::new(super::lyrics::QRC_KEY))
}

impl QqTripleDes {
    pub(crate) fn new(key: &[u8; 24]) -> Self {
        let part = |offset: usize| {
            let mut bytes = [0_u8; 8];
            bytes.copy_from_slice(&key[offset..offset + 8]);
            subkeys(bytes)
        };
        Self {
            k1: part(0),
            k2: part(8),
            k3: part(16),
        }
    }

    pub(crate) fn decrypt_block(&self, block: &mut [u8; 8]) {
        let mut value = load_block(block);
        value = des_block(value, self.k3.iter().rev().copied());
        value = des_block(value, self.k2.iter().copied());
        value = des_block(value, self.k1.iter().rev().copied());
        *block = store_block(value);
    }

    #[cfg(test)]
    pub(crate) fn encrypt_block(&self, block: &mut [u8; 8]) {
        let mut value = load_block(block);
        value = des_block(value, self.k1.iter().copied());
        value = des_block(value, self.k2.iter().rev().copied());
        value = des_block(value, self.k3.iter().copied());
        *block = store_block(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8; 24] = b"!@#)(*$%123ZXC!@!@#)(NHL";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn block(hex: &str) -> [u8; 8] {
        let mut out = [0_u8; 8];
        for (index, slot) in out.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("hex");
        }
        out
    }

    #[test]
    fn real_qq_cipher_block_decrypts_to_zlib_header() {
        // 取自真实 `GetPlayLyricInfo` 响应（王力宏《唯一》songid 1121156）的首个密文块：
        // 明文以 zlib 头 78 9c 开始。标准 3DES 解同一块得到 de eb 93 1d …（乱码）。
        let des = QqTripleDes::new(KEY);
        let mut data = block("21f1c7547b306d28");
        des.decrypt_block(&mut data);
        assert_eq!(hex(&data), "789c55584b8f94c7");
    }

    #[test]
    fn known_answer_vectors_round_trip() {
        // 由独立实现生成的多块向量（加密方向），解密回原文且加密再得密文
        let des = QqTripleDes::new(KEY);
        for (plain, cipher) in [
            ("0000000000000000", "a27b02aa779bf226"),
            ("789c55584b8f94c7", "21f1c7547b306d28"),
            ("0123456789abcdef", "6c6467a1fc44c95b"),
            ("ffffffffffffffff", "4b789c44381642a3"),
        ] {
            let mut data = block(cipher);
            des.decrypt_block(&mut data);
            assert_eq!(hex(&data), plain, "decrypt {cipher}");
            des.encrypt_block(&mut data);
            assert_eq!(hex(&data), cipher, "encrypt {plain}");
        }
    }

    #[test]
    fn single_des_matches_reference_for_first_subkey() {
        // 逐层排错用：只用 K1 的单 DES 加密向量（数据与密钥都走 QQ 字节序）
        let mut key = [0_u8; 8];
        key.copy_from_slice(&KEY[..8]);
        let keys = subkeys(key);
        for (plain, cipher) in [
            ("0000000000000000", "8f7758b4db4d359e"),
            ("0123456789abcdef", "a2753fc39b04be29"),
        ] {
            let out = store_block(des_block(load_block(&block(plain)), keys.iter().copied()));
            assert_eq!(hex(&out), cipher, "plain {plain}");
        }
    }

    #[test]
    fn block_byte_order_round_trips() {
        let bytes = block("0011223344556677");
        assert_eq!(load_block(&bytes), 0x3322_1100_7766_5544);
        assert_eq!(store_block(load_block(&bytes)), bytes);
    }

    #[test]
    fn sboxes_differ_from_fips_only_at_the_two_known_cells() {
        // 标准 DES 的两处取值；其余 510 个格子与 FIPS 46-3 一致（防止日后被当成笔误「修正」）
        assert_eq!(SBOXES[1][16 + 7], 15, "S2 row 2 col 8 (FIPS: 14)");
        assert_eq!(SBOXES[3][48 + 5], 10, "S4 row 4 col 6 (FIPS: 1)");
        let fips_s2_row2: [u8; 16] = [3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5];
        let fips_s4_row4: [u8; 16] = [3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14];
        for (column, expected) in fips_s2_row2.iter().enumerate() {
            if column != 7 {
                assert_eq!(SBOXES[1][16 + column], *expected);
            }
        }
        for (column, expected) in fips_s4_row4.iter().enumerate() {
            if column != 5 {
                assert_eq!(SBOXES[3][48 + column], *expected);
            }
        }
    }
}
