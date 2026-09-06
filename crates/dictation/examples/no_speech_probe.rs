//! Probes what the decoder makes of cuts of a recording and of silence, and
//! whether the Speech Gate would have let each cut through, so neither is
//! ever trusted blindly.
//!
//! cargo run -p dictation --example no_speech_probe -- <model.bin> <backends_dir> <file.wav>

use std::path::PathBuf;

use dictation::{ENGINE_SAMPLE_RATE, EngineConfig, SpeechGate, Transcriber, load_audio_file};

fn probe(transcriber: &mut Transcriber, label: &str, pcm: &[f32]) -> anyhow::Result<()> {
    let plain = transcriber.segments(pcm)?;
    let mut gate = SpeechGate::new();
    let events = gate.feed(pcm);
    println!(
        "{label}: plain={:?} gate={} ({} transitions, speech ends at {})",
        plain
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>(),
        if gate.is_open() { "open" } else { "closed" },
        events.len(),
        gate.speech_end()
            .map(|end| format!("{:.2}s", end as f32 / ENGINE_SAMPLE_RATE as f32))
            .unwrap_or_else(|| "never".to_string())
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let model_path = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("model path"))?);
    let backends_dir = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("backends dir"))?);
    let file = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("wav"))?);
    let config = EngineConfig {
        model_path,
        backends_dir: Some(backends_dir),
        language: Some("ru".into()),
        glossary: Vec::new(),
        threads: 0,
    };
    let mut transcriber = Transcriber::load(&config)?;
    let pcm = load_audio_file(&file)?;
    let rate = ENGINE_SAMPLE_RATE as f32;
    for seconds in [0.6f32, 1.0, 1.5, 2.0, 3.0, 5.0] {
        let cut = &pcm[..((seconds * rate) as usize).min(pcm.len())];
        probe(&mut transcriber, &format!("first {seconds:.1}s"), cut)?;
    }
    let silence = vec![0.0f32; (6.0 * rate) as usize];
    for seconds in [1.0f32, 2.0, 3.0] {
        let mut cut = pcm[..((seconds * rate) as usize).min(pcm.len())].to_vec();
        cut.extend_from_slice(&silence);
        probe(
            &mut transcriber,
            &format!("first {seconds:.1}s + 6s silence"),
            &cut,
        )?;
    }
    let mut silent = vec![0.0f32; (8.0 * rate) as usize];
    for (index, sample) in silent.iter_mut().enumerate() {
        *sample = ((index * 7919) % 1000) as f32 / 1000.0 * 0.002 - 0.001;
    }
    probe(&mut transcriber, "8s near-silence", &silent)?;
    Ok(())
}
