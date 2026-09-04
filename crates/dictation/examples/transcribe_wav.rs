//! Recognizes audio files with the dictation engine and prints text and timings.
//!
//! cargo run -p dictation --example transcribe_wav -- <model.bin> <backends_dir> <file.wav>...

use std::path::PathBuf;
use std::time::Instant;

use dictation::{EngineConfig, Transcriber, load_audio_file};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let model_path = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("model path"))?);
    let backends_dir = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("backends dir"))?);
    let files: Vec<PathBuf> = args.map(PathBuf::from).collect();

    let config = EngineConfig {
        model_path,
        backends_dir: Some(backends_dir),
        language: Some("ru".into()),
        glossary: vec![
            "TypeScript".into(),
            "JavaScript".into(),
            "GitHub".into(),
            "Docker".into(),
            "Kubernetes".into(),
            "API".into(),
            "Rust".into(),
            "PowerShell".into(),
            "Claude Code".into(),
            "Ollama".into(),
            "frontend".into(),
            "backend".into(),
            "deploy".into(),
            "commit".into(),
            "pull request".into(),
            "merge".into(),
            "refactoring".into(),
        ],
        threads: 0,
    };

    let load_started = Instant::now();
    let mut transcriber = Transcriber::load(&config)?;
    println!(
        "model loaded in {:.1}s, backend: {}",
        load_started.elapsed().as_secs_f32(),
        transcriber.backend()
    );

    for file in files {
        let pcm = load_audio_file(&file)?;
        let audio_seconds = pcm.len() as f32 / dictation::ENGINE_SAMPLE_RATE as f32;
        let started = Instant::now();
        let segments = transcriber.segments(&pcm)?;
        let elapsed = started.elapsed().as_secs_f32();
        println!(
            "\n== {} ({audio_seconds:.1}s audio, {elapsed:.2}s recognition, RTF {:.2})",
            file.display(),
            elapsed / audio_seconds.max(0.01)
        );
        for segment in segments {
            println!(
                "[{:6.2} - {:6.2}] {}",
                segment.start.as_secs_f32(),
                segment.end.as_secs_f32(),
                segment.text
            );
        }
    }
    Ok(())
}
