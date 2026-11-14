/**
 * 一阶指数平滑系数：经过 `dtSec` 之后，显示值应向目标靠近的比例。
 *
 * 用时间常数而不是「每帧固定比例」，结果与显示器刷新率无关：把一段时间拆成
 * 任意多帧，累计效果相同。`dtSec` 封顶 0.25 秒，标签页恢复后的长间隔不会让
 * 数值一步跳到目标。
 */
export function smoothingFactor(dtSec: number, tauSec: number): number {
  if (!(dtSec > 0) || !(tauSec > 0)) return 0;
  return 1 - Math.exp(-Math.min(dtSec, 0.25) / tauSec);
}
