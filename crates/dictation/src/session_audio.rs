//! Session Audio: the sound of every Dictation Session, kept locally so the
//! user can replay it and a bad recognition can be reproduced. One 16 kHz
//! mono WAV per Dictation Block, named by the block id; a Resume appends to
//! the block's file. The store keeps at most `keep` files and evicts the
//! oldest by modification time.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result, anyhow};

use crate::{load_audio_file, replace_file, save_recording};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAudioStore {
    dir: PathBuf,
    keep: usize,
}

impl SessionAudioStore {
    /// `keep` is the number of files retained; `0` turns Session Audio off so
    /// nothing is written.
    pub fn new(dir: PathBuf, keep: u32) -> Self {
        Self {
            dir,
            keep: keep as usize,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.keep > 0
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, block_id: &str) -> Result<PathBuf> {
        if block_id.is_empty()
            || block_id
                .chars()
                .any(|character| matches!(character, '/' | '\\' | ':' | '.'))
        {
            return Err(anyhow!("invalid Dictation Block id {block_id:?}"));
        }
        Ok(self.dir.join(format!("{block_id}.wav")))
    }

    /// The Session Audio of `block_id`, if Session Audio is on and the file
    /// is still there. With `keep = 0` a file left over from earlier is not
    /// offered either: the user turned the feature off.
    pub fn existing(&self, block_id: &str) -> Option<PathBuf> {
        if !self.is_enabled() {
            return None;
        }
        self.path_for(block_id).ok().filter(|path| path.is_file())
    }

    /// Appends `pcm` (16 kHz mono) to the block's Session Audio, creating the
    /// file if needed, then evicts the oldest files beyond `keep`. Returns the
    /// path written, or `None` when Session Audio is off.
    pub fn append(&self, block_id: &str, pcm: &[f32]) -> Result<Option<PathBuf>> {
        if !self.is_enabled() {
            return Ok(None);
        }
        let path = self.path_for(block_id)?;
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let joined;
        let pcm = if path.is_file() {
            let mut existing = load_audio_file(&path)?;
            existing.extend_from_slice(pcm);
            joined = existing;
            joined.as_slice()
        } else {
            pcm
        };
        // Written next to the block file and swapped in, so a failure while
        // writing a Resume never destroys the sound already saved.
        let staging = path.with_extension("tmp");
        save_recording(pcm, &staging)?;
        replace_file(&staging, &path)?;
        self.evict(&path)?;
        Ok(Some(path))
    }

    /// Deletes the oldest WAVs so that at most `keep` remain. The file just
    /// written is never a candidate, whatever its timestamp says.
    fn evict(&self, just_written: &Path) -> Result<()> {
        let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(&self.dir)
            .with_context(|| format!("reading {}", self.dir.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "wav"))
            .filter(|path| path != just_written)
            .filter_map(|path| {
                let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
                Some((modified, path))
            })
            .collect();
        files.sort();
        let excess = (files.len() + 1).saturating_sub(self.keep);
        for (_, path) in files.into_iter().take(excess) {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
        Ok(())
    }
}

/// Where one Dictation Session writes its sound: the store plus the id of
/// the Dictation Block the session belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAudioSink {
    pub store: SessionAudioStore,
    pub block_id: String,
}

impl SessionAudioSink {
    pub fn append(&self, pcm: &[f32]) -> Result<Option<PathBuf>> {
        self.store.append(&self.block_id, pcm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ENGINE_SAMPLE_RATE;
    use std::time::Duration;

    fn tone(seconds: u32) -> Vec<f32> {
        (0..ENGINE_SAMPLE_RATE * seconds)
            .map(|index| ((index % 80) as f32 / 80.0 - 0.5) * 0.5)
            .collect()
    }

    fn wav_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn age(path: &Path, seconds: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn keeping_n_files_evicts_the_oldest_by_modification_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionAudioStore::new(dir.path().to_path_buf(), 2);
        let pcm = tone(1);

        let first = store.append("first", &pcm).unwrap().unwrap();
        let second = store.append("second", &pcm).unwrap().unwrap();
        age(&first, 30);
        age(&second, 10);
        store.append("third", &pcm).unwrap();

        assert_eq!(wav_names(dir.path()), vec!["second.wav", "third.wav"]);
    }

    #[test]
    fn resume_appends_to_the_block_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionAudioStore::new(dir.path().to_path_buf(), 5);

        let path = store.append("block", &tone(1)).unwrap().unwrap();
        store.append("block", &tone(2)).unwrap();

        assert_eq!(wav_names(dir.path()), vec!["block.wav"]);
        assert_eq!(store.existing("block"), Some(path.clone()));
        let samples = load_audio_file(&path).unwrap();
        assert_eq!(samples.len(), ENGINE_SAMPLE_RATE as usize * 3);
    }

    #[test]
    fn a_discarded_session_is_kept_under_its_own_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionAudioStore::new(dir.path().to_path_buf(), 5);

        store.append("accepted", &tone(1)).unwrap();
        store.append("discarded", &tone(1)).unwrap();

        assert_eq!(wav_names(dir.path()), vec!["accepted.wav", "discarded.wav"]);
        assert!(store.existing("discarded").is_some());
        assert_eq!(store.existing("missing"), None);
    }

    #[test]
    fn keep_zero_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionAudioStore::new(dir.path().join("audio"), 0);

        assert!(!store.is_enabled());
        assert_eq!(store.append("block", &tone(1)).unwrap(), None);
        assert!(!dir.path().join("audio").exists());
    }

    #[test]
    fn keep_zero_offers_no_file_even_when_one_is_left_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = SessionAudioStore::new(dir.path().to_path_buf(), 5)
            .append("block", &tone(1))
            .unwrap()
            .unwrap();
        assert!(path.is_file());

        let disabled = SessionAudioStore::new(dir.path().to_path_buf(), 0);
        assert_eq!(disabled.existing("block"), None);
    }

    #[test]
    fn block_ids_that_leave_the_directory_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionAudioStore::new(dir.path().to_path_buf(), 5);
        assert!(store.append("../escape", &tone(1)).is_err());
        assert!(store.append("", &tone(1)).is_err());
    }
}
