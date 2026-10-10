//! Capability and synthetic RGB fixture probe. Never point this at personal content.
use std::io::Write;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = lumen_windows::ocr::Engine::default();
    writeln!(std::io::stdout(), "language={}", engine.language()?)?;
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 4 {
        let width = args[2].parse()?;
        let height = args[3].parse()?;
        let rgb = std::fs::read(&args[1])?;
        for run in 0..5 {
            let start = std::time::Instant::now();
            let result = engine.recognize(width, height, &rgb, &|| false)?;
            writeln!(
                std::io::stdout(),
                "run={run} ms={:.1} text_bytes={}",
                start.elapsed().as_secs_f64() * 1000.0,
                result.text.len()
            )?;
            assert!(
                result.text.contains("ERROR 42"),
                "synthetic exact text not recognized"
            );
        }
        let empty = engine.recognize(201, 99, &[255; 201 * 99 * 3], &|| false)?;
        assert!(empty.text.is_empty());
        assert!(matches!(
            engine.recognize(width, height, &rgb, &|| true),
            Err(lumen_windows::ocr::Error::Cancelled)
        ));
        let checks = std::cell::Cell::new(0);
        let cancelled = engine.recognize(width, height, &rgb, &|| {
            checks.set(checks.get() + 1);
            checks.get() >= 4
        });
        assert!(matches!(
            cancelled,
            Err(lumen_windows::ocr::Error::Cancelled)
        ));
        writeln!(std::io::stdout(), "empty_unaligned_and_preempt=ok")?;
    }
    Ok(())
}
