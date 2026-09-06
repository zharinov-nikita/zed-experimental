//! Recognizes audio files with the dictation engine and prints text and timings,
//! plus where the Speech Gate sees speech begin and end and what the engine
//! recognizes when the file is cut there, as it is on stop.
//!
//! cargo run -p dictation --example transcribe_wav -- <model.bin> <backends_dir> <file.wav>...

use std::path::PathBuf;
use std::time::Instant;

use dictation::{
    ENGINE_SAMPLE_RATE, EngineConfig, GateEvent, SpeechGate, Transcriber, load_audio_file,
};

fn seconds(samples: usize) -> f32 {
    samples as f32 / ENGINE_SAMPLE_RATE as f32
}

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
        let audio_seconds = seconds(pcm.len());
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

        let mut gate = SpeechGate::new();
        let events: Vec<String> = gate
            .feed(&pcm)
            .into_iter()
            .map(|event| match event {
                GateEvent::Opened { start } => format!("open@{:.2}", seconds(start)),
                GateEvent::Closed { end } => format!("close@{:.2}", seconds(end)),
            })
            .collect();
        println!("speech gate: {}", events.join("  "));
        let speech_end = gate.speech_end().unwrap_or(0);
        let tail = transcriber.segments(&pcm[..speech_end.min(pcm.len())])?;
        println!(
            "as a stop tail (cut at the gate's end of speech, {:.2}s): {:?}",
            seconds(speech_end),
            tail.iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}
