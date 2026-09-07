//! The Speech Gate: decides whether the audio the live loop has not decoded
//! yet contains speech at all, by comparing short frames with the noise of
//! the same session (ADR 0002). It never sees text; it only tells the loop
//! where speech begins and ends, in sample positions since the start of the
//! session.
//!
//! The gate errs on the side of speech: it opens after a tenth of a second
//! above the threshold and reports a start well before that, but closes only
//! after a long stretch of clear silence. The constants were tuned on
//! recordings made with the user's microphone, whose driver gates quiet
//! frames to digital zero, so the noise floor has an absolute lower bound and
//! the opening threshold an absolute minimum.

use std::time::Duration;

use crate::duration_to_samples;

/// Loudness is measured per frame of this length.
pub const FRAME: Duration = Duration::from_millis(20);
/// The noise floor never drops below this, so digital silence does not make
/// every breath count as speech.
const FLOOR_MIN_DB: f32 = -70.0;
/// The floor follows quiet frames down at once and rises this slowly, so a
/// loud phrase does not lift it.
const FLOOR_RISE_DB_PER_SECOND: f32 = 3.0;
/// A frame counts as speech when it is this far above the noise floor…
const OPEN_ABOVE_FLOOR_DB: f32 = 10.0;
/// …and at least this loud in absolute terms.
const OPEN_MIN_DB: f32 = -50.0;
/// Consecutive speech frames needed to open; a keyboard click is shorter.
const OPEN_FRAMES: usize = 5;
/// Audio before the first speech frame that is decoded with it, so the
/// decoder sees the onset the gate reacted to.
const OPEN_MARGIN: Duration = Duration::from_millis(400);
/// Silence needed to close. A speaker thinking mid-sentence pauses for up to
/// two seconds; splitting there costs the decoder the context of the
/// sentence and loses words.
const CLOSE_SILENCE: Duration = Duration::from_millis(3000);
/// Audio after the last speech frame that is still decoded with it.
const CLOSE_MARGIN: Duration = Duration::from_millis(500);

/// What the gate reports as it consumes audio. Positions are samples since
/// the start of the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateEvent {
    /// Speech began; decoding should start at `start`.
    Opened { start: usize },
    /// Speech ended at `end`; nothing after it needs decoding until the next
    /// `Opened`.
    Closed { end: usize },
}

pub struct SpeechGate {
    open: bool,
    floor_db: f32,
    speech_frames: usize,
    silent_frames: usize,
    position: usize,
    /// Samples of an incomplete frame, kept until the next `feed`.
    partial: Vec<f32>,
    /// End of the last speech frame seen while open.
    last_speech_end: Option<usize>,
}

impl Default for SpeechGate {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechGate {
    /// A gate for a new session: closed, with the noise floor at its minimum
    /// so that the first word opens it as easily as any later one.
    pub fn new() -> Self {
        Self {
            open: false,
            floor_db: FLOOR_MIN_DB,
            speech_frames: 0,
            silent_frames: 0,
            position: 0,
            partial: Vec::with_capacity(frame_samples()),
            last_speech_end: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Samples the gate has consumed so far.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Where the speech the gate has let through ends: the last speech frame
    /// plus [`CLOSE_MARGIN`], never beyond what was fed. `None` until the
    /// gate has opened once.
    pub fn speech_end(&self) -> Option<usize> {
        self.last_speech_end
            .map(|end| (end + duration_to_samples(CLOSE_MARGIN)).min(self.position))
    }

    /// Consumes 16 kHz mono samples that continue the ones fed before and
    /// returns the transitions they caused, in order.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<GateEvent> {
        let frame = frame_samples();
        let mut events = Vec::new();
        let mut rest = samples;
        if !self.partial.is_empty() {
            let needed = frame - self.partial.len();
            let take = needed.min(rest.len());
            self.partial.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.partial.len() < frame {
                return events;
            }
            let level = level_db(&self.partial);
            self.partial.clear();
            events.extend(self.frame(level));
        }
        let mut chunks = rest.chunks_exact(frame);
        for chunk in &mut chunks {
            events.extend(self.frame(level_db(chunk)));
        }
        self.partial.extend_from_slice(chunks.remainder());
        events
    }

    fn frame(&mut self, level_db: f32) -> Option<GateEvent> {
        let rise = FLOOR_RISE_DB_PER_SECOND * FRAME.as_secs_f32();
        self.floor_db = level_db.min(self.floor_db + rise).max(FLOOR_MIN_DB);
        let threshold = (self.floor_db + OPEN_ABOVE_FLOOR_DB).max(OPEN_MIN_DB);
        self.position += frame_samples();

        if level_db >= threshold {
            self.speech_frames += 1;
            self.silent_frames = 0;
            if !self.open && self.speech_frames >= OPEN_FRAMES {
                self.open = true;
                let run_start = self.position - self.speech_frames * frame_samples();
                self.last_speech_end = Some(self.position);
                return Some(GateEvent::Opened {
                    start: run_start.saturating_sub(duration_to_samples(OPEN_MARGIN)),
                });
            }
            if self.open {
                self.last_speech_end = Some(self.position);
            }
        } else {
            self.speech_frames = 0;
            self.silent_frames += 1;
            if self.open
                && self.silent_frames * frame_samples() >= duration_to_samples(CLOSE_SILENCE)
            {
                self.open = false;
                self.silent_frames = 0;
                return self.speech_end().map(|end| GateEvent::Closed { end });
            }
        }
        None
    }
}

fn frame_samples() -> usize {
    duration_to_samples(FRAME).max(1)
}

/// Loudness of a frame in decibels relative to full scale; digital silence
/// is clamped far below any threshold instead of becoming minus infinity.
fn level_db(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return -140.0;
    }
    let mean_square = frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32;
    20.0 * mean_square.sqrt().max(1e-7).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: usize = crate::ENGINE_SAMPLE_RATE as usize;

    /// Deterministic noise at exactly `rms` per frame, so a frame's level is
    /// the same no matter how the buffer is cut.
    fn noise(seconds: f32, rms: f32) -> Vec<f32> {
        let frame = frame_samples();
        let frames = (seconds * SAMPLE_RATE as f32 / frame as f32).round() as usize;
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut samples = Vec::with_capacity(frames * frame);
        for _ in 0..frames {
            let raw: Vec<f32> = (0..frame)
                .map(|_| {
                    state = state
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
                })
                .collect();
            let raw_rms = (raw.iter().map(|x| x * x).sum::<f32>() / frame as f32).sqrt();
            samples.extend(raw.iter().map(|x| x / raw_rms * rms));
        }
        samples
    }

    fn db_to_rms(db: f32) -> f32 {
        10f32.powf(db / 20.0)
    }

    fn seconds(value: f32) -> usize {
        (value * SAMPLE_RATE as f32).round() as usize
    }

    #[test]
    fn quiet_noise_never_opens_the_gate() {
        let mut gate = SpeechGate::new();
        let events = gate.feed(&noise(5.0, db_to_rms(-60.0)));
        assert_eq!(events, Vec::new());
        assert!(!gate.is_open());
        assert_eq!(gate.speech_end(), None);
    }

    #[test]
    fn a_short_click_does_not_open_the_gate() {
        let mut signal = noise(1.0, db_to_rms(-60.0));
        signal.extend(noise(0.06, db_to_rms(-30.0)));
        signal.extend(noise(1.0, db_to_rms(-60.0)));
        let mut gate = SpeechGate::new();
        assert_eq!(gate.feed(&signal), Vec::new());
    }

    #[test]
    fn a_burst_above_the_noise_opens_the_gate_with_a_margin_before_it() {
        let mut signal = noise(1.0, db_to_rms(-60.0));
        signal.extend(noise(0.5, db_to_rms(-30.0)));
        let mut gate = SpeechGate::new();
        let events = gate.feed(&signal);
        assert_eq!(
            events,
            vec![GateEvent::Opened {
                start: seconds(1.0) - duration_to_samples(OPEN_MARGIN)
            }]
        );
        assert!(gate.is_open());
        assert_eq!(
            gate.speech_end(),
            Some(seconds(1.5)),
            "still open: the end is clamped to what was fed"
        );
    }

    #[test]
    fn a_burst_at_the_very_start_opens_at_zero() {
        let mut gate = SpeechGate::new();
        let events = gate.feed(&noise(0.5, db_to_rms(-30.0)));
        assert_eq!(events, vec![GateEvent::Opened { start: 0 }]);
    }

    #[test]
    fn the_gate_closes_only_after_clear_silence() {
        let mut gate = SpeechGate::new();
        let mut signal = noise(1.0, db_to_rms(-60.0));
        signal.extend(noise(1.0, db_to_rms(-30.0)));
        gate.feed(&signal);
        assert!(gate.is_open());

        let close_silence = CLOSE_SILENCE.as_secs_f32();
        assert_eq!(
            gate.feed(&noise(close_silence - 0.1, db_to_rms(-60.0))),
            Vec::new(),
            "a pause shorter than the closing silence keeps the gate open"
        );
        assert!(gate.is_open());

        let events = gate.feed(&noise(0.5, db_to_rms(-60.0)));
        assert_eq!(
            events,
            vec![GateEvent::Closed {
                end: seconds(2.0) + duration_to_samples(CLOSE_MARGIN)
            }]
        );
        assert!(!gate.is_open());
    }

    #[test]
    fn speech_end_follows_the_last_loud_frame_while_open() {
        let mut gate = SpeechGate::new();
        let mut signal = noise(1.0, db_to_rms(-60.0));
        signal.extend(noise(1.0, db_to_rms(-30.0)));
        signal.extend(noise(1.0, db_to_rms(-60.0)));
        gate.feed(&signal);
        assert!(gate.is_open());
        assert_eq!(
            gate.speech_end(),
            Some(seconds(2.0) + duration_to_samples(CLOSE_MARGIN))
        );
    }

    #[test]
    fn steady_loud_noise_becomes_the_floor_and_stops_counting_as_speech() {
        let mut gate = SpeechGate::new();
        let events = gate.feed(&noise(12.0, db_to_rms(-45.0)));
        assert_eq!(
            events.first(),
            Some(&GateEvent::Opened { start: 0 }),
            "at first the noise is louder than anything the gate knows"
        );
        assert!(
            matches!(events.get(1), Some(GateEvent::Closed { .. })),
            "{events:?}: once the floor has risen to the noise, the noise is silence"
        );
        assert_eq!(events.len(), 2);
        assert!(!gate.is_open());

        let quiet_burst = gate.feed(&noise(0.5, db_to_rms(-40.0)));
        assert_eq!(
            quiet_burst,
            Vec::new(),
            "a burst barely above the noise does not open the gate"
        );
        let clear_burst = gate.feed(&noise(0.5, db_to_rms(-30.0)));
        assert!(
            matches!(clear_burst.as_slice(), [GateEvent::Opened { .. }]),
            "{clear_burst:?}"
        );
    }

    #[test]
    fn feeding_in_odd_chunks_gives_the_same_events_as_one_feed() {
        let mut signal = noise(1.0, db_to_rms(-60.0));
        signal.extend(noise(1.0, db_to_rms(-30.0)));
        signal.extend(noise(4.0, db_to_rms(-60.0)));

        let mut whole = SpeechGate::new();
        let expected = whole.feed(&signal);
        assert_eq!(expected.len(), 2, "{expected:?}");

        let mut chunked = SpeechGate::new();
        let mut actual = Vec::new();
        for chunk in signal.chunks(333) {
            actual.extend(chunked.feed(chunk));
        }
        assert_eq!(actual, expected);
        assert_eq!(chunked.speech_end(), whole.speech_end());
    }
}
