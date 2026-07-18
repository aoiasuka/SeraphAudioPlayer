/* ============================================================
   SeraphViz — 模拟音频特征引擎（仅供设计演示）
   合成一段"有音乐感"的信号特征流：鼓点 / 贝斯 / 旋律 / 镲片，
   同一状态同时驱动 响度 / 电平 / 声场 / 频谱 / 瀑布 五个面板。
   实装时以 seraph-visualizer 的 FFT / 样本流替换本文件即可。
   ============================================================ */
(function () {
  "use strict";

  // 可复现伪随机（演示动画每次打开一致）
  function mulberry32(seed) {
    let a = seed >>> 0;
    return function () {
      a |= 0;
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  const BIN_COUNT = 96; // 1/12 倍频程量级的对数频点
  const F_MIN = 20;
  const F_MAX = 20000;
  const BPM = 96;
  const HISTORY_ROWS = 64; // 瀑布图缓存行数
  const HISTORY_INTERVAL = 0.09; // 每 90ms 存一帧 ≈ 5.8s 时间窗
  const SCATTER_N = 150; // 声场每帧采样点对数

  const BIN_FREQS = new Float32Array(BIN_COUNT);
  const BIN_LOGF = new Float32Array(BIN_COUNT);
  for (let i = 0; i < BIN_COUNT; i++) {
    const f = F_MIN * Math.pow(F_MAX / F_MIN, i / (BIN_COUNT - 1));
    BIN_FREQS[i] = f;
    BIN_LOGF[i] = Math.log10(f);
  }

  function gauss(x, mu, sigma) {
    const d = (x - mu) / sigma;
    return Math.exp(-0.5 * d * d);
  }
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

  function createEngine(opts) {
    const rng = mulberry32((opts && opts.seed) || 20260718);
    const TAU = Math.PI * 2;

    const spectrum = new Float32Array(BIN_COUNT).fill(-90);
    const peakHold = new Float32Array(BIN_COUNT).fill(-90);
    const jitter = new Float32Array(BIN_COUNT);
    const history = [];
    for (let r = 0; r < HISTORY_ROWS; r++) {
      history.push(new Float32Array(BIN_COUNT).fill(-90));
    }

    const state = {
      t: 0,
      bins: { freqs: BIN_FREQS, logf: BIN_LOGF, count: BIN_COUNT, fMin: F_MIN, fMax: F_MAX },
      spectrum,
      peakHold,
      history,
      historyHead: HISTORY_ROWS - 1,
      historyRows: HISTORY_ROWS,
      historyWindowSec: HISTORY_ROWS * HISTORY_INTERVAL,
      historyStamp: 0, // 每 push 一行 +1，供增量渲染
      levels: {
        l: { rms: -60, peak: -60, hold: -60, holdT: 0 },
        r: { rms: -60, peak: -60, hold: -60, holdT: 0 },
        clip: false,
        clipT: -10,
      },
      loud: { m: -23, s: -23, i: -16.2, lra: 5.8, tp: -8, tpMax: -8, target: -14 },
      stereo: { pts: new Float32Array(SCATTER_N * 2), n: SCATTER_N, corr: 0.7, width: 0.4 },
    };

    let lastHist = -1;

    function step(dt) {
      const t = (state.t += dt);

      // ---- 节拍与"乐器"包络 ----
      const beatF = (t * BPM) / 60;
      const bar = Math.floor(beatF / 4);
      const beat = Math.floor(beatF) % 4;
      const bp = beatF % 1;
      const kick = Math.exp(-7 * bp);
      let snare = beat === 1 || beat === 3 ? Math.exp(-10 * bp) : 0;
      if (bar % 4 === 3 && beat === 3) snare = Math.exp(-10 * ((bp * 2) % 1)) * 0.9; // 小节尾 fill
      const hatAcc = Math.floor(beatF * 2) % 2 ? 0.5 : 0.9;
      const hat = Math.exp(-15 * ((beatF * 2) % 1)) * hatAcc;
      const roots = [55, 55, 73.42, 61.74];
      const root = roots[bar % 4];
      const bassAmp = clamp(0.55 + 0.3 * Math.sin(t * 0.9) + 0.15 * kick, 0.1, 1);
      const melF = 740 * Math.pow(2, 0.8 * Math.sin(t * 0.21) + 0.3 * Math.sin(t * 0.047));
      const melAmp = clamp(0.45 + 0.4 * Math.sin(t * 0.31 + 1), 0.05, 1);
      const pad = 0.5 + 0.5 * Math.sin(t * 0.13);

      const lin2db = (x) => 20 * Math.log10(Math.max(x, 1e-5));
      const kickDb = -13 + lin2db(kick);
      const bassDb = -16 + lin2db(bassAmp);
      const snareDb = -18 + lin2db(snare + 1e-4);
      const hatDb = -22 + lin2db(hat + 1e-4);
      const padDb = -30 + lin2db(pad + 1e-3);
      const melDb = -23 + lin2db(melAmp);
      const lRoot = Math.log10(root);
      const lMel = Math.log10(melF);

      // ---- 频谱：底噪 + 各源能量线性叠加 ----
      for (let i = 0; i < BIN_COUNT; i++) {
        const lf = BIN_LOGF[i];
        const base = -39 - 15.5 * (lf - 1.3) + 9 * gauss(lf, 1.78, 0.34);
        let p = Math.pow(10, base / 10);
        p += (gauss(lf, 1.716, 0.14) + 0.25 * gauss(lf, 3.55, 0.4)) * Math.pow(10, kickDb / 10);
        p +=
          (gauss(lf, lRoot, 0.11) +
            0.5 * gauss(lf, lRoot + 0.301, 0.13) +
            0.22 * gauss(lf, lRoot + 0.477, 0.13)) *
          Math.pow(10, bassDb / 10);
        p += gauss(lf, 3.28, 0.42) * Math.pow(10, snareDb / 10);
        p += gauss(lf, 3.93, 0.26) * Math.pow(10, hatDb / 10);
        p += gauss(lf, 2.72, 0.45) * Math.pow(10, padDb / 10);
        p +=
          (gauss(lf, lMel, 0.09) + 0.4 * gauss(lf, lMel + 0.301, 0.1)) *
          Math.pow(10, melDb / 10);

        jitter[i] = jitter[i] * 0.9 + (rng() - 0.5) * 1.7;
        const target = clamp(10 * Math.log10(p) + jitter[i], -90, 0);
        const k = target > spectrum[i] ? Math.min(1, dt * 24) : Math.min(1, dt * 7);
        spectrum[i] += (target - spectrum[i]) * k;
        peakHold[i] = Math.max(spectrum[i], peakHold[i] - dt * 3.5);
      }

      // ---- 电平（dBFS，含峰值保持与削波印章） ----
      const inst =
        -16.5 +
        7 * kick +
        4.5 * snare +
        1.6 * hat +
        2.2 * (bassAmp - 0.55) +
        1.2 * (pad - 0.5);
      const pan = 1.1 * Math.sin(t * 0.53);
      const lv = state.levels;
      let framePeak = -90;
      for (const ch of ["l", "r"]) {
        const s = lv[ch];
        const sgn = ch === "l" ? 1 : -1;
        const chInst = inst + sgn * pan + (rng() - 0.5) * 0.8;
        s.rms += (chInst - s.rms) * Math.min(1, dt * (chInst > s.rms ? 9 : 2.6));
        let pk = chInst + 5.2 + 2.5 * snare + 1.8 * kick;
        if (rng() < dt / 14) pk += 3.2; // 偶发瞬态冲点
        pk = Math.min(pk, -0.2);
        s.peak = pk;
        if (pk > s.hold) {
          s.hold = pk;
          s.holdT = t;
        } else if (t - s.holdT > 1.4) {
          s.hold = Math.max(pk, s.hold - dt * 14);
        }
        framePeak = Math.max(framePeak, pk);
      }
      if (framePeak > -1.2) {
        lv.clip = true;
        lv.clipT = t;
      } else if (t - lv.clipT > 3) {
        lv.clip = false;
      }

      // ---- 响度（EBU R128 语义的简化模拟） ----
      const ld = state.loud;
      const lmInst = inst - 5.2;
      ld.m += (lmInst - ld.m) * Math.min(1, dt * 2.6); // ≈400ms
      ld.s += (ld.m - ld.s) * Math.min(1, dt * 0.45); // ≈3s
      const anchor = -15.8 + 0.8 * Math.sin(t * 0.021);
      ld.i += (anchor + (ld.s - anchor) * 0.25 - ld.i) * Math.min(1, dt * 0.05);
      ld.lra = 5.6 + 1.9 * Math.sin(t * 0.037 + 2) + 0.7 * Math.sin(t * 0.011);
      let tpInst = framePeak + 0.35;
      if (rng() < dt / 9) tpInst += 2.4;
      ld.tp += (tpInst - ld.tp) * Math.min(1, dt * (tpInst > ld.tp ? 12 : 1.2));
      ld.tpMax = Math.max(ld.tp, ld.tpMax - dt * 0.02);

      // ---- 声场散点（goniometer 样本对）与相关度 ----
      const st = state.stereo;
      st.width = clamp(0.42 + 0.27 * Math.sin(t * 0.23) + 0.12 * Math.sin(t * 0.071), 0.05, 0.85);
      let sll = 0,
        srr = 0,
        slr = 0;
      for (let k2 = 0; k2 < SCATTER_N; k2++) {
        const tau = k2 / SCATTER_N;
        const mid =
          0.52 * Math.sin(TAU * (3.1 * tau + t * 1.61)) +
          0.3 * Math.sin(TAU * (7.7 * tau - t * 1.13) + 0.7) +
          0.26 * (rng() * 2 - 1) +
          0.35 * kick * Math.sin(TAU * (1.35 * tau + t * 2.2));
        const side =
          st.width *
          (0.5 * Math.sin(TAU * (5.3 * tau + t * 0.87) + 1.1) + 0.5 * (rng() * 2 - 1));
        const L = clamp((mid + side) * 0.62, -1, 1);
        const R = clamp((mid - side) * 0.62, -1, 1);
        st.pts[k2 * 2] = L;
        st.pts[k2 * 2 + 1] = R;
        sll += L * L;
        srr += R * R;
        slr += L * R;
      }
      const corrInst = slr / Math.sqrt(sll * srr + 1e-9);
      st.corr += (corrInst - st.corr) * Math.min(1, dt * 4);

      // ---- 瀑布历史 ----
      if (t - lastHist >= HISTORY_INTERVAL) {
        lastHist = t;
        state.historyHead = (state.historyHead + 1) % HISTORY_ROWS;
        state.history[state.historyHead].set(spectrum);
        state.historyStamp++;
      }
    }

    // ---- 运行循环与订阅 ----
    const subs = new Set();
    let raf = 0;
    let lastTs = 0;
    let running = false;

    function emit() {
      subs.forEach((fn) => fn(state));
    }
    function frame(ts) {
      if (!running) return;
      const now = ts / 1000;
      const dt = Math.min(0.05, lastTs ? now - lastTs : 0.016);
      lastTs = now;
      step(dt || 0.016);
      emit();
      raf = requestAnimationFrame(frame);
    }

    const engine = {
      state,
      subscribe(fn) {
        subs.add(fn);
        return () => subs.delete(fn);
      },
      renderOnce: emit,
      get running() {
        return running;
      },
      start() {
        if (running) return;
        running = true;
        lastTs = 0;
        raf = requestAnimationFrame(frame);
      },
      stop() {
        running = false;
        cancelAnimationFrame(raf);
      },
      toggle() {
        running ? engine.stop() : engine.start();
        return running;
      },
      /** 预跑几秒让曲线、峰值保持、瀑布历史成型，再按系统动效设置决定是否运行 */
      boot() {
        for (let k = 0; k < 180; k++) step(1 / 30);
        const reduced =
          window.matchMedia &&
          window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        if (reduced) emit();
        else engine.start();
        return !reduced;
      },
    };
    return engine;
  }

  /** RUN/HOLD 按钮 + 减少动态徽标绑定 */
  function bindRunToggle(engine, button, badge) {
    const paint = (isRunning) => {
      button.setAttribute("aria-pressed", String(isRunning));
      button.innerHTML = isRunning
        ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><line x1="9" y1="5" x2="9" y2="19"/><line x1="15" y1="5" x2="15" y2="19"/></svg>运行中 RUN'
        : '<svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true"><polygon points="7,4 20,12 7,20"/></svg>已暂停 HOLD';
    };
    button.addEventListener("click", () => paint(engine.toggle()));
    const startedRunning = engine.boot();
    paint(startedRunning);
    if (!startedRunning && badge) badge.hidden = false;
  }

  window.SeraphViz = { createEngine, bindRunToggle };
})();
