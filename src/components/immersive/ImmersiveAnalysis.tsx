import { useState } from "react";
import { usePlayerStore } from "@/store/player";
import { useImmersiveAnalysis } from "./useImmersiveAnalysis";

export function ImmersiveAnalysis() {
  const playing = usePlayerStore((s) => s.isPlaying);
  const [frozen, setFrozen] = useState(false);
  const analysis = useImmersiveAnalysis(playing, frozen);
  return (
    <div className="immersive-analysis">
      <section className="immersive-spectrum" aria-label="频谱分析">
        <header className="immersive-analysis-heading"><h2>频谱分析 <small>/ SPECTRUM</small></h2><button aria-pressed={frozen} onClick={() => setFrozen((value) => !value)}>{frozen ? "继续分析" : "冻结画面"}</button></header>
        <div className="immersive-spectrum-legend"><span><i className="immersive-legend-bar" />瞬时频谱</span><span><i className="immersive-legend-peak" />峰值保持</span><span ref={analysis.frameTimeRef}>FRAME / --:--</span></div>
        <div className="immersive-canvas"><canvas ref={analysis.spectrumRef} tabIndex={0} role="img" aria-label="频谱，可用左右方向键查看频段" aria-describedby="immersive-spectrum-readout" onPointerMove={analysis.onPointerMove} onPointerLeave={analysis.onPointerLeave} onKeyDown={analysis.onKeyDown} /></div>
        <div className="immersive-analysis-readout"><output id="immersive-spectrum-readout" ref={analysis.frequencyRef}>20 Hz — 20 kHz · dBFS</output><span ref={analysis.statusRef}>等待音频信号</span></div>
      </section>
      <div className="immersive-analysis-bottom">
        <section className="immersive-field" aria-label="立体声场">
          <header className="immersive-analysis-heading"><h3>立体声场</h3><span>STEREO FIELD</span></header>
          <div className="immersive-field-body"><div className="immersive-canvas"><canvas ref={analysis.fieldRef} role="img" aria-label="左右声道矢量图" /></div><div className="immersive-correlation"><output aria-label="声道相关度" ref={analysis.correlationRef}>—</output><small>声道相关度</small></div></div>
        </section>
        <section className="immersive-levels" aria-label="声道电平">
          <header className="immersive-analysis-heading"><h3>声道电平</h3><span>RMS / PEAK · dBFS</span></header>
          <div className="immersive-canvas"><canvas ref={analysis.levelsRef} aria-hidden="true" /></div>
          <output ref={analysis.levelsTextRef} className="sr-only" aria-label="左右声道电平">等待音频信号</output>
        </section>
      </div>
    </div>
  );
}
