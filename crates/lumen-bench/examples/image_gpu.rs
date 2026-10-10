//! Isolated T213 image CPU/DirectML proof with public T303 fixtures. No user DB.
use lumen_embedding::{Embedder, EmbeddingProfile, ImageInput, dot};
use lumen_embedding_ort::{Device, ModelVariant, OrtBackend, OrtConfig};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("release required".into());
    }
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let warm = args.get(5).is_some_and(|arg| arg == "--warm");
    if args.len() != 5 && !(args.len() == 6 && warm) {
        return Err("model vision runtime public-fixtures output-json [--warm]".into());
    }
    lumen_embedding_ort::init_runtime(&args[2])?;
    let mut reference = Vec::new();
    let mut runs = Vec::new();
    for (label, device, image_device) in [
        ("cpu", Device::Cpu, Device::Cpu),
        (
            "gpu-text-cpu-images",
            Device::DirectMl { adapter: 0 },
            Device::Cpu,
        ),
        (
            "gpu-text-gpu-backbone",
            Device::DirectMl { adapter: 0 },
            Device::DirectMl { adapter: 0 },
        ),
    ] {
        let mut cfg = OrtConfig::new(&args[0], ModelVariant::Q4, device);
        cfg.vision_dir = Some(args[1].clone());
        cfg.vision_device = Device::Cpu;
        cfg.image_device = image_device;
        cfg.threads = Some(2);
        let e = Embedder::new(Arc::new(OrtBackend::new(cfg)?), EmbeddingProfile::DEFAULT)?;
        let before = e.embed_query("local synthetic text fidelity", None)?;
        let mut times = Vec::new();
        let mut cosines = Vec::new();
        let mut cycle_ms = Vec::new();
        for (index, name) in ["0001.jpg", "0002.jpg"].iter().enumerate() {
            let image = lumen_image::decode(&args[3].join(name), None, &|| false)?;
            if warm {
                // Compare resident modality shapes; first-use DirectML compilation is separate.
                e.embed_images(
                    &[ImageInput {
                        width: image.metadata.width,
                        height: image.metadata.height,
                        rgb: &image.rgb,
                    }],
                    None,
                )?;
            }
            e.embed_query("local synthetic text fidelity", None)?;
            let cycle = Instant::now();
            let start = Instant::now();
            let v = e
                .embed_images(
                    &[ImageInput {
                        width: image.metadata.width,
                        height: image.metadata.height,
                        rgb: &image.rgb,
                    }],
                    None,
                )?
                .into_flat();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            e.embed_query("local synthetic text fidelity", None)?;
            cycle_ms.push(cycle.elapsed().as_secs_f64() * 1000.0);
            if device == Device::Cpu {
                reference.push(v);
            } else {
                cosines.push(dot(&reference[index], &v));
            }
        }
        let after = e.embed_query("local synthetic text fidelity", None)?;
        runs.push(serde_json::json!({"mode":label,"device":device.to_string(),"image_ms":times,"mixed_cycle_ms":cycle_ms,"cosines_vs_cpu":cosines,"post_image_text_cosine":dot(&before,&after)}));
    }
    std::fs::write(
        &args[4],
        serde_json::to_vec_pretty(&serde_json::json!({"warm":warm,"runs":runs}))?,
    )?;
    Ok(())
}
