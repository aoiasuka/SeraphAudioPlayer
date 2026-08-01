//! 缩略图按钮图标:⏮ ⏯ ⏭ 形状的纯光栅化 + HICON 胶水。
//!
//! 不依赖图片资源:形状在单位空间 [0,1]² 内用几何定义,4×4 超采样抗锯齿
//! 光栅化为 32bpp 预乘 BGRA(top-down)。颜色运行时按任务栏主题选择——
//! 浅色任务栏用档案墨色、深色任务栏用牛皮纸白,与应用设计语言同源。

use core::ffi::c_void;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

/// 档案墨色(浅色任务栏用),同 CSS `--ink`。
pub const INK: (u8, u8, u8) = (0x2b, 0x27, 0x22);
/// 牛皮纸白(深色任务栏用),同 CSS `--paper`。
pub const PAPER: (u8, u8, u8) = (0xf5, 0xf1, 0xe8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconGlyph {
    Prev,
    Play,
    Pause,
    Next,
}

/// 超采样密度(每像素 N×N 子采样)。
const SUPERSAMPLE: u32 = 4;

/// 光栅化为 32bpp 预乘 BGRA(top-down 行序)缓冲,长度 size*size*4。
pub fn rasterize(glyph: IconGlyph, size: usize, color: (u8, u8, u8)) -> Vec<u8> {
    let mut out = vec![0u8; size * size * 4];
    let n = SUPERSAMPLE;
    let samples = n * n;

    for y in 0..size {
        for x in 0..size {
            let mut hits = 0u32;
            for sy in 0..n {
                for sx in 0..n {
                    let u = (x as f32 + (sx as f32 + 0.5) / n as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / n as f32) / size as f32;
                    if inside(glyph, u, v) {
                        hits += 1;
                    }
                }
            }
            let alpha = (hits * 255 / samples) as u8;
            let index = (y * size + x) * 4;
            // 预乘 alpha,通道序 BGRA
            out[index] = premultiply(color.2, alpha);
            out[index + 1] = premultiply(color.1, alpha);
            out[index + 2] = premultiply(color.0, alpha);
            out[index + 3] = alpha;
        }
    }
    out
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    (channel as u32 * alpha as u32 / 255) as u8
}

/// 单位空间形状判定。Prev 定义为 Next 的水平镜像,保证两者严格对称。
fn inside(glyph: IconGlyph, u: f32, v: f32) -> bool {
    match glyph {
        IconGlyph::Play => in_triangle(u, v, (0.22, 0.12), (0.22, 0.88), (0.90, 0.50)),
        IconGlyph::Pause => {
            ((0.20..=0.42).contains(&u) || (0.58..=0.80).contains(&u)) && (0.14..=0.86).contains(&v)
        }
        IconGlyph::Next => {
            in_triangle(u, v, (0.14, 0.16), (0.14, 0.84), (0.64, 0.50))
                || ((0.70..=0.86).contains(&u) && (0.16..=0.84).contains(&v))
        }
        IconGlyph::Prev => inside(IconGlyph::Next, 1.0 - u, v),
    }
}

fn in_triangle(u: f32, v: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    fn edge(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
        (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
    }
    let p = (u, v);
    let d1 = edge(a, b, p);
    let d2 = edge(b, c, p);
    let d3 = edge(c, a, p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// BGRA 缓冲 → HICON。失败(GDI 资源耗尽等)返回 None,调用方降级为无图标。
///
/// # Safety
/// 仅调用 GDI/User32,无前置条件;标记 unsafe 只因内部裸指针写入。
pub unsafe fn create_hicon(glyph: IconGlyph, size: usize, color: (u8, u8, u8)) -> Option<HICON> {
    let pixels = rasterize(glyph, size, color);

    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: size as i32,
        // 负高度 = top-down 行序,与 rasterize 输出一致
        biHeight: -(size as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0, // BI_RGB
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };

    let mut bits: *mut c_void = std::ptr::null_mut();
    let color_bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    if bits.is_null() {
        let _ = DeleteObject(color_bitmap.into());
        return None;
    }
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());

    // 32bpp 带 alpha 的图标仍需一张占位 AND mask
    let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
    let icon_info = ICONINFO {
        fIcon: true.into(),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color_bitmap,
    };
    let icon = CreateIconIndirect(&icon_info).ok();

    let _ = DeleteObject(color_bitmap.into());
    if !mask.is_invalid() {
        let _ = DeleteObject(mask.into());
    }
    icon
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 32;
    const COLOR: (u8, u8, u8) = (0xb5, 0x48, 0x2a); // 印章红,便于验证通道序

    fn alpha_at(buf: &[u8], x: usize, y: usize) -> u8 {
        buf[(y * SIZE + x) * 4 + 3]
    }

    #[test]
    fn buffer_has_expected_length() {
        let buf = rasterize(IconGlyph::Play, SIZE, COLOR);
        assert_eq!(buf.len(), SIZE * SIZE * 4);
    }

    #[test]
    fn play_triangle_covers_center_not_corners_nor_past_tip() {
        let buf = rasterize(IconGlyph::Play, SIZE, COLOR);
        assert_eq!(alpha_at(&buf, 16, 16), 255, "三角形中心应不透明");
        assert_eq!(alpha_at(&buf, 0, 0), 0, "角落应透明");
        assert_eq!(alpha_at(&buf, 31, 31), 0, "角落应透明");
        assert_eq!(alpha_at(&buf, 30, 16), 0, "三角形尖端(0.90)之外应透明");
    }

    #[test]
    fn play_has_antialiased_edge_pixels() {
        let buf = rasterize(IconGlyph::Play, SIZE, COLOR);
        let partial = (0..SIZE * SIZE)
            .map(|i| buf[i * 4 + 3])
            .any(|alpha| alpha > 0 && alpha < 255);
        assert!(partial, "斜边应存在半透明抗锯齿像素");
    }

    #[test]
    fn pause_has_two_bars_with_center_gap() {
        let buf = rasterize(IconGlyph::Pause, SIZE, COLOR);
        assert_eq!(alpha_at(&buf, 10, 16), 255, "左竖条应不透明");
        assert_eq!(alpha_at(&buf, 22, 16), 255, "右竖条应不透明");
        assert_eq!(alpha_at(&buf, 16, 16), 0, "两条之间应留缝");
        assert_eq!(alpha_at(&buf, 10, 1), 0, "竖条上方应透明");
    }

    #[test]
    fn next_has_right_bar_and_gap_between_triangle_and_bar() {
        let buf = rasterize(IconGlyph::Next, SIZE, COLOR);
        assert_eq!(alpha_at(&buf, 6, 16), 255, "左侧三角应不透明");
        assert_eq!(alpha_at(&buf, 25, 16), 255, "右侧竖条应不透明");
        assert_eq!(alpha_at(&buf, 21, 16), 0, "三角与竖条之间应留缝");
    }

    #[test]
    fn prev_is_horizontal_mirror_of_next() {
        let next = rasterize(IconGlyph::Next, SIZE, COLOR);
        let prev = rasterize(IconGlyph::Prev, SIZE, COLOR);
        assert_eq!(alpha_at(&prev, 6, 16), 255, "镜像后竖条在左");
        assert_eq!(alpha_at(&prev, 25, 16), 255, "镜像后三角在右");
        assert_eq!(alpha_at(&prev, 10, 16), 0, "镜像后竖条与三角之间留缝");
        // 整幅按行镜像逐像素相等
        for y in 0..SIZE {
            for x in 0..SIZE {
                assert_eq!(
                    alpha_at(&prev, x, y),
                    alpha_at(&next, SIZE - 1 - x, y),
                    "({x},{y}) 处镜像不对称"
                );
            }
        }
    }

    #[test]
    fn output_is_premultiplied_bgra() {
        let buf = rasterize(IconGlyph::Pause, SIZE, COLOR);
        // 不透明像素:BGRA = (0x2a, 0x48, 0xb5, 0xff)
        let index = (16 * SIZE + 10) * 4;
        assert_eq!(&buf[index..index + 4], &[0x2a, 0x48, 0xb5, 0xff]);
        // 全透明像素预乘后各通道为 0
        let index = 0;
        assert_eq!(&buf[index..index + 4], &[0, 0, 0, 0]);
    }
}
