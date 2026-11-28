/**
 * 声场散点的密度参数（沉浸页与分析页共用）。
 *
 * 散点稀不稀，主要不取决于画布多大，而取决于音乐本身：声场宽、电平高的曲目会把点铺满
 * 整个菱形，同样多的墨就显得稀。所以这里不按画布分档，而是让墨量本身够用——每帧多取
 * 真实采样（后端按 `fieldPoints` 抽取，不是把同一批点复制几遍）、余晖留得更久、点略大。
 *
 * 2026-11-28 的教训：上一版按画布的 CSS 尺寸分三档（480 / 960 / 1440），门槛是半径
 * 152 px。可是系统缩放 150% 的 2560×1440 显示器上，沉浸页的声场半径只有约 110 px，
 * 永远到不了第二档——改了等于没改。**调这些参数要看图**：`scripts/field-preview.html`
 * 的预览页按真实的映射与像素对齐渲染改前改后（见项目 CLAUDE.md）。
 *
 * 下面的 `extent` 都是声场图形的「半径」：菱形的半对角线、或半圆的半径，CSS 像素。
 */
import { clamp } from "./types";

/**
 * 沉浸页每帧向后端要多少对采样点。等于后端的上限（`seraph_visualizer::analysis::
 * MAX_SCATTER_PAIRS`）：44.1 kHz 下一帧约 1455 个样本，几乎全取。
 */
export const IMMERSIVE_FIELD_POINTS = 1440;

/** 分析页每帧要多少对。那里的点本来就大（2.2 px 起），1440 在窄声场下会糊成一条。 */
export const PAGE_FIELD_POINTS = 960;

/**
 * 沉浸页余晖各帧的不透明度，最新的一帧在前：0.7 × 0.8^k，共 10 帧（约 1/3 秒）。
 * 加起来相当于 3.1 帧的墨量；此前是 6 帧、衰减很快（0.85 → 0.36 → 0.16…），只相当于 1.5 帧。
 */
export const IMMERSIVE_FIELD_TRAIL_ALPHA: readonly number[] = Array.from(
  { length: 10 },
  (_, age) => Number((0.7 * 0.8 ** age).toFixed(3))
);

/** 沉浸页的点径（CSS 像素）：最小 1.4，随画布放大，封顶 2.6。 */
export function immersiveFieldPointSize(extent: number): number {
  return clamp(extent / 78, 1.4, 2.6);
}

/** 分析页的点径：最小 2.2（原来的固定值），面板放大后随之增大，封顶 3.4。 */
export function pageFieldPointSize(extent: number): number {
  return clamp(extent / 60, 2.2, 3.4);
}

/**
 * 声场画布的像素比：不低于 2 倍（小画布上的斜线、文字和散点要靠超采样保持清晰），
 * 并且是设备像素比的**整数倍**。
 *
 * 此前直接取 `max(设备像素比, 2)`：系统缩放 150% 时画布按 2 倍绘制、再被浏览器缩到
 * 1.5 倍显示，每个 2 像素的点落在 1.5 个设备像素上，被插值得又淡又虚。取整数倍
 * （150% → 3 倍，125% → 2.5 倍）之后缩小是整数比，点的边缘落在设备像素上。
 */
export function fieldPixelRatio(devicePixelRatio: number): number {
  const dpr = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  return dpr * Math.ceil(2 / dpr);
}
