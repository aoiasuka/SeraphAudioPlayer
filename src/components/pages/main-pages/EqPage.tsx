import {
  Download,
  Plus,
  RotateCcw,
  Save,
  Trash2,
  Upload,
} from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { DraftNumberInput } from "@/components/ui/DraftNumberInput";
import { parseApoPreset, toApoText } from "@/lib/eqApoParser";
import { GENRE_EQ_PRESETS, clipRisk } from "@/lib/eqPresets";
import { combinedResponseDb, logFreqPoints } from "@/lib/eqResponse";
import { sanitizeEqBands, sanitizeEqPreamp } from "@/lib/eqSanitize";
import { invoke, normalizeIpcError } from "@/lib/tauri";
import { useEqStore } from "@/store/eq";
import { usePlayerStore } from "@/store/player";
import type { EqBand, EqBandKind } from "@/types/dsp";

const BAND_KIND_LABELS: Record<EqBandKind, string> = {
  peaking: "峰化 PK",
  lowshelf: "低架 LSC",
  highshelf: "高架 HSC",
  lowpass: "低通 LP",
  highpass: "高通 HP",
};

function formatFreq(freq: number) {
  return freq >= 1000 ? `${(freq / 1000).toFixed(freq % 1000 === 0 ? 0 : 1)}k` : `${Math.round(freq)}`;
}

/**
 * 频响曲线的纵轴量程（UI5-05）：默认 ±15 dB；响应超出时按 6 dB 一档放大到刚好
 * 容纳峰值，曲线不再被压平（频段与预放大各 ±24 dB，叠加后可以远超 15）。
 */
export function responseCurveRange(response: readonly number[]) {
  let peak = 0;
  for (const db of response) if (Number.isFinite(db)) peak = Math.max(peak, Math.abs(db));
  return peak <= 15 ? 15 : Math.ceil(peak / 6) * 6;
}

/** 频响曲线 SVG。横轴 log 频率，纵轴默认 ±15 dB，超出时自动放大。 */
function ResponseCurve({ preamp, bands }: { preamp: number; bands: EqBand[] }) {
  const width = 640;
  const height = 150;
  const { path, dbRange } = useMemo(() => {
    const points = logFreqPoints(160);
    const response = combinedResponseDb(preamp, bands, points);
    const dbRange = responseCurveRange(response);
    const logMin = Math.log10(20);
    const logMax = Math.log10(20_000);
    const d = response
      .map((db, index) => {
        const x = ((Math.log10(points[index]) - logMin) / (logMax - logMin)) * width;
        const y = height / 2 - (Math.max(-dbRange, Math.min(dbRange, db)) / dbRange) * (height / 2);
        return `${index === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
    return { path: d, dbRange };
  }, [preamp, bands]);

  const gridFreqs = [100, 1000, 10000];
  const logMin = Math.log10(20);
  const logMax = Math.log10(20_000);

  return (
    <div className="relative">
    <span className="pointer-events-none absolute right-1 top-0.5 font-tw text-[9px] font-bold text-ink3">
      ±{dbRange} dB
    </span>
    <svg
      viewBox={`0 0 ${width} ${height}`}
      className="h-[150px] w-full"
      preserveAspectRatio="none"
      role="img"
      aria-label={`频响曲线，纵轴 ±${dbRange} dB`}
    >
      {/* 0dB 中线 */}
      <line x1={0} y1={height / 2} x2={width} y2={height / 2} className="stroke-ink3" strokeWidth={1} strokeDasharray="4 3" />
      {/* ±中间刻度 */}
      {[-2 / 3, -1 / 3, 1 / 3, 2 / 3].map((fraction) => {
        const db = fraction * dbRange;
        const y = height / 2 - fraction * (height / 2);
        return (
          <line key={db} x1={0} y1={y} x2={width} y2={y} className="stroke-line" strokeWidth={0.5} />
        );
      })}
      {/* 频率网格 */}
      {gridFreqs.map((freq) => {
        const x = ((Math.log10(freq) - logMin) / (logMax - logMin)) * width;
        return (
          <line key={freq} x1={x} y1={0} x2={x} y2={height} className="stroke-line" strokeWidth={0.5} />
        );
      })}
      <path d={path} className="stroke-stamp" strokeWidth={2} fill="none" />
    </svg>
    </div>
  );
}

export function EqPage() {
  const eq = useEqStore();
  const showNotification = usePlayerStore((s) => s.showNotification);
  const [saveDialogOpen, setSaveDialogOpen] = useState(false);
  const [presetName, setPresetName] = useState("");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  // UI5-05：重置与删除预设不可撤销，改为「点一次待确认、3 秒内再点一次才执行」
  const [armed, setArmed] = useState<string | null>(null);
  useEffect(() => {
    if (armed === null) return;
    const timer = window.setTimeout(() => setArmed(null), 3000);
    return () => window.clearTimeout(timer);
  }, [armed]);
  const confirmThen = (key: string, action: () => void) => {
    if (armed !== key) {
      setArmed(key);
      return;
    }
    setArmed(null);
    action();
  };

  const presetSelectValue = eq.activePresetId ?? "custom";
  const risk = useMemo(() => clipRisk(eq.preamp, eq.bands), [eq.preamp, eq.bands]);

  const handlePresetChange = (value: string) => {
    if (value === "custom") return;
    const genre = GENRE_EQ_PRESETS.find((preset) => preset.id === value);
    if (genre) {
      eq.applyPreset(
        genre.bands.map((band) => ({ ...band })),
        genre.preamp,
        genre.id
      );
      return;
    }
    const user = eq.userPresets.find((preset) => preset.id === value);
    if (user) {
      eq.applyPreset(
        user.bands.map((band) => ({ ...band })),
        user.preamp,
        user.id
      );
    }
  };

  const handleSavePreset = (event: FormEvent) => {
    event.preventDefault();
    if (!presetName.trim()) return;
    eq.saveUserPreset(presetName);
    setPresetName("");
    setSaveDialogOpen(false);
    showNotification(`已保存 EQ 预设：${presetName.trim()}`);
  };

  const handleImport = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [
          { name: "EQ 预设", extensions: ["txt", "json"] },
        ],
      });
      if (typeof selected !== "string" || !selected) return;

      const content = await invoke<string>("import_eq_preset", { path: selected });
      const trimmed = content.trim();

      // 优先按本应用 JSON 预设解析，失败再按 AutoEq/APO 文本解析
      if (trimmed.startsWith("{")) {
        const parsed = JSON.parse(trimmed) as {
          preamp?: number;
          bands?: EqBand[];
        };
        // H-1：先消毒再入库——无效频段丢弃、超上限截断、preamp 钳制，
        // 提示段数以消毒后的实际数量为准（与 APO 分支同样导入后自动启用）
        const bands = sanitizeEqBands(parsed.bands);
        if (bands) {
          const dropped = Array.isArray(parsed.bands)
            ? parsed.bands.length - bands.length
            : 0;
          eq.setBands(bands, sanitizeEqPreamp(parsed.preamp), null);
          if (!eq.enabled) eq.setEnabled(true);
          showNotification(
            dropped > 0
              ? `已导入 JSON 预设（${bands.length} 段，忽略 ${dropped} 段无效数据）`
              : `已导入 JSON 预设（${bands.length} 段）`
          );
          return;
        }
      }

      const result = parseApoPreset(content);
      eq.setBands(result.bands, result.preamp, null);
      if (!eq.enabled) eq.setEnabled(true);
      showNotification(
        result.warnings.length > 0
          ? `已导入 ${result.bands.length} 段（${result.warnings[0]}）`
          : `已导入 AutoEq/APO 预设（${result.bands.length} 段）`
      );
    } catch (err) {
      const message =
        err instanceof Error ? err.message : normalizeIpcError(err).message;
      showNotification(`导入预设失败：${message}`, "error");
    }
  };

  const handleExport = async (format: "json" | "apo") => {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const target = await save({
        defaultPath: format === "json" ? "seraph-eq.json" : "seraph-eq.txt",
        filters: [
          format === "json"
            ? { name: "JSON 预设", extensions: ["json"] }
            : { name: "EqualizerAPO 预设", extensions: ["txt"] },
        ],
      });
      if (!target) return;

      const content =
        format === "json"
          ? JSON.stringify({ preamp: eq.preamp, bands: eq.bands }, null, 2)
          : toApoText(eq.preamp, eq.bands);
      await invoke("export_eq_preset", { path: target, content });
      showNotification(`已导出 EQ 预设（${format.toUpperCase()}）`);
    } catch (err) {
      showNotification(`导出预设失败：${normalizeIpcError(err).message}`, "error");
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
      {/* 顶部：总开关 + 预设 + 导入导出 */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-[1.5px] border-ink bg-card p-3">
        <div className="flex items-center gap-3">
          <ToggleSwitch
            checked={eq.enabled}
            onChange={eq.setEnabled}
            label="启用 EQ"
          />
          <span className="font-tw text-[11px] text-ink3">
            {eq.enabled ? "DSP 链已启用" : "DSP 链已停用（直通）"}
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <select
            value={presetSelectValue}
            onChange={(event) => handlePresetChange(event.target.value)}
            className="h-8 cursor-pointer border-[1.5px] border-line bg-card px-2 font-tw text-xs font-bold text-ink2 outline-none transition-colors hover:border-ink focus:border-ink"
            aria-label="选择预设"
          >
            <option value="custom">自定义</option>
            <optgroup label="曲风预设">
              {GENRE_EQ_PRESETS.map((preset) => (
                <option key={preset.id} value={preset.id}>
                  {preset.name}
                </option>
              ))}
            </optgroup>
            {eq.userPresets.length > 0 ? (
              <optgroup label="我的预设">
                {eq.userPresets.map((preset) => (
                  <option key={preset.id} value={preset.id}>
                    {preset.name}
                  </option>
                ))}
              </optgroup>
            ) : null}
          </select>
          <button
            type="button"
            onClick={() => setSaveDialogOpen(true)}
            className="stamp-btn inline-flex h-8 items-center gap-1.5 px-2.5 font-tw text-xs font-bold"
            title="保存当前为预设"
          >
            <Save className="h-3.5 w-3.5" />
            保存
          </button>
          <button
            type="button"
            onClick={() => void handleImport()}
            className="stamp-btn inline-flex h-8 items-center gap-1.5 px-2.5 font-tw text-xs font-bold"
            title="导入 AutoEq / EqualizerAPO / JSON 预设"
          >
            <Upload className="h-3.5 w-3.5" />
            导入
          </button>
          <button
            type="button"
            onClick={() => void handleExport("apo")}
            className="stamp-btn inline-flex h-8 items-center gap-1.5 px-2.5 font-tw text-xs font-bold"
            title="导出为 EqualizerAPO 文本"
          >
            <Download className="h-3.5 w-3.5" />
            APO
          </button>
          <button
            type="button"
            onClick={() => void handleExport("json")}
            className="stamp-btn inline-flex h-8 items-center gap-1.5 px-2.5 font-tw text-xs font-bold"
            title="导出为 JSON"
          >
            <Download className="h-3.5 w-3.5" />
            JSON
          </button>
        </div>
      </div>

      {/* 频响曲线 */}
      <div className="border-[1.5px] border-line bg-card p-3">
        <div className="mb-1 flex items-center justify-between">
          <span className="font-tw text-[10px] font-bold tracking-[2px] text-ink3">
            FREQUENCY RESPONSE
          </span>
          <span className="font-tw text-[10px] text-ink3">±15 dB · 20Hz–20kHz</span>
        </div>
        <ResponseCurve preamp={eq.preamp} bands={eq.bands} />
      </div>

      {/* 主控：Preamp + DSD 开关 */}
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <div className="border-[1.5px] border-line bg-card p-3">
          <div className="mb-2 flex items-center justify-between">
            <span className="font-tw text-[11px] font-bold text-ink2">
              预放大 Preamp
            </span>
            <span className="font-tw text-xs font-bold text-stamp">
              {eq.preamp > 0 ? "+" : ""}
              {eq.preamp.toFixed(1)} dB
            </span>
          </div>
          <input
            type="range"
            min={-24}
            max={24}
            step={0.5}
            value={eq.preamp}
            onChange={(event) => eq.setPreamp(Number.parseFloat(event.target.value))}
            className="w-full accent-stamp"
            aria-label="预放大"
          />
          {/* DSP-04：均衡器之后没有限幅器，提升量超过预放大留出的余量时提示并给出建议值 */}
          {risk.clipping ? (
            <div
              role="status"
              data-testid="eq-clip-warning"
              className="mt-2 flex flex-wrap items-center justify-between gap-2 border-[1.5px] border-stamp px-2 py-1.5"
            >
              <span className="font-tw text-[10px] leading-relaxed text-ink2">
                合成响应峰值超出 0 dB 约 {risk.overDb.toFixed(1)} dB，满幅素材会被削波。
              </span>
              <button
                type="button"
                onClick={() => eq.setPreamp(risk.suggestedPreamp)}
                className="stamp-btn inline-flex h-7 shrink-0 items-center px-2 font-tw text-[11px] font-bold"
              >
                预放大设为 {risk.suggestedPreamp.toFixed(1)} dB
              </button>
            </div>
          ) : null}
        </div>
        <div className="flex items-center justify-between border-[1.5px] border-line bg-card p-3">
          <div>
            <p className="font-tw text-[11px] font-bold text-ink2">
              EQ 对 DSD 生效
            </p>
            <p className="mt-0.5 font-tw text-[10px] leading-relaxed text-ink3">
              DSD 已解码为 PCM；默认不加处理以保「原汁」，开启后 EQ/crossfeed 同样作用于 DSD。
            </p>
          </div>
          <ToggleSwitch checked={eq.applyToDsd} onChange={eq.setApplyToDsd} label="" />
        </div>
      </div>

      {/* 频段竖 slider 阵列 */}
      <div className="border-[1.5px] border-line bg-card p-3">
        <div className="mb-3 flex items-center justify-between">
          <span className="font-tw text-[10px] font-bold tracking-[2px] text-ink3">
            BANDS · {eq.bands.length} 段
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => setAdvancedOpen((prev) => !prev)}
              className="font-tw text-[11px] font-bold text-ink2 underline-offset-2 hover:text-ink hover:underline"
            >
              {advancedOpen ? "收起高级编辑" : "高级编辑（频率/Q/类型）"}
            </button>
            <button
              type="button"
              onClick={() => confirmThen("reset", eq.resetBands)}
              className={`inline-flex items-center gap-1 font-tw text-[11px] font-bold hover:text-stamp ${armed === "reset" ? "text-stamp" : "text-ink2"}`}
              title="重置为平直（需再点一次确认）"
            >
              <RotateCcw className="h-3 w-3" />
              {armed === "reset" ? "再点一次确认" : "重置"}
            </button>
          </div>
        </div>

        {/* 竖直 slider 阵列（图形均衡器视图） */}
        <div className="flex items-end justify-between gap-1 overflow-x-auto pb-2">
          {eq.bands.map((band, index) => (
            <BandSlider
              key={index}
              band={band}
              onGain={(gain) => eq.setBandGain(index, gain)}
            />
          ))}
        </div>
      </div>

      {/* 高级逐段编辑 */}
      {advancedOpen ? (
        <div className="border-[1.5px] border-line bg-card p-3">
          <div className="mb-2 flex items-center justify-between">
            <span className="font-tw text-[10px] font-bold tracking-[2px] text-ink3">
              ADVANCED · 逐段参数
            </span>
            <button
              type="button"
              onClick={eq.addBand}
              className="stamp-btn inline-flex h-7 items-center gap-1 px-2 font-tw text-[11px] font-bold"
            >
              <Plus className="h-3 w-3" />
              添加频段
            </button>
          </div>
          <div className="space-y-1.5">
            <div className="grid grid-cols-[70px_minmax(0,1fr)_70px_60px_28px] items-center gap-2 font-tw text-[9px] font-bold tracking-wider text-ink3">
              <span>类型</span>
              <span>频率 Hz</span>
              <span>增益 dB</span>
              <span>Q</span>
              <span />
            </div>
            {eq.bands.map((band, index) => (
              <AdvancedBandRow
                // 频段没有稳定 ID：把段数拼进 key，增删频段时整组重建，
                // 数字框里未提交的草稿不会错位继承到后面的频段。
                key={`${eq.bands.length}:${index}`}
                band={band}
                onChange={(patch) => eq.updateBand(index, patch)}
                onRemove={() => eq.removeBand(index)}
              />
            ))}
          </div>
        </div>
      ) : null}

      {/* Crossfeed */}
      <div className="border-[1.5px] border-line bg-card p-3">
        <div className="mb-2 flex items-center justify-between">
          <div>
            <span className="font-tw text-[11px] font-bold text-ink2">
              Crossfeed（耳机串扰馈送）
            </span>
            <p className="mt-0.5 font-tw text-[10px] leading-relaxed text-ink3">
              把每个声道经低通后少量混入对侧，缓解耳机「脑内定位」的听感疲劳。
            </p>
          </div>
          <ToggleSwitch
            checked={eq.crossfeed.enabled}
            onChange={(enabled) => eq.setCrossfeed({ enabled })}
            label=""
          />
        </div>
        {eq.crossfeed.enabled ? (
          <div className="grid grid-cols-2 gap-4 pt-1">
            <label className="block">
              <span className="mb-1 flex justify-between font-tw text-[10px] font-bold text-ink3">
                <span>强度</span>
                <span className="text-stamp">{Math.round(eq.crossfeed.amount * 100)}%</span>
              </span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={eq.crossfeed.amount}
                onChange={(event) =>
                  eq.setCrossfeed({ amount: Number.parseFloat(event.target.value) })
                }
                className="w-full accent-stamp"
              />
            </label>
            <label className="block">
              <span className="mb-1 flex justify-between font-tw text-[10px] font-bold text-ink3">
                <span>截止频率</span>
                <span className="text-stamp">{Math.round(eq.crossfeed.cutoffHz)} Hz</span>
              </span>
              <input
                type="range"
                min={300}
                max={2000}
                step={50}
                value={eq.crossfeed.cutoffHz}
                onChange={(event) =>
                  eq.setCrossfeed({ cutoffHz: Number.parseFloat(event.target.value) })
                }
                className="w-full accent-stamp"
              />
            </label>
          </div>
        ) : null}
      </div>

      {/* 用户预设管理 */}
      {eq.userPresets.length > 0 ? (
        <div className="border-[1.5px] border-line bg-card p-3">
          <span className="mb-2 block font-tw text-[10px] font-bold tracking-[2px] text-ink3">
            MY PRESETS
          </span>
          <div className="flex flex-wrap gap-2">
            {eq.userPresets.map((preset) => (
              <div
                key={preset.id}
                className="inline-flex items-center gap-2 border-[1.5px] border-line bg-paper2 px-2.5 py-1"
              >
                <button
                  type="button"
                  onClick={() =>
                    eq.applyPreset(
                      preset.bands.map((band) => ({ ...band })),
                      preset.preamp,
                      preset.id
                    )
                  }
                  className="font-tw text-xs font-bold text-ink hover:text-stamp"
                >
                  {preset.name}
                </button>
                <button
                  type="button"
                  onClick={() => confirmThen(`preset:${preset.id}`, () => eq.deleteUserPreset(preset.id))}
                  className={`inline-flex items-center gap-1 font-tw text-[10px] font-bold hover:text-stamp ${armed === `preset:${preset.id}` ? "text-stamp" : "text-ink3"}`}
                  aria-label={armed === `preset:${preset.id}` ? `再点一次确认删除预设 ${preset.name}` : `删除预设 ${preset.name}`}
                >
                  <Trash2 className="h-3 w-3" />
                  {armed === `preset:${preset.id}` ? "确认删除" : null}
                </button>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      <Dialog open={saveDialogOpen} onClose={() => setSaveDialogOpen(false)} className="max-w-sm">
        <form onSubmit={handleSavePreset} className="space-y-4">
          <div>
            <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
              Save EQ Preset
            </p>
            <h2 className="mt-1 font-serif text-lg font-bold text-ink">保存 EQ 预设</h2>
          </div>
          <label className="block space-y-1.5">
            <span className="font-tw text-[11px] font-bold text-ink2">预设名称</span>
            <input
              value={presetName}
              onChange={(event) => setPresetName(event.target.value)}
              autoFocus
              placeholder="例如：我的耳机校正"
              className="h-10 w-full border-[1.5px] border-ink bg-card px-3 font-tw text-sm font-semibold text-ink outline-none transition-colors placeholder:text-ink3 focus:border-stamp"
            />
          </label>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setSaveDialogOpen(false)}
              className="stamp-btn h-9 px-3 font-tw text-xs font-bold"
            >
              取消
            </button>
            <button
              type="submit"
              disabled={!presetName.trim()}
              className="h-9 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp disabled:cursor-not-allowed disabled:bg-line disabled:border-line disabled:text-ink2"
            >
              保存
            </button>
          </div>
        </form>
      </Dialog>
    </div>
  );
}

function BandSlider({ band, onGain }: { band: EqBand; onGain: (gain: number) => void }) {
  return (
    <div className="flex min-w-[42px] flex-1 flex-col items-center gap-1">
      <span className="font-tw text-[9px] font-bold text-stamp">
        {band.gain > 0 ? "+" : ""}
        {band.gain.toFixed(1)}
      </span>
      <input
        type="range"
        min={-24}
        max={24}
        step={0.5}
        value={band.gain}
        onChange={(event) => onGain(Number.parseFloat(event.target.value))}
        // 竖直 slider
        className="eq-vertical-slider accent-stamp"
        style={{ writingMode: "vertical-lr", direction: "rtl", height: "120px" }}
        aria-label={`${formatFreq(band.freq)}Hz 增益`}
      />
      <span className="font-tw text-[9px] font-bold text-ink3">{formatFreq(band.freq)}</span>
    </div>
  );
}

function AdvancedBandRow({
  band,
  onChange,
  onRemove,
}: {
  band: EqBand;
  onChange: (patch: Partial<EqBand>) => void;
  onRemove: () => void;
}) {
  return (
    <div className="grid grid-cols-[70px_minmax(0,1fr)_70px_60px_28px] items-center gap-2">
      <select
        value={band.kind}
        onChange={(event) => onChange({ kind: event.target.value as EqBandKind })}
        aria-label="滤波器类型"
        className="h-7 cursor-pointer border-[1.5px] border-line bg-card px-1 font-tw text-[10px] font-bold text-ink2 outline-none focus:border-ink"
      >
        {(Object.keys(BAND_KIND_LABELS) as EqBandKind[]).map((kind) => (
          <option key={kind} value={kind}>
            {BAND_KIND_LABELS[kind]}
          </option>
        ))}
      </select>
      {/* BUG-04：草稿式输入，失焦 / 回车才提交（提交路径仍经 sanitizeEqBandPatch 钳制） */}
      <DraftNumberInput
        value={band.freq}
        format={(freq) => String(Math.round(freq))}
        onCommit={(freq) => onChange({ freq })}
        aria-label="中心频率（Hz）"
        className="h-7 min-w-0 border-[1.5px] border-line bg-card px-2 font-tw text-[11px] text-ink outline-none focus:border-ink"
      />
      <DraftNumberInput
        value={band.gain}
        onCommit={(gain) => onChange({ gain })}
        aria-label="增益（dB）"
        className="h-7 min-w-0 border-[1.5px] border-line bg-card px-2 font-tw text-[11px] text-ink outline-none focus:border-ink"
      />
      <DraftNumberInput
        value={band.q}
        onCommit={(q) => onChange({ q })}
        aria-label="Q 值"
        className="h-7 min-w-0 border-[1.5px] border-line bg-card px-2 font-tw text-[11px] text-ink outline-none focus:border-ink"
      />
      <button
        type="button"
        onClick={onRemove}
        className="flex h-7 w-7 items-center justify-center text-ink3 hover:text-stamp"
        aria-label="删除频段"
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

function ToggleSwitch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label || "开关"}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 shrink-0 items-center border-[1.5px] transition-colors ${
        checked ? "border-stamp bg-stamp/20" : "border-line bg-card"
      }`}
    >
      <span
        className={`absolute h-4 w-4 transition-transform ${
          checked ? "translate-x-[22px] bg-stamp" : "translate-x-[3px] bg-ink3"
        }`}
      />
    </button>
  );
}
