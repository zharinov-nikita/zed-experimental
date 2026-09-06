//! Probes the no-speech gate on cuts of a recording: which prefixes of the
//! speech survive `tail_segments`, so the gate is never trusted blindly.
//!
//! cargo run -p dictation --example no_speech_probe -- <model.bin> <backends_dir> <file.wav>

use std::path::PathBuf;

use dictation::{ENGINE_SAMPLE_RATE, EngineConfig, Transcriber, load_audio_file};

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
        let plain = transcriber.segments(cut)?;
        let tail = transcriber.tail_segments(cut, "", "")?;
        println!(
            "first {seconds:.1}s: plain={:?} tail={:?}",
            plain
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            tail.iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
        );
    }
    let silence = vec![0.0f32; (6.0 * rate) as usize];
    for seconds in [1.0f32, 2.0, 3.0] {
        let mut cut = pcm[..((seconds * rate) as usize).min(pcm.len())].to_vec();
        cut.extend_from_slice(&silence);
        let plain = transcriber.segments(&cut)?;
        let tail = transcriber.tail_segments(&cut, "", "")?;
        println!(
            "first {seconds:.1}s + 6s silence: plain={:?} tail={:?}",
            plain
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            tail.iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
        );
    }
    let mut silent = vec![0.0f32; (8.0 * rate) as usize];
    for (index, sample) in silent.iter_mut().enumerate() {
        *sample = ((index * 7919) % 1000) as f32 / 1000.0 * 0.002 - 0.001;
    }
    let plain = transcriber.segments(&silent)?;
    let tail = transcriber.tail_segments(&silent, "", "")?;
    println!(
        "8s near-silence: plain={:?} tail={:?}",
        plain
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>(),
        tail.iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
    );
    Ok(())
}
