// 由 benchmark-analysis.mjs 追加到 analysis.rs 副本，不进入应用构建。
fn benchmark_snapshots(engine: &mut AnalysisEngine, cold: bool) -> (f64, f64) {
    use std::{hint::black_box, time::Instant};
    let batch = if cold { 1 } else { 1000 };
    let mut timings = Vec::with_capacity(100);
    for index in 0..110 {
        if cold {
            engine.integrated_cache.take();
            engine.lra_cache.take();
        }
        let start = Instant::now();
        for _ in 0..batch {
            black_box(black_box(&*engine).snapshot());
        }
        let elapsed = start.elapsed().as_secs_f64() * 1000.0 / batch as f64;
        if index >= 10 {
            timings.push(elapsed);
        }
    }
    timings.sort_by(f64::total_cmp);
    (timings[49], timings[94])
}

fn main() {
    for seconds in [60_usize, 3600, 20000] {
        let mut engine = AnalysisEngine::new(48_000, 2);
        let mut seed = 20_260_913_u64;
        let mut energy = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            1.0e-8 + ((seed >> 32) as u32 as f64 / u32::MAX as f64) * 0.2
        };
        engine.gating_blocks = (0..seconds * 10).map(|_| energy()).collect();
        engine.lra_samples = (0..seconds).map(|_| energy()).collect();
        let hot = benchmark_snapshots(&mut engine, false);
        let cold = benchmark_snapshots(&mut engine, true);
        println!(
            r#"{{"historySeconds":{seconds},"hot":{{"p50Ms":{:.6},"p95Ms":{:.6}}},"cold":{{"p50Ms":{:.6},"p95Ms":{:.6}}}}}"#,
            hot.0, hot.1, cold.0, cold.1
        );
    }
}
