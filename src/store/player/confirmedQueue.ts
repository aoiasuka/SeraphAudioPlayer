/**
 * 「乐观更新 + 后端确认」的串行提交队列。
 *
 * 界面先切换（乐观值写进 store），命令排队发给后端；后端拒绝时回到**最后一次确认成功**的值。
 * 两条约束：同一设置的命令必须串行（后发的不能先到）；回滚基线只能是确认过的值，
 * 不能是另一个尚未确认的乐观值。
 */

/** 同一设置串行提交；旧成功仍更新确认值，失败不能把乐观值当作回滚基线。 */
export function createConfirmedQueue<T>() {
  let confirmed: T;
  const queue: Array<() => Promise<void>> = [];
  const drain = async () => {
    while (queue.length > 0) {
      await queue[0]();
      queue.shift();
    }
  };
  return (
    previous: T,
    value: T,
    send: () => Promise<void>,
    onSuccess: () => void | Promise<void>,
    onFailure: (err: unknown, confirmed: T) => void
  ) => {
    const idle = queue.length === 0;
    // 队列空闲时从 store 接续水合/外部刷新；在途期间绝不重新读取乐观状态作基线。
    if (idle) confirmed = previous;
    queue.push(async () => {
      try {
        await send();
      } catch (err) {
        onFailure(err, confirmed);
        return;
      }
      confirmed = value;
      await onSuccess();
    });
    if (idle) void drain();
  };
}

/**
 * 几个字段共用一条写命令时的串行提交（例如歌词的两个显示选项一起发给
 * `set_lyrics_display_options`）。每次只改一个字段，**其余字段带已确认的值**——
 * 不能把另一个字段尚未确认的乐观值夹带出去：那条命令若随后失败并回滚，后端手里
 * 却已经是它的新值了。
 *
 * `previous` 是这次改动写进 store **之前**的整组值。
 */
export function createConfirmedRecordQueue<K extends string, V>() {
  let confirmed: Record<K, V>;
  const queue: Array<() => Promise<void>> = [];
  const drain = async () => {
    while (queue.length > 0) {
      await queue[0]();
      queue.shift();
    }
  };
  return (
    previous: Record<K, V>,
    key: K,
    value: V,
    send: (next: Record<K, V>) => Promise<void>,
    onSuccess: () => void,
    onFailure: (err: unknown, confirmedValue: V) => void
  ) => {
    const idle = queue.length === 0;
    if (idle) confirmed = { ...previous };
    queue.push(async () => {
      const next = { ...confirmed, [key]: value };
      try {
        await send(next);
      } catch (err) {
        onFailure(err, confirmed[key]);
        return;
      }
      confirmed = next;
      onSuccess();
    });
    if (idle) void drain();
  };
}
