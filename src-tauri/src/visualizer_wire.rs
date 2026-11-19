//! 可视化帧的二进制编码（VIZ-01）。
//!
//! 两条取帧命令此前返回 JSON：一帧分析数据约三千个数，后端每秒 30 次把它们
//! 格式化成十进制文本，前端再逐个解析回来。现在改成小端字节直传
//! （`tauri::ipc::Response`），前端在响应缓冲上直接建 `Float32Array` /
//! `Int16Array` 视图，不再经过文本。
//!
//! 布局与前端 `src/lib/analysis/wire.ts` 一一对应。两边各有一份回归测试，
//! 对的是同一组黄金字节（`src/lib/analysis/__fixtures__/*.hex`，最初按下面的
//! 布局表用独立脚本算出，不是从本模块的输出抄来的）——
//! **改布局必须两边一起改**：先按新布局改 .hex，再让两边的测试各自转绿。
//!
//! 约定：
//! - 全部小端；头部长度是 4 的倍数，`f32` 数组紧随其后，`i16` 数组排在所有
//!   `f32` 之后，前端的类型化数组视图因此天然对齐；
//! - 数组和必填标量里的非有限值一律写成 0。JSON 时代 serde 把它们写成 `null`，
//!   前端按 0 处理；二进制里的 NaN 会原样流进前端的平滑状态并一直留在里面；
//! - 可空标量（响度各项）用 NaN 表示「暂无」，非有限值同样按「暂无」；
//! - 请求身份（UTF-8）放在帧尾，长度就是剩余字节数；
//! - 还没有帧时命令返回空字节串。

use crate::visualizer_service::{AnalysisFrameDto, SpectrumFrameDto};

/// 频谱帧（侧栏 48 柱）：
///
/// | 偏移 | 类型 | 字段 |
/// |---|---|---|
/// | 0 | `[u8; 4]` | 魔数 `SSP1` |
/// | 4 | `u32` | 柱数 N |
/// | 8 | `u64` | 帧序号 |
/// | 16 | `f32` | 左声道峰值 |
/// | 20 | `f32` | 右声道峰值 |
/// | 24 | `f32 × N` | 各柱高度 |
/// | 24 + 4N | UTF-8 | 请求身份（到帧尾） |
const SPECTRUM_MAGIC: [u8; 4] = *b"SSP1";
const SPECTRUM_HEADER_BYTES: usize = 24;

/// 分析帧（分析页与沉浸页）：
///
/// | 偏移 | 类型 | 字段 |
/// |---|---|---|
/// | 0 | `[u8; 4]` | 魔数 `SAN1` |
/// | 4 | `u32` | 频谱点数 S |
/// | 8 | `u32` | 散点浮点数个数 C（成对的 L、R） |
/// | 12 | `u32` | 波形点数 W（交错的 L、R） |
/// | 16 | `u64` | 帧序号 |
/// | 24 | `u32` | 采样率 |
/// | 28 | `f32 × 5` | 左峰值、右峰值、左 RMS、右 RMS、相关度 |
/// | 48 | `f32 × 6` | 瞬时、短期、积分响度、响度范围、真峰值、真峰值最大（NaN = 暂无） |
/// | 72 | `f32 × S` | 频谱 |
/// | 72 + 4S | `f32 × C` | 散点 |
/// | 72 + 4S + 4C | `i16 × W` | 波形 |
/// | 其后 | UTF-8 | 请求身份（到帧尾） |
const ANALYSIS_MAGIC: [u8; 4] = *b"SAN1";
const ANALYSIS_HEADER_BYTES: usize = 72;

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_len(out: &mut Vec<u8>, len: usize) {
    // 各数组由分析线程生成，长度是几十到几千；真超出 u32 就按上限写，
    // 前端会因为长度对不上帧长而判为畸形帧丢弃。
    put_u32(out, u32::try_from(len).unwrap_or(u32::MAX));
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// 必填的浮点数：非有限值写成 0。
fn put_f32(out: &mut Vec<u8>, value: f32) {
    let value = if value.is_finite() { value } else { 0.0 };
    out.extend_from_slice(&value.to_le_bytes());
}

/// 可空的浮点数：`None` 与非有限值都写成同一个 NaN。
fn put_optional_f32(out: &mut Vec<u8>, value: Option<f32>) {
    let value = value.filter(|value| value.is_finite()).unwrap_or(f32::NAN);
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_f32_slice(out: &mut Vec<u8>, values: &[f32]) {
    for &value in values {
        put_f32(out, value);
    }
}

pub(crate) fn encode_spectrum_frame(frame: &SpectrumFrameDto) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(SPECTRUM_HEADER_BYTES + frame.bins.len() * 4 + frame.request_id.len());
    out.extend_from_slice(&SPECTRUM_MAGIC);
    put_len(&mut out, frame.bins.len());
    put_u64(&mut out, frame.sequence);
    put_f32(&mut out, frame.peak_left);
    put_f32(&mut out, frame.peak_right);
    debug_assert_eq!(out.len(), SPECTRUM_HEADER_BYTES);
    put_f32_slice(&mut out, &frame.bins);
    out.extend_from_slice(frame.request_id.as_bytes());
    out
}

pub(crate) fn encode_analysis_frame(frame: &AnalysisFrameDto) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        ANALYSIS_HEADER_BYTES
            + (frame.spectrum.len() + frame.scatter.len()) * 4
            + frame.waveform.len() * 2
            + frame.request_id.len(),
    );
    out.extend_from_slice(&ANALYSIS_MAGIC);
    put_len(&mut out, frame.spectrum.len());
    put_len(&mut out, frame.scatter.len());
    put_len(&mut out, frame.waveform.len());
    put_u64(&mut out, frame.sequence);
    put_u32(&mut out, frame.sample_rate);
    put_f32(&mut out, frame.peak_left);
    put_f32(&mut out, frame.peak_right);
    put_f32(&mut out, frame.rms_left);
    put_f32(&mut out, frame.rms_right);
    put_f32(&mut out, frame.correlation);
    put_optional_f32(&mut out, frame.momentary_lufs);
    put_optional_f32(&mut out, frame.short_term_lufs);
    put_optional_f32(&mut out, frame.integrated_lufs);
    put_optional_f32(&mut out, frame.loudness_range_lu);
    put_optional_f32(&mut out, frame.true_peak_db);
    put_optional_f32(&mut out, frame.true_peak_max_db);
    debug_assert_eq!(out.len(), ANALYSIS_HEADER_BYTES);
    put_f32_slice(&mut out, &frame.spectrum);
    put_f32_slice(&mut out, &frame.scatter);
    for &point in &frame.waveform {
        out.extend_from_slice(&point.to_le_bytes());
    }
    out.extend_from_slice(frame.request_id.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与前端 `wire.test.ts` 共用的黄金字节（十六进制文本）。
    const SPECTRUM_GOLDEN: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/lib/analysis/__fixtures__/spectrum-frame.hex"
    ));
    const ANALYSIS_GOLDEN: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/lib/analysis/__fixtures__/analysis-frame.hex"
    ));

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// 黄金帧的取值与 `wire.test.ts` 里断言的一致；改这里要同步改那边和 .hex。
    fn golden_spectrum() -> SpectrumFrameDto {
        SpectrumFrameDto {
            bins: vec![0.0, 0.25, 1.0],
            peak_left: 0.5,
            peak_right: 0.75,
            request_id: "session-1:7".into(),
            // 超过 32 位，确认序号按 64 位写出
            sequence: (1 << 32) + 2,
        }
    }

    fn golden_analysis() -> AnalysisFrameDto {
        AnalysisFrameDto {
            spectrum: vec![0.5, 1.0],
            peak_left: 0.5,
            peak_right: 0.25,
            rms_left: 0.125,
            rms_right: 0.0625,
            momentary_lufs: Some(-23.5),
            short_term_lufs: Some(-20.25),
            integrated_lufs: None,
            loudness_range_lu: Some(6.5),
            true_peak_db: Some(-1.25),
            true_peak_max_db: None,
            correlation: -0.5,
            scatter: vec![0.25, -0.25, -1.0, 1.0],
            waveform: vec![0, i16::MAX, i16::MIN, -1],
            sample_rate: 96_000,
            // 非 ASCII，确认请求身份按 UTF-8 原样回传
            request_id: "曲目-α:12".into(),
            // JS 能精确表示的最大整数
            sequence: (1 << 53) - 1,
        }
    }

    #[test]
    fn spectrum_frame_matches_the_golden_bytes_shared_with_the_frontend() {
        assert_eq!(
            hex(&encode_spectrum_frame(&golden_spectrum())),
            SPECTRUM_GOLDEN.trim()
        );
    }

    #[test]
    fn analysis_frame_matches_the_golden_bytes_shared_with_the_frontend() {
        assert_eq!(
            hex(&encode_analysis_frame(&golden_analysis())),
            ANALYSIS_GOLDEN.trim()
        );
    }

    #[test]
    fn frame_length_follows_the_documented_layout() {
        let spectrum = golden_spectrum();
        assert_eq!(
            encode_spectrum_frame(&spectrum).len(),
            SPECTRUM_HEADER_BYTES + spectrum.bins.len() * 4 + spectrum.request_id.len()
        );
        let analysis = golden_analysis();
        assert_eq!(
            encode_analysis_frame(&analysis).len(),
            ANALYSIS_HEADER_BYTES
                + (analysis.spectrum.len() + analysis.scatter.len()) * 4
                + analysis.waveform.len() * 2
                + analysis.request_id.len()
        );
        // 两个头都是 4 的倍数，f32 数组在前端才对得齐
        assert_eq!(SPECTRUM_HEADER_BYTES % 4, 0);
        assert_eq!(ANALYSIS_HEADER_BYTES % 4, 0);
    }

    fn f32_at(bytes: &[u8], offset: usize) -> f32 {
        f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn non_finite_values_never_reach_the_frontend_as_nan_in_arrays_or_required_fields() {
        let mut frame = golden_analysis();
        frame.peak_left = f32::NAN;
        frame.rms_right = f32::INFINITY;
        frame.correlation = f32::NEG_INFINITY;
        frame.spectrum = vec![f32::NAN, 0.5];
        frame.scatter = vec![f32::INFINITY, -0.5];
        let bytes = encode_analysis_frame(&frame);
        assert_eq!(f32_at(&bytes, 28), 0.0, "左峰值");
        assert_eq!(f32_at(&bytes, 40), 0.0, "右 RMS");
        assert_eq!(f32_at(&bytes, 44), 0.0, "相关度");
        assert_eq!(f32_at(&bytes, 72), 0.0, "频谱里的 NaN");
        assert_eq!(f32_at(&bytes, 76), 0.5);
        assert_eq!(f32_at(&bytes, 80), 0.0, "散点里的无穷");
        assert_eq!(f32_at(&bytes, 84), -0.5);

        let mut spectrum = golden_spectrum();
        spectrum.peak_right = f32::NAN;
        spectrum.bins = vec![f32::NEG_INFINITY];
        let bytes = encode_spectrum_frame(&spectrum);
        assert_eq!(f32_at(&bytes, 20), 0.0);
        assert_eq!(f32_at(&bytes, 24), 0.0);
    }

    #[test]
    fn missing_and_non_finite_optional_values_are_both_encoded_as_nan() {
        let mut frame = golden_analysis();
        frame.momentary_lufs = None;
        frame.short_term_lufs = Some(f32::NEG_INFINITY);
        frame.integrated_lufs = Some(f32::NAN);
        frame.loudness_range_lu = Some(7.0);
        let bytes = encode_analysis_frame(&frame);
        assert!(f32_at(&bytes, 48).is_nan());
        assert!(f32_at(&bytes, 52).is_nan());
        assert!(f32_at(&bytes, 56).is_nan());
        assert_eq!(f32_at(&bytes, 60), 7.0);
    }
}
