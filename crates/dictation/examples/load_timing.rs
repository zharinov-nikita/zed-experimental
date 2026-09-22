//! Local debug probe: splits Model Loading into its stages so a slow start can
//! be attributed to backend initialization, reading the model or creating the
//! session. Loads twice to separate the one-time backend cost from the
//! per-model cost.
//!
//! cargo run -p dictation --example load_timing -- <model.bin> <backends_dir>

use std::path::PathBuf;
use std::time::Instant;

use dictation::{EngineConfig, Transcriber};
use transcribe_cpp::{Backend, Model, ModelOptions, SessionOptions};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let model_path = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("model path"))?);
    let backends_dir = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("backends dir"))?);
    let backend = match args.next().as_deref() {
        None | Some("auto") => Backend::Auto,
        Some("cpu") => Backend::Cpu,
        Some("vulkan") => Backend::Vulkan,
        Some(other) => anyhow::bail!("unknown backend {other}"),
    };

    let started = Instant::now();
    transcribe_cpp::init_backends(&backends_dir)
        .map_err(|error| anyhow::anyhow!("initializing speech backends: {error}"))?;
    println!("init_backends      {:>8.0} ms", started.elapsed().as_secs_f64() * 1000.0);

    let raw_passes = std::env::var("LOAD_TIMING_TRANSCRIBER_ONLY").is_err();
    for pass in 1..=2 {
        if !raw_passes {
            break;
        }
        let model_options = ModelOptions {
            backend,
            ..Default::default()
        };
        let started = Instant::now();
        let model = Model::load_with(&model_path, &model_options)?;
        let model_ms = started.elapsed().as_secs_f64() * 1000.0;

        let started = Instant::now();
        let session = model.session_with(&SessionOptions::default())?;
        let session_ms = started.elapsed().as_secs_f64() * 1000.0;

        println!("pass {pass}: Model::load_with {model_ms:>8.0} ms   session_with {session_ms:>8.0} ms   backend {}", model.backend());

        let started = Instant::now();
        drop(session);
        drop(model);
        println!("pass {pass}: drop           {:>8.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    }
    let config = EngineConfig {
        model_path,
        backends_dir: Some(backends_dir),
        language: Some("ru".into()),
        glossary: Vec::new(),
        threads: 0,
    };
    for pass in 1..=2 {
        let started = Instant::now();
        let transcriber = Transcriber::load(&config)?;
        println!(
            "pass {pass}: Transcriber::load {:>8.0} ms   backend {}",
            started.elapsed().as_secs_f64() * 1000.0,
            transcriber.backend()
        );
        drop(transcriber);
    }
    Ok(())
}
