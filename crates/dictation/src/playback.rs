//! Playback of Session Audio on the configured output device. The output
//! stream lives on its own thread for as long as the sound plays or until it
//! is stopped, so the section that started it never blocks.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, Result};
use cpal::DeviceId;
use rodio::Player;

/// How often the playback thread checks whether the sound ended or was stopped.
const POLL: Duration = Duration::from_millis(50);

pub struct Playback {
    stop: Arc<AtomicBool>,
}

impl Playback {
    /// Decodes `path` and plays it on `device` (`None` is the system
    /// default). `on_finished` runs on the playback thread once the sound
    /// ends or is stopped, with the error when the output device could not
    /// be opened.
    pub fn start(
        path: &Path,
        device: Option<DeviceId>,
        on_finished: impl FnOnce(Result<()>) + Send + 'static,
    ) -> Result<Self> {
        let file =
            std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let source = rodio::Decoder::new(std::io::BufReader::new(file))
            .with_context(|| format!("decoding {}", path.display()))?;
        let stop = Arc::new(AtomicBool::new(false));
        thread::Builder::new()
            .name("DictationPlayback".into())
            .spawn({
                let stop = stop.clone();
                move || {
                    let outcome = match audio::open_test_output(device) {
                        Ok(output) => {
                            let player = Player::connect_new(output.mixer());
                            player.append(source);
                            while !stop.load(Ordering::Relaxed) && !player.empty() {
                                thread::sleep(POLL);
                            }
                            player.stop();
                            Ok(())
                        }
                        Err(error) => Err(error.context("opening the output device")),
                    };
                    on_finished(outcome);
                }
            })
            .context("spawning playback thread")?;
        Ok(Self { stop })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop();
    }
}
