/* ============================================================
   SeraphPanels — 五个计量面板的渲染器（档案 / 打字机风）
   响度 LOUDNESS · 电平 LEVELS · 声场 SOUND FIELD ·
   频谱 SPECTRUM · 瀑布 SPECTROGRAM
   画法约定（与主程序 EqPage 频响曲线一致）：
   网格 = line 色 0.5px；参考线 = ink3 虚线；主迹线 = stamp 红 2px；
   数字 = Courier 等宽（天然表格数字，避免跳动）。
   ============================================================ */
(function () {
  "use strict";

  // ---------- 公共工具 ----------
  const cssCache = {};
  function cssVar(name) {
    if (!cssCache[name]) {
      cssCache[name] = getComputedStyle(document.documentElement)
        .getPropertyValue(name)
        .trim();
    }
    return cssCache[name];
  }
  function hexA(hex, a) {
    const v = hex.replace("#", "");
    const n = parseInt(v.length === 3 ? v.replace(/./g, "$&$&") : v, 16);
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
  }
  const lerp = (a, b, t) => a + (b - a) * t;
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  const LMIN = Math.log10(20);
  const LMAX = Math.log10(20000);
  const TW = '11px "Courier Prime","Courier New",monospace';
  const TW9 = '9px "Courier Prime","Courier New",monospace';
  const TW10 = 'bold 10px "Courier Prime","Courier New",monospace';

  function el(tag, cls, html) {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (html !== undefined) node.innerHTML = html;
    return node;
  }

  function createCanvas(parent, ariaLabel, hoverable) {
    const wrap = el("div", "viz-canvas-wrap");
    const canvas = document.createElement("canvas");
    canvas.setAttribute("role", "img");
    canvas.setAttribute("aria-label", ariaLabel);
    if (hoverable) canvas.className = "is-hoverable";
    wrap.appendChild(canvas);
    parent.appendChild(wrap);
    const ctx = canvas.getContext("2d");
    function prep() {
      const dpr = window.devicePixelRatio || 1;
      const w = wrap.clientWidth;
      const h = wrap.clientHeight;
      const W = Math.max(1, Math.round(w * dpr));
      const H = Math.max(1, Math.round(h * dpr));
      if (canvas.width !== W || canvas.height !== H) {
        canvas.width = W;
        canvas.height = H;
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      return { w, h };
    }
    return { canvas, ctx, prep, wrap };
  }

  const fmt1 = (v) => (v <= -99.95 ? "-∞" : v.toFixed(1));
  const fmtF = (f) =>
    f >= 1000 ? `${(f / 1000).toFixed(f % 1000 === 0 ? 0 : 1)}k` : `${Math.round(f)}`;
  const freqX = (lf, x0, x1) => x0 + ((lf - LMIN) / (LMAX - LMIN)) * (x1 - x0);

  // ============================================================
  // No.01 LOUDNESS 响度
  // ============================================================
  function mountLoudness(body, meta, engine) {
    body.innerHTML = `
      <div class="viz-loud__grid">
        <div class="viz-loud__cell"><span class="viz-loud__lbl">SHORT TERM 短期</span>
          <span class="viz-num viz-loud__val" data-s>--.-</span><span class="viz-loud__unit">LUFS · 3S</span></div>
        <div class="viz-loud__cell viz-loud__cell--main"><span class="viz-loud__lbl">INTEGRATED 整体</span>
          <span class="viz-num viz-loud__val" data-i>--.-</span><span class="viz-loud__unit">LUFS</span></div>
        <div class="viz-loud__cell"><span class="viz-loud__lbl">MOMENTARY 瞬时</span>
          <span class="viz-num viz-loud__val" data-m>--.-</span><span class="viz-loud__unit">LUFS · 400MS</span></div>
      </div>
      <div class="viz-loud__sub">
        <span>LRA <span class="viz-num" data-lra>-.-</span> LU</span>
        <span>TRUE PEAK MAX <span class="viz-num" data-tp>--.-</span> dBTP</span>
        <span class="viz-flag" data-over>OVER</span>
      </div>
      <div class="viz-loud__target">
        <label>TARGET 目标
          <select class="viz-select" data-target aria-label="响度目标">
            <option value="-14">-14 · 流媒体</option>
            <option value="-16">-16 · 播客</option>
            <option value="-23">-23 · EBU 广播</option>
            <option value="-9">-9 · 母带参考</option>
          </select>
        </label>
        <div class="viz-loud__bar" data-bar></div>
      </div>`;
    const q = (s) => body.querySelector(s);
    const nodes = {
      m: q("[data-m]"),
      s: q("[data-s]"),
      i: q("[data-i]"),
      lra: q("[data-lra]"),
      tp: q("[data-tp]"),
      over: q("[data-over]"),
    };
    const select = q("[data-target]");
    const bar = createCanvas(q("[data-bar]"), "整体响度相对目标的偏差标尺", false);

    select.addEventListener("change", () => {
      engine.state.loud.target = Number(select.value);
      if (!engine.running) engine.renderOnce();
    });

    function drawBar(ld) {
      const { w, h } = bar.prep();
      if (w < 30 || h < 12) return;
      const ctx = bar.ctx;
      const ink = cssVar("--ink"),
        ink3 = cssVar("--ink3"),
        line = cssVar("--line"),
        stamp = cssVar("--stamp"),
        brown = cssVar("--brown"),
        gold = cssVar("--gold-dark");
      ctx.clearRect(0, 0, w, h);
      const x0 = 6,
        x1 = w - 6,
        mid = h / 2 - 4;
      const devX = (lu) => lerp(x0, x1, (clamp(lu, -9, 9) + 9) / 18);
      // 刻度
      ctx.font = TW9;
      ctx.textAlign = "center";
      ctx.fillStyle = ink3;
      ctx.strokeStyle = line;
      ctx.lineWidth = 1;
      for (const lu of [-9, -6, -3, 0, 3, 6, 9]) {
        const x = devX(lu);
        ctx.beginPath();
        ctx.moveTo(x, mid - 8);
        ctx.lineTo(x, mid + 8);
        ctx.stroke();
        ctx.fillText(lu > 0 ? `+${lu}` : `${lu}`, x, h - 2);
      }
      ctx.strokeStyle = hexA(cssVar("--ink3"), 0.7);
      ctx.beginPath();
      ctx.moveTo(x0, mid);
      ctx.lineTo(x1, mid);
      ctx.stroke();
      // 目标线（0 偏差）
      ctx.strokeStyle = gold;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(devX(0), mid - 11);
      ctx.lineTo(devX(0), mid + 11);
      ctx.stroke();
      // M 实时细条
      const devM = ld.m - ld.target;
      ctx.fillStyle = hexA(cssVar("--ink"), 0.28);
      ctx.fillRect(Math.min(devX(0), devX(devM)), mid + 4, Math.abs(devX(devM) - devX(0)), 3);
      // I 主偏差条
      const devI = ld.i - ld.target;
      ctx.fillStyle = devI > 1 ? stamp : devI < -1 ? hexA(ink, 0.55) : brown;
      ctx.fillRect(Math.min(devX(0), devX(devI)), mid - 6, Math.abs(devX(devI) - devX(0)), 6);
    }

    engine.subscribe((st) => {
      const ld = st.loud;
      nodes.m.textContent = fmt1(ld.m);
      nodes.s.textContent = fmt1(ld.s);
      nodes.i.textContent = fmt1(ld.i);
      nodes.lra.textContent = ld.lra.toFixed(1);
      nodes.tp.textContent = fmt1(ld.tpMax);
      const over = ld.tpMax > -1;
      nodes.over.classList.toggle("is-on", over);
      nodes.tp.style.color = over ? cssVar("--stamp") : "";
      if (meta) {
        const dev = ld.i - ld.target;
        meta.textContent = `Δ TARGET ${dev >= 0 ? "+" : ""}${dev.toFixed(1)} LU`;
      }
      drawBar(ld);
    });
  }

  // ============================================================
  // No.02 LEVELS 电平表
  // ============================================================
  function mountLevels(body, meta, engine) {
    const readout = el(
      "div",
      "viz-levels__readout",
      `<span>PEAK <span class="viz-num" data-pl>--.-</span> / <span class="viz-num" data-pr>--.-</span></span>
       <span>RMS <span class="viz-num" data-rl>--.-</span> / <span class="viz-num" data-rr>--.-</span></span>
       <span class="viz-flag" data-clip>CLIP</span>`
    );
    body.appendChild(readout);
    const cv = createCanvas(body, "左右声道峰值与均方根电平条，红区为 -6 dBFS 以上", false);
    const q = (s) => readout.querySelector(s);
    const nPl = q("[data-pl]"),
      nPr = q("[data-pr]"),
      nRl = q("[data-rl]"),
      nRr = q("[data-rr]"),
      nClip = q("[data-clip]");

    const SCALE = [0, -3, -6, -9, -12, -18, -24, -30, -40, -50, -60];
    const LABELED = new Set([0, -6, -12, -24, -40, -60]);

    engine.subscribe((st) => {
      const lv = st.levels;
      nPl.textContent = fmt1(lv.l.hold);
      nPr.textContent = fmt1(lv.r.hold);
      nRl.textContent = fmt1(lv.l.rms);
      nRr.textContent = fmt1(lv.r.rms);
      nPl.classList.toggle("is-hot", lv.l.hold > -6);
      nPr.classList.toggle("is-hot", lv.r.hold > -6);
      nClip.classList.toggle("is-on", lv.clip);

      const { w, h } = cv.prep();
      if (w < 60 || h < 60) return;
      const ctx = cv.ctx;
      const ink = cssVar("--ink"),
        ink2 = cssVar("--ink2"),
        ink3 = cssVar("--ink3"),
        line = cssVar("--line"),
        stamp = cssVar("--stamp"),
        brown = cssVar("--brown"),
        paper2 = cssVar("--paper2");
      ctx.clearRect(0, 0, w, h);

      const top = 8,
        bottom = h - 18,
        axisW = 34;
      const dbY = (db) => lerp(top, bottom, clamp(-db, 0, 60) / 60);
      const meterX0 = axisW + 6;
      const barW = clamp((w - meterX0 - 10) / 4.6, 22, 52);
      const gap = barW * 0.7;
      const cx = (w + meterX0) / 2;
      const bars = [
        { x: cx - gap / 2 - barW, ch: st.levels.l, tag: "L" },
        { x: cx + gap / 2, ch: st.levels.r, tag: "R" },
      ];

      // 刻度网格与轴标签
      ctx.font = TW9;
      ctx.textAlign = "right";
      ctx.textBaseline = "middle";
      for (const db of SCALE) {
        const y = dbY(db);
        ctx.strokeStyle = line;
        ctx.lineWidth = LABELED.has(db) ? 1 : 0.5;
        ctx.beginPath();
        ctx.moveTo(meterX0 - 3, y);
        ctx.lineTo(w - 6, y);
        ctx.stroke();
        if (LABELED.has(db)) {
          ctx.fillStyle = ink2;
          ctx.fillText(String(db), axisW, y);
        }
      }

      for (const bar of bars) {
        const { x, ch, tag } = bar;
        // 槽底
        ctx.fillStyle = paper2;
        ctx.fillRect(x, top, barW, bottom - top);
        // 红区（0 ~ -6）阴影线
        const redY = dbY(-6);
        ctx.fillStyle = cssVar("--stamp-soft");
        ctx.fillRect(x, top, barW, redY - top);
        ctx.save();
        ctx.beginPath();
        ctx.rect(x, top, barW, redY - top);
        ctx.clip();
        ctx.strokeStyle = hexA(cssVar("--stamp"), 0.35);
        ctx.lineWidth = 1;
        for (let sx = x - (bottom - top); sx < x + barW; sx += 5) {
          ctx.beginPath();
          ctx.moveTo(sx, redY + 2);
          ctx.lineTo(sx + (redY - top) + 4, top - 2);
          ctx.stroke();
        }
        ctx.restore();
        // 峰值（浅棕）与 RMS（实墨）
        ctx.fillStyle = hexA(cssVar("--brown"), 0.34);
        ctx.fillRect(x, dbY(ch.peak), barW, bottom - dbY(ch.peak));
        ctx.fillStyle = hexA(cssVar("--ink"), 0.85);
        ctx.fillRect(x, dbY(ch.rms), barW, bottom - dbY(ch.rms));
        // 峰值保持线
        ctx.strokeStyle = ch.hold > -6 ? stamp : brown;
        ctx.lineWidth = 2.5;
        ctx.beginPath();
        ctx.moveTo(x - 2, dbY(ch.hold));
        ctx.lineTo(x + barW + 2, dbY(ch.hold));
        ctx.stroke();
        // 外框 + 声道标签
        ctx.strokeStyle = ink;
        ctx.lineWidth = 1.5;
        ctx.strokeRect(x, top, barW, bottom - top);
        ctx.font = TW10;
        ctx.textAlign = "center";
        ctx.fillStyle = ink2;
        ctx.fillText(tag, x + barW / 2, h - 6);
        ctx.font = TW9;
        ctx.textBaseline = "middle";
        ctx.textAlign = "right";
      }
      ctx.textBaseline = "alphabetic";
      if (meta) meta.textContent = "dBFS · PEAK+RMS";
    });
  }

  // ============================================================
  // No.03 SOUND FIELD 声场
  // ============================================================
  function mountSoundField(body, meta, engine) {
    const cv = createCanvas(body, "立体声声场极坐标散点与相关度表", false);
    const toolbar = el("div", "viz-panel__toolbar");
    const tabs = el("div", "viz-tabs");
    const hint = el("span", "viz-panel__hint", "ρ&gt;0 同相 · ρ&lt;0 反相（印章红警示）");
    toolbar.appendChild(tabs);
    toolbar.appendChild(hint);
    body.appendChild(toolbar);

    let mode = "polar";
    const MODES = [
      ["polar", "POLAR 极坐标"],
      ["lissajous", "LISSAJOUS 李萨如"],
    ];
    for (const [key, label] of MODES) {
      const b = el("button", "viz-tab", label);
      b.type = "button";
      b.setAttribute("aria-pressed", String(key === mode));
      b.addEventListener("click", () => {
        mode = key;
        tabs.querySelectorAll(".viz-tab").forEach((n) => n.setAttribute("aria-pressed", "false"));
        b.setAttribute("aria-pressed", "true");
        if (!engine.running) engine.renderOnce();
      });
      tabs.appendChild(b);
    }

    // 荧光余晖：保留最近 6 帧散点
    const trail = [];

    engine.subscribe((st) => {
      if (engine.running || trail.length === 0) {
        trail.push(Float32Array.from(st.stereo.pts));
        if (trail.length > 6) trail.shift();
      }
      const { w, h } = cv.prep();
      if (w < 60 || h < 80) return;
      const ctx = cv.ctx;
      const ink = cssVar("--ink"),
        ink2 = cssVar("--ink2"),
        ink3 = cssVar("--ink3"),
        line = cssVar("--line"),
        stamp = cssVar("--stamp");
      ctx.clearRect(0, 0, w, h);

      const corrH = 34;
      const mainH = h - corrH;

      if (mode === "polar") {
        const cx = w / 2;
        const cy = mainH - 10;
        const R = Math.min(w * 0.42, mainH - 26);
        // 外半圆 + 半径参考弧
        ctx.strokeStyle = ink;
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.arc(cx, cy, R, Math.PI, 0);
        ctx.stroke();
        ctx.strokeStyle = line;
        ctx.setLineDash([4, 3]);
        ctx.beginPath();
        ctx.arc(cx, cy, R / 2, Math.PI, 0);
        ctx.stroke();
        ctx.setLineDash([]);
        // 角度辐条
        for (let deg = -90; deg <= 90; deg += 30) {
          const a = (deg * Math.PI) / 180;
          const strong = deg === -45 || deg === 45;
          ctx.strokeStyle = strong ? hexA(ink, 0.75) : hexA(cssVar("--ink3"), 0.5);
          ctx.lineWidth = strong ? 1 : 0.5;
          ctx.beginPath();
          ctx.moveTo(cx + Math.sin(a) * R * 0.12, cy - Math.cos(a) * R * 0.12);
          ctx.lineTo(cx + Math.sin(a) * R, cy - Math.cos(a) * R);
          ctx.stroke();
        }
        // 底线
        ctx.strokeStyle = ink;
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.moveTo(cx - R, cy);
        ctx.lineTo(cx + R, cy);
        ctx.stroke();
        // 标签
        ctx.font = TW10;
        ctx.fillStyle = ink2;
        ctx.textAlign = "center";
        const aL = (-45 * Math.PI) / 180;
        const aR = (45 * Math.PI) / 180;
        ctx.fillText("L", cx + Math.sin(aL) * (R + 11), cy - Math.cos(aL) * (R + 11) + 3);
        ctx.fillText("R", cx + Math.sin(aR) * (R + 11), cy - Math.cos(aR) * (R + 11) + 3);
        ctx.font = TW9;
        ctx.fillStyle = ink3;
        ctx.fillText("MONO", cx, cy - R - 8);
        // 散点（余晖由旧到新）
        for (let g = 0; g < trail.length; g++) {
          const pts = trail[g];
          const age = trail.length - 1 - g;
          const alpha = [0.8, 0.5, 0.34, 0.22, 0.14, 0.08][age] || 0.06;
          ctx.fillStyle = hexA(ink, alpha);
          for (let k = 0; k < pts.length; k += 2) {
            const L = pts[k],
              Rv = pts[k + 1];
            const gx = (Rv - L) / Math.SQRT2;
            const gy = (L + Rv) / Math.SQRT2;
            const r = Math.min(1, Math.hypot(gx, gy) / 1.35);
            const phi = Math.atan2(gx, Math.abs(gy));
            const px = cx + Math.sin(phi) * r * R;
            const py = cy - Math.cos(phi) * r * R;
            if (r > 0.82 && age === 0) ctx.fillStyle = hexA(stamp, 0.8);
            ctx.fillRect(px - 1.1, py - 1.1, 2.2, 2.2);
            if (r > 0.82 && age === 0) ctx.fillStyle = hexA(ink, alpha);
          }
        }
      } else {
        // 李萨如：菱形框（±45° 的 L/R 轴）
        const cx = w / 2;
        const cyc = mainH / 2 + 2;
        const S = Math.min(w, mainH) / 2 - 18;
        ctx.strokeStyle = ink;
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        ctx.moveTo(cx, cyc - S);
        ctx.lineTo(cx + S, cyc);
        ctx.lineTo(cx, cyc + S);
        ctx.lineTo(cx - S, cyc);
        ctx.closePath();
        ctx.stroke();
        ctx.strokeStyle = hexA(cssVar("--ink3"), 0.5);
        ctx.lineWidth = 0.5;
        ctx.beginPath();
        ctx.moveTo(cx - S, cyc);
        ctx.lineTo(cx + S, cyc);
        ctx.moveTo(cx, cyc - S);
        ctx.lineTo(cx, cyc + S);
        ctx.stroke();
        ctx.font = TW10;
        ctx.fillStyle = ink2;
        ctx.textAlign = "center";
        ctx.fillText("+L", cx - S / 2 - 12, cyc - S / 2 - 6);
        ctx.fillText("+R", cx + S / 2 + 12, cyc - S / 2 - 6);
        for (let g = 0; g < trail.length; g++) {
          const pts = trail[g];
          const age = trail.length - 1 - g;
          const alpha = [0.8, 0.5, 0.34, 0.22, 0.14, 0.08][age] || 0.06;
          ctx.fillStyle = hexA(ink, alpha);
          for (let k = 0; k < pts.length; k += 2) {
            const L = pts[k],
              Rv = pts[k + 1];
            const gx = ((Rv - L) / Math.SQRT2 / 1.35) * S;
            const gy = ((L + Rv) / Math.SQRT2 / 1.35) * S;
            ctx.fillRect(cx + gx - 1.1, cyc - gy - 1.1, 2.2, 2.2);
          }
        }
      }

      // 相关度表（底部横条）
      const corr = st.stereo.corr;
      const y0 = h - corrH + 8;
      const bx0 = 86,
        bx1 = w - 56;
      ctx.font = TW9;
      ctx.textAlign = "left";
      ctx.fillStyle = ink3;
      ctx.fillText("CORRELATION 相关", 2, y0 + 8);
      ctx.strokeStyle = line;
      ctx.lineWidth = 1;
      ctx.strokeRect(bx0, y0, bx1 - bx0, 10);
      const midX = (bx0 + bx1) / 2;
      for (const tv of [-1, -0.5, 0, 0.5, 1]) {
        const x = lerp(bx0, bx1, (tv + 1) / 2);
        ctx.beginPath();
        ctx.moveTo(x, y0 + 10);
        ctx.lineTo(x, y0 + 14);
        ctx.strokeStyle = ink3;
        ctx.stroke();
      }
      const cxr = lerp(bx0, bx1, (clamp(corr, -1, 1) + 1) / 2);
      ctx.fillStyle = corr >= 0 ? hexA(ink, 0.78) : hexA(stamp, 0.9);
      ctx.fillRect(Math.min(midX, cxr), y0 + 2, Math.abs(cxr - midX), 6);
      ctx.strokeStyle = ink;
      ctx.beginPath();
      ctx.moveTo(midX, y0 - 2);
      ctx.lineTo(midX, y0 + 12);
      ctx.stroke();
      ctx.font = TW;
      ctx.textAlign = "right";
      ctx.fillStyle = corr < 0 ? stamp : ink;
      ctx.fillText(`${corr >= 0 ? "+" : ""}${corr.toFixed(2)}`, w - 8, y0 + 9);

      if (meta)
        meta.textContent = `W ${st.stereo.width.toFixed(2)} · ρ ${corr >= 0 ? "+" : ""}${corr.toFixed(2)}`;
    });
  }

  // ============================================================
  // No.04 SPECTRUM 频谱
  // ============================================================
  function mountSpectrum(body, meta, engine, detail) {
    const cv = createCanvas(body, "实时频谱曲线，对数频轴 20Hz 至 20kHz，含峰值保持虚线", true);
    let cursorBin = null;
    let lastState = null;

    const MINOR = [30, 40, 60, 80, 150, 300, 400, 600, 800, 1500, 3000, 4000, 6000, 8000, 15000];
    const MAJOR = [100, 1000, 10000];
    const LABELS = [50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];

    function draw(st) {
      lastState = st;
      const { w, h } = cv.prep();
      if (w < 60 || h < 60) return;
      const ctx = cv.ctx;
      const ink = cssVar("--ink"),
        ink2 = cssVar("--ink2"),
        ink3 = cssVar("--ink3"),
        line = cssVar("--line"),
        stamp = cssVar("--stamp");
      ctx.clearRect(0, 0, w, h);
      const x0 = 6,
        x1 = w - 30,
        y0 = 6,
        y1 = h - 17;
      const dbTop = 0,
        dbBot = -90;
      const dbY = (db) => lerp(y0, y1, (dbTop - clamp(db, dbBot, dbTop)) / (dbTop - dbBot));

      // 网格：频率
      for (const f of MINOR) {
        const x = freqX(Math.log10(f), x0, x1);
        ctx.strokeStyle = hexA(cssVar("--line"), 0.55);
        ctx.lineWidth = 0.5;
        ctx.beginPath();
        ctx.moveTo(x, y0);
        ctx.lineTo(x, y1);
        ctx.stroke();
      }
      for (const f of MAJOR) {
        const x = freqX(Math.log10(f), x0, x1);
        ctx.strokeStyle = hexA(cssVar("--ink3"), 0.55);
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x, y0);
        ctx.lineTo(x, y1);
        ctx.stroke();
      }
      // 网格：dB
      ctx.font = TW9;
      ctx.textAlign = "left";
      for (let db = 0; db >= -90; db -= 10) {
        const y = dbY(db);
        ctx.strokeStyle = db === 0 ? hexA(cssVar("--ink3"), 0.7) : hexA(cssVar("--line"), 0.7);
        ctx.lineWidth = 0.5;
        ctx.beginPath();
        ctx.moveTo(x0, y);
        ctx.lineTo(x1, y);
        ctx.stroke();
        if (db % 20 === 0) {
          ctx.fillStyle = ink3;
          ctx.fillText(String(db), x1 + 4, y + 3);
        }
      }
      // 频率标签
      ctx.textAlign = "center";
      ctx.fillStyle = ink2;
      const labels = w < 520 ? MAJOR : LABELS;
      for (const f of labels) {
        ctx.fillText(fmtF(f), freqX(Math.log10(f), x0, x1), h - 5);
      }

      const bins = st.bins;
      const px = (i) => freqX(bins.logf[i], x0, x1);

      // 峰值保持（墨灰虚线）
      ctx.strokeStyle = hexA(cssVar("--ink2"), 0.75);
      ctx.lineWidth = 1;
      ctx.setLineDash([3, 3]);
      ctx.beginPath();
      for (let i = 0; i < bins.count; i++) {
        const y = dbY(st.peakHold[i]);
        i === 0 ? ctx.moveTo(px(i), y) : ctx.lineTo(px(i), y);
      }
      ctx.stroke();
      ctx.setLineDash([]);

      // 主频谱：印章红迹线 + 淡填充（与 EQ 页曲线同语言）
      ctx.beginPath();
      ctx.moveTo(x0, y1);
      for (let i = 0; i < bins.count; i++) ctx.lineTo(px(i), dbY(st.spectrum[i]));
      ctx.lineTo(x1, y1);
      ctx.closePath();
      ctx.fillStyle = hexA(cssVar("--stamp"), 0.08);
      ctx.fill();
      ctx.strokeStyle = stamp;
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (let i = 0; i < bins.count; i++) {
        const y = dbY(st.spectrum[i]);
        i === 0 ? ctx.moveTo(px(i), y) : ctx.lineTo(px(i), y);
      }
      ctx.stroke();

      // 游标
      if (cursorBin !== null) {
        const i = cursorBin;
        const cx = px(i);
        const cy = dbY(st.spectrum[i]);
        ctx.strokeStyle = hexA(cssVar("--ink"), 0.8);
        ctx.lineWidth = 1;
        ctx.setLineDash([2, 3]);
        ctx.beginPath();
        ctx.moveTo(cx, y0);
        ctx.lineTo(cx, y1);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.fillStyle = stamp;
        ctx.beginPath();
        ctx.arc(cx, cy, 3.2, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = cssVar("--card");
        ctx.lineWidth = 1.2;
        ctx.stroke();
        if (meta)
          meta.textContent = `${fmtF(bins.freqs[i])} Hz · ${st.spectrum[i].toFixed(1)} dB`;
      } else if (meta) {
        meta.textContent = `${bins.count} BANDS · LOG 20–20k`;
      }
      cv.lastGeom = { x0, x1 };
    }

    // 悬停 / 键盘游标
    cv.canvas.addEventListener("mousemove", (ev) => {
      if (!lastState || !cv.lastGeom) return;
      const rect = cv.canvas.getBoundingClientRect();
      const x = ev.clientX - rect.left;
      const { x0, x1 } = cv.lastGeom;
      const lf = LMIN + (clamp((x - x0) / (x1 - x0), 0, 1)) * (LMAX - LMIN);
      cursorBin = Math.round(
        ((lf - LMIN) / (LMAX - LMIN)) * (lastState.bins.count - 1)
      );
      if (!engine.running) draw(lastState);
    });
    cv.canvas.addEventListener("mouseleave", () => {
      if (document.activeElement === cv.canvas) return;
      cursorBin = null;
      if (!engine.running && lastState) draw(lastState);
    });
    if (detail) {
      cv.canvas.tabIndex = 0;
      cv.canvas.setAttribute(
        "aria-label",
        "实时频谱曲线。获得焦点后用左右方向键移动读数游标。"
      );
      cv.canvas.addEventListener("keydown", (ev) => {
        if (!lastState) return;
        if (ev.key === "ArrowLeft" || ev.key === "ArrowRight") {
          ev.preventDefault();
          const dir = ev.key === "ArrowRight" ? 1 : -1;
          cursorBin = clamp((cursorBin ?? 48) + dir, 0, lastState.bins.count - 1);
          if (!engine.running) draw(lastState);
        } else if (ev.key === "Escape") {
          cursorBin = null;
          if (!engine.running) draw(lastState);
        }
      });
      cv.canvas.addEventListener("blur", () => {
        cursorBin = null;
        if (!engine.running && lastState) draw(lastState);
      });
    }

    engine.subscribe(draw);
  }

  // ============================================================
  // No.05 SPECTROGRAM 瀑布图
  // ============================================================
  function mountSpectrogram(body, meta, engine) {
    const cv = createCanvas(body, "频谱瀑布图，山脊模式为逐帧频谱线堆叠，热图模式为时间-频率能量色图", false);
    const toolbar = el("div", "viz-panel__toolbar");
    const tabs = el("div", "viz-tabs");
    const hint = el("span", "viz-panel__hint", "");
    toolbar.appendChild(tabs);
    toolbar.appendChild(hint);
    body.appendChild(toolbar);

    let mode = "ridge";
    const MODES = [
      ["ridge", "RIDGE 山脊"],
      ["heat", "HEAT 热图"],
    ];
    for (const [key, label] of MODES) {
      const b = el("button", "viz-tab", label);
      b.type = "button";
      b.setAttribute("aria-pressed", String(key === mode));
      b.addEventListener("click", () => {
        mode = key;
        tabs.querySelectorAll(".viz-tab").forEach((n) => n.setAttribute("aria-pressed", "false"));
        b.setAttribute("aria-pressed", "true");
        if (!engine.running) engine.renderOnce();
      });
      tabs.appendChild(b);
    }

    // 热图色带 LUT：纸 → 浅褐 → 棕 → 印章红 → 墨（墨渍渗纸的档案色阶）
    const STOPS = [
      [0, 0xfb, 0xf9, 0xf3],
      [0.3, 0xec, 0xdf, 0xc4],
      [0.52, 0xcb, 0xb4, 0x89],
      [0.68, 0x9a, 0x7a, 0x52],
      [0.8, 0x7a, 0x5c, 0x3e],
      [0.9, 0xb5, 0x48, 0x2a],
      [1, 0x2b, 0x27, 0x22],
    ];
    const LUT = new Uint8ClampedArray(256 * 3);
    for (let v = 0; v < 256; v++) {
      const tv = v / 255;
      let s0 = STOPS[0],
        s1 = STOPS[STOPS.length - 1];
      for (let k = 0; k < STOPS.length - 1; k++) {
        if (tv >= STOPS[k][0] && tv <= STOPS[k + 1][0]) {
          s0 = STOPS[k];
          s1 = STOPS[k + 1];
          break;
        }
      }
      const f = (tv - s0[0]) / Math.max(1e-6, s1[0] - s0[0]);
      LUT[v * 3] = lerp(s0[1], s1[1], f);
      LUT[v * 3 + 1] = lerp(s0[2], s1[2], f);
      LUT[v * 3 + 2] = lerp(s0[3], s1[3], f);
    }
    const off = document.createElement("canvas");
    const offCtx = off.getContext("2d");

    const norm = (db) => clamp((db + 78) / 78, 0, 1);

    engine.subscribe((st) => {
      const { w, h } = cv.prep();
      if (w < 60 || h < 60) return;
      const ctx = cv.ctx;
      const ink = cssVar("--ink"),
        ink2 = cssVar("--ink2"),
        ink3 = cssVar("--ink3"),
        line = cssVar("--line"),
        stamp = cssVar("--stamp"),
        card = cssVar("--card");
      ctx.clearRect(0, 0, w, h);
      const rows = st.historyRows;
      const bins = st.bins;

      if (mode === "ridge") {
        const topY = 16,
          botY = h - 24;
        for (let k = 0; k < rows; k++) {
          const row = st.history[(st.historyHead + 1 + k) % rows];
          const d = k / (rows - 1); // 0 最远（最旧）→ 1 最近（最新）
          const xL = lerp(w * 0.16, w * 0.05, d);
          const xR = lerp(w * 0.84, w * 0.95, d);
          const base = lerp(topY, botY, d);
          const amp = lerp(0.11, 0.24, d) * h;
          // 遮挡填充
          ctx.beginPath();
          ctx.moveTo(xL, base);
          for (let i = 0; i < bins.count; i++) {
            const v = Math.pow(norm(row[i]), 1.3);
            ctx.lineTo(lerp(xL, xR, i / (bins.count - 1)), base - v * amp);
          }
          ctx.lineTo(xR, base);
          ctx.closePath();
          ctx.fillStyle = card;
          ctx.fill();
          // 墨线
          ctx.strokeStyle = hexA(cssVar("--ink"), lerp(0.16, 0.85, d));
          ctx.lineWidth = lerp(0.6, 1.3, d);
          ctx.beginPath();
          for (let i = 0; i < bins.count; i++) {
            const v = Math.pow(norm(row[i]), 1.3);
            const x = lerp(xL, xR, i / (bins.count - 1));
            const y = base - v * amp;
            i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
          }
          ctx.stroke();
          // 高能量段描印章红
          ctx.strokeStyle = hexA(cssVar("--stamp"), lerp(0.14, 0.8, d));
          ctx.lineWidth = lerp(0.7, 1.5, d);
          let seg = false;
          ctx.beginPath();
          for (let i = 0; i < bins.count; i++) {
            const v = Math.pow(norm(row[i]), 1.3);
            const x = lerp(xL, xR, i / (bins.count - 1));
            const y = base - v * amp;
            if (v > 0.52) {
              seg ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
              seg = true;
            } else seg = false;
          }
          ctx.stroke();
        }
        // 最新行下方的频率标注
        ctx.font = TW9;
        ctx.textAlign = "center";
        ctx.fillStyle = ink2;
        for (const f of [100, 1000, 10000]) {
          const x = freqX(Math.log10(f), w * 0.05, w * 0.95);
          ctx.fillText(fmtF(f), x, h - 8);
          ctx.strokeStyle = line;
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(x, botY);
          ctx.lineTo(x, botY + 4);
          ctx.stroke();
        }
        ctx.textAlign = "left";
        ctx.fillStyle = ink3;
        ctx.fillText("PAST ↑", 6, topY + 2);
        hint.innerHTML = `窗口 ≈${st.historyWindowSec.toFixed(1)}S · ${rows} 帧 × ${bins.count} 频点`;
      } else {
        // HEAT 热图
        const x0 = 8,
          x1 = w - 44,
          y0 = 8,
          y1 = h - 22;
        if (off.width !== bins.count || off.height !== rows) {
          off.width = bins.count;
          off.height = rows;
        }
        const img = offCtx.createImageData(bins.count, rows);
        for (let k = 0; k < rows; k++) {
          const row = st.history[(st.historyHead + 1 + k) % rows];
          for (let i = 0; i < bins.count; i++) {
            // 热图专用地板 -66dB + gamma 1.25：底噪回归纸色，能量区拉开层次
            const v = Math.round(
              Math.pow(clamp((row[i] + 66) / 66, 0, 1), 1.25) * 255
            );
            const o = (k * bins.count + i) * 4;
            img.data[o] = LUT[v * 3];
            img.data[o + 1] = LUT[v * 3 + 1];
            img.data[o + 2] = LUT[v * 3 + 2];
            img.data[o + 3] = 255;
          }
        }
        offCtx.putImageData(img, 0, 0);
        ctx.imageSmoothingEnabled = true; // 晕染过渡，似墨色渗纸
        ctx.drawImage(off, x0, y0, x1 - x0, y1 - y0);
        ctx.strokeStyle = ink;
        ctx.lineWidth = 1.5;
        ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
        // 轴：频率（下）与时间（右）
        ctx.font = TW9;
        ctx.textAlign = "center";
        ctx.fillStyle = ink2;
        for (const f of [100, 1000, 10000]) {
          const x = freqX(Math.log10(f), x0, x1);
          ctx.fillText(fmtF(f), x, h - 8);
          ctx.strokeStyle = hexA(cssVar("--ink"), 0.5);
          ctx.beginPath();
          ctx.moveTo(x, y1);
          ctx.lineTo(x, y1 + 4);
          ctx.stroke();
        }
        ctx.textAlign = "left";
        ctx.fillStyle = ink3;
        ctx.fillText("NOW", x1 + 6, y1 - 2);
        ctx.fillText(`-${st.historyWindowSec.toFixed(0)}S`, x1 + 6, y0 + 9);
        hint.innerHTML = "色带：纸 → 浅褐 → 棕 → 印章红 → 墨";
      }
      if (meta) meta.textContent = mode === "ridge" ? "RIDGE · INK TRACE" : "HEAT · PAPER-INK MAP";
    });
  }

  // ============================================================
  // 自动挂载
  // ============================================================
  const MOUNTS = {
    loudness: mountLoudness,
    levels: mountLevels,
    soundfield: mountSoundField,
    spectrum: mountSpectrum,
    spectrogram: mountSpectrogram,
  };

  function mountAll(engine) {
    document.querySelectorAll("[data-panel]").forEach((section) => {
      const kind = section.dataset.panel;
      const body = section.querySelector(".viz-panel__body");
      const meta = section.querySelector("[data-meta]");
      if (MOUNTS[kind] && body) {
        MOUNTS[kind](body, meta, engine, section.hasAttribute("data-detail"));
      }
    });
    // HOLD（含系统减少动态）状态下：布局就绪后补画一帧，窗口尺寸变化时保持画面清晰
    const repaintIfHeld = () => {
      if (!engine.running) engine.renderOnce();
    };
    requestAnimationFrame(repaintIfHeld);
    window.addEventListener("resize", repaintIfHeld);
  }

  window.SeraphPanels = { mountAll };
})();
