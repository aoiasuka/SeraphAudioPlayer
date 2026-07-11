// 发现2：播放代际计数。每次新的播放/暂停意图递增；
// 慢速异步续体（如 B 站重缓存）完成时若代际已过期则丢弃，避免旧曲目顶掉新选中的曲目。
// 审2-R2：从 playbackActions 抽出为独立模块——outputActions 也需要代际复查，
// 而 playbackActions 已导入 outputActions 的 sendPlayCommand，反向导入会形成环。
let playEpoch = 0;

export function bumpPlayEpoch() {
  return ++playEpoch;
}

export function currentPlayEpoch() {
  return playEpoch;
}
