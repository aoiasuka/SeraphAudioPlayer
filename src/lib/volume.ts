/**
 * 音量滑块位置（0..1）→ 下发给引擎的线性增益（OUT5-02）。
 *
 * 人耳对响度的感知接近对数：把滑块值直接当线性增益时，50% 只衰减 6 dB，
 * 听感上「没怎么变小」，而最后 10% 的行程里响度急剧下降。这里用立方曲线：
 * 100% → 0 dB、50% → −18 dB、25% → −36 dB、10% → −60 dB，0 仍是静音。
 * store 与持久化里保存的始终是滑块位置，只在发给引擎的那一步换算。
 */
export function volumeToGain(position: number): number {
  if (!Number.isFinite(position) || position <= 0) return 0;
  const clamped = Math.min(1, position);
  return clamped * clamped * clamped;
}
