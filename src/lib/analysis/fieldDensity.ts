/**
 * 声场散点的密度随画布放大（沉浸页与分析页共用）。
 *
 * 点云的展开范围随画布线性放大，点数与点径不变的话，墨量密度按面积的倒数下降——
 * 大屏、高分辨率下显得稀稀拉拉。这里两头一起补：点径适度放大，点数按画布分三档向
 * 后端多要（后端按 `fieldPoints` 取真实采样，不是把同一批点复制几遍）。
 * 普通尺寸的窗口落在最低档，画面与之前相同。
 *
 * 下面的 `extent` 都是声场图形的「半径」：菱形的半对角线、或半圆的半径，CSS 像素。
 */
import { clamp } from "./types";

/** 后端每帧给多少对采样点：三档。与 `seraph_visualizer::analysis` 的默认值 / 上限一致。 */
export const FIELD_POINT_TIERS = [480, 960, 1440] as const;
/** 升到下一档的画布半径。 */
const TIER_EXTENTS = [140, 240] as const;
/** 回差：窗口尺寸正好在分界线附近抖动时不来回换档（换档会让后端重开一次订阅）。 */
const TIER_HYSTERESIS = 12;

/**
 * 这么大的画布该向后端要多少对点。`current` 是眼下用的档位，带回差：
 * 升档要超过分界线 12 px，降档要低于分界线 12 px。
 */
export function fieldPointBudget(extent: number, current: number = FIELD_POINT_TIERS[0]): number {
  let tier = Math.max(0, FIELD_POINT_TIERS.indexOf(current as (typeof FIELD_POINT_TIERS)[number]));
  if (!Number.isFinite(extent) || extent <= 0) return FIELD_POINT_TIERS[tier];
  while (tier < TIER_EXTENTS.length && extent >= TIER_EXTENTS[tier] + TIER_HYSTERESIS) tier += 1;
  while (tier > 0 && extent < TIER_EXTENTS[tier - 1] - TIER_HYSTERESIS) tier -= 1;
  return FIELD_POINT_TIERS[tier];
}

/**
 * 沉浸页的点径（CSS 像素）。上限此前是 1.75，画布半径超过 190 px 之后点就不再变大；
 * 放宽到 2.5，再往上靠点数补，免得颗粒发粗。半径 190 px 以内与之前相同。
 */
export function immersiveFieldPointSize(extent: number): number {
  return clamp(extent / 110, 1.1, 2.5);
}

/** 分析页的点径。此前固定 2.2；面板放大（自定义布局、宽屏）后随之增大，封顶 3.4。 */
export function pageFieldPointSize(extent: number): number {
  return clamp(extent / 60, 2.2, 3.4);
}
