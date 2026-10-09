//! T302 OS raster latency (synthetic local PDFs; no Tauri/live DB/model).
#![allow(clippy::unwrap_used)]
#[cfg(windows)]
#[path = "../../../fixtures/pdf/mod.rs"]
mod fixture;
#[cfg(windows)]
#[path = "../src/machine.rs"]
mod machine;
#[cfg(windows)]
#[path = "../src/stats.rs"]
mod stats;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use lumen_windows::pdf::Renderer;
    use serde_json::json;
    use std::{path::PathBuf, time::Instant};
    if cfg!(debug_assertions) {
        return Err("release build required".into());
    }
    let output = std::env::args_os().nth(1).ok_or("provide output.json")?;
    let dir = std::env::temp_dir().join(format!("lumen-t302-bench-{}", std::process::id()));
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir = Temp(dir);
    std::fs::create_dir_all(&dir.0)?;
    let file = dir.0.join("synthetic.pdf");
    let text = b"Coastal research and solar energy. A local synthetic PDF for preview timing.";
    fixture::document(&vec![text.as_slice(); 128]).save(&file)?;
    let before = machine::memory();
    let mut renderer = Renderer::default();
    let started = Instant::now();
    let first = renderer
        .render(&file, 7, &|| false)
        .map_err(|e| e.message())?;
    let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
    // Save only synthetic render output for visual verification, outside the repo.
    let sample = PathBuf::from(&output).with_extension("png");
    if let Some(parent) = sample.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&sample, &first.png)?;
    let mut cached = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        renderer
            .render(&file, 7, &|| false)
            .map_err(|e| e.message())?;
        cached.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let mut uncached = Vec::new();
    for page in 8..=37 {
        let start = Instant::now();
        renderer
            .render(&file, page, &|| false)
            .map_err(|e| e.message())?;
        uncached.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let after = machine::memory();
    let report = json!({"task":"T302", "build":"release", "engine":"Windows.Data.Pdf", "machine": machine::MachineInfo::collect(), "physical_pages":128,"source_bytes":std::fs::metadata(&file)?.len(),"cold_ms":cold_ms,"cached":stats::Summary::of(&cached),"uncached_page":stats::Summary::of(&uncached),"image":{"width":first.width,"height":first.height,"png_bytes":first.png.len()},"memory_before":before,"memory_after":after});
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
#[cfg(not(windows))]
fn main() {}
