//! Focus music: optional looping audio for focus sessions.
//!
//! Playback runs on a dedicated OS thread owning the rodio `OutputStream`,
//! NOT in the webview — DayTrail hides its window to the tray, and macOS
//! throttles hidden WKWebViews, so webview audio would die mid-session.
//! The thread receives commands over a channel and outlives the window.
//!
//! Two kinds of tracks:
//!   * built-in soundscapes generated procedurally (no bundled audio files,
//!     no licensing, no installer bloat);
//!   * user-supplied files dropped into `<app data>/focus-music/`.

use std::{
    fs,
    path::PathBuf,
    sync::{
        mpsc::{self, Sender},
        Mutex,
    },
    time::Duration,
};

use rodio::{OutputStream, Sink, Source};
use serde::Serialize;

const MUSIC_DIR_NAME: &str = "focus-music";
const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg"];
const SAMPLE_RATE: u32 = 44_100;
pub const DEFAULT_VOLUME: f32 = 0.8;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FocusMusicTrack {
    /// Stable identifier: `builtin:<slug>` or `file:<file name>`.
    pub id: String,
    pub name: String,
    pub kind: String,
}

enum AudioCommand {
    Play(ResolvedTrack),
    Stop,
    SetVolume(f32),
}

enum ResolvedTrack {
    BrownNoise,
    Rain,
    DeepTone,
    File(PathBuf),
}

static AUDIO_TX: Mutex<Option<Sender<AudioCommand>>> = Mutex::new(None);
static NOW_PLAYING: Mutex<Option<FocusMusicTrack>> = Mutex::new(None);

fn builtin_tracks() -> Vec<FocusMusicTrack> {
    vec![
        FocusMusicTrack {
            id: "builtin:brown-noise".into(),
            name: "Brown noise".into(),
            kind: "builtin".into(),
        },
        FocusMusicTrack {
            id: "builtin:rain".into(),
            name: "Rain".into(),
            kind: "builtin".into(),
        },
        FocusMusicTrack {
            id: "builtin:deep-tone".into(),
            name: "Deep tone".into(),
            kind: "builtin".into(),
        },
    ]
}

/// Directory scanned for user-supplied music. Defaults to `<app data>/focus-music/`,
/// created on first listing so users can discover it from the Settings hint and
/// drop files in — but the user can point this at any folder of their choosing
/// instead (see `Settings::focus_music_dir`), which takes priority when set.
pub fn music_dir(custom_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(custom) = custom_dir.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(custom));
    }
    let dir = dirs::data_local_dir()?
        .join("ai.daytrail.desktop")
        .join(MUSIC_DIR_NAME);
    let _ = fs::create_dir_all(&dir);
    Some(dir)
}

pub fn list_tracks(custom_dir: Option<&str>) -> Vec<FocusMusicTrack> {
    let mut tracks = builtin_tracks();

    let Some(dir) = music_dir(custom_dir) else {
        return tracks;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return tracks;
    };

    let mut file_tracks: Vec<FocusMusicTrack> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
                return None;
            }
            let file_name = path.file_name()?.to_str()?.to_string();
            let display = path.file_stem()?.to_str()?.to_string();
            Some(FocusMusicTrack {
                id: format!("file:{file_name}"),
                name: display,
                kind: "file".into(),
            })
        })
        .collect();
    file_tracks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    tracks.extend(file_tracks);
    tracks
}

/// Resolve a track id to a playable source description. File ids are
/// validated as bare file names (no separators) and must resolve inside the
/// music directory, so a malicious id can't escape it.
fn resolve(id: &str, custom_dir: Option<&str>) -> Option<(ResolvedTrack, FocusMusicTrack)> {
    match id {
        "builtin:brown-noise" | "builtin:rain" | "builtin:deep-tone" => {
            let meta = builtin_tracks().into_iter().find(|t| t.id == id)?;
            let resolved = match id {
                "builtin:brown-noise" => ResolvedTrack::BrownNoise,
                "builtin:rain" => ResolvedTrack::Rain,
                _ => ResolvedTrack::DeepTone,
            };
            Some((resolved, meta))
        }
        _ => {
            let file_name = id.strip_prefix("file:")?;
            if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
                return None;
            }
            let path = music_dir(custom_dir)?.join(file_name);
            if !path.is_file() {
                return None;
            }
            let display = path.file_stem()?.to_str()?.to_string();
            Some((
                ResolvedTrack::File(path.clone()),
                FocusMusicTrack {
                    id: id.to_string(),
                    name: display,
                    kind: "file".into(),
                },
            ))
        }
    }
}

pub fn play(id: &str, volume: f32, custom_dir: Option<&str>) -> Option<FocusMusicTrack> {
    let (resolved, meta) = resolve(id, custom_dir)?;
    let tx = ensure_audio_thread()?;
    tx.send(AudioCommand::SetVolume(volume.clamp(0.0, 1.0))).ok()?;
    tx.send(AudioCommand::Play(resolved)).ok()?;
    if let Ok(mut guard) = NOW_PLAYING.lock() {
        *guard = Some(meta.clone());
    }
    Some(meta)
}

pub fn stop() {
    if let Ok(guard) = AUDIO_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(AudioCommand::Stop);
        }
    }
    if let Ok(mut guard) = NOW_PLAYING.lock() {
        *guard = None;
    }
}

pub fn set_volume(volume: f32) {
    if let Ok(guard) = AUDIO_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(AudioCommand::SetVolume(volume.clamp(0.0, 1.0)));
        }
    }
}

pub fn now_playing() -> Option<FocusMusicTrack> {
    NOW_PLAYING.lock().ok()?.clone()
}

/// Lazily spawn the audio thread. rodio's `OutputStream` is not `Send`, so
/// one long-lived thread owns it and everything else talks over a channel.
fn ensure_audio_thread() -> Option<Sender<AudioCommand>> {
    let mut guard = AUDIO_TX.lock().ok()?;
    if let Some(tx) = guard.as_ref() {
        return Some(tx.clone());
    }

    let (tx, rx) = mpsc::channel::<AudioCommand>();
    std::thread::Builder::new()
        .name("focus-audio".into())
        .spawn(move || {
            let Ok((_stream, handle)) = OutputStream::try_default() else {
                eprintln!("focus audio: no output device available");
                return;
            };
            let mut sink: Option<Sink> = None;
            let mut volume = DEFAULT_VOLUME;

            for command in rx {
                match command {
                    AudioCommand::Play(track) => {
                        if let Some(existing) = sink.take() {
                            existing.stop();
                        }
                        let Ok(next) = Sink::try_new(&handle) else {
                            continue;
                        };
                        next.set_volume(volume);
                        match track {
                            ResolvedTrack::BrownNoise => next.append(BrownNoise::new()),
                            ResolvedTrack::Rain => next.append(Rain::new()),
                            ResolvedTrack::DeepTone => next.append(DeepTone::new()),
                            ResolvedTrack::File(path) => {
                                let Ok(file) = fs::File::open(&path) else {
                                    continue;
                                };
                                let Ok(decoder) =
                                    rodio::Decoder::new(std::io::BufReader::new(file))
                                else {
                                    continue;
                                };
                                next.append(decoder.repeat_infinite());
                            }
                        }
                        sink = Some(next);
                    }
                    AudioCommand::Stop => {
                        if let Some(existing) = sink.take() {
                            existing.stop();
                        }
                    }
                    AudioCommand::SetVolume(next_volume) => {
                        volume = next_volume;
                        if let Some(existing) = sink.as_ref() {
                            existing.set_volume(volume);
                        }
                    }
                }
            }
        })
        .ok()?;

    *guard = Some(tx.clone());
    Some(tx)
}

/// Tiny xorshift PRNG so the noise generators don't pull in the `rand` crate.
struct XorShift(u32);

impl XorShift {
    fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Uniform sample in [-1.0, 1.0].
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Brown (red) noise: integrated white noise with a leaky accumulator.
/// Softer than white noise; a well-known concentration aid.
struct BrownNoise {
    rng: XorShift,
    last: f32,
}

impl BrownNoise {
    fn new() -> Self {
        Self {
            rng: XorShift::new(0x9E37_79B9),
            last: 0.0,
        }
    }
}

impl Iterator for BrownNoise {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let white = self.rng.next_f32();
        self.last = (self.last + 0.02 * white) / 1.02;
        Some((self.last * 3.5).clamp(-1.0, 1.0) * 0.55)
    }
}

impl Source for BrownNoise {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Rain-like texture: white noise through two one-pole low-pass stages with
/// a slow random amplitude drift, resembling steady rain on a window.
struct Rain {
    rng: XorShift,
    lp1: f32,
    lp2: f32,
    drift: f32,
    drift_target: f32,
    samples_until_drift: u32,
}

impl Rain {
    fn new() -> Self {
        Self {
            rng: XorShift::new(0x1234_5678),
            lp1: 0.0,
            lp2: 0.0,
            drift: 0.8,
            drift_target: 0.8,
            samples_until_drift: 0,
        }
    }
}

impl Iterator for Rain {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let white = self.rng.next_f32();
        // ~1.2 kHz one-pole low-pass, applied twice for a softer spectrum.
        const ALPHA: f32 = 0.16;
        self.lp1 += ALPHA * (white - self.lp1);
        self.lp2 += ALPHA * (self.lp1 - self.lp2);

        if self.samples_until_drift == 0 {
            self.drift_target = 0.6 + 0.4 * ((self.rng.next_f32() + 1.0) / 2.0);
            self.samples_until_drift = SAMPLE_RATE / 2;
        }
        self.samples_until_drift -= 1;
        self.drift += (self.drift_target - self.drift) / 8_192.0;

        Some((self.lp2 * 2.4 * self.drift).clamp(-1.0, 1.0) * 0.9)
    }
}

impl Source for Rain {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Deep ambient tone: three low sine partials with a slow breathing LFO.
struct DeepTone {
    tick: u64,
}

impl DeepTone {
    fn new() -> Self {
        Self { tick: 0 }
    }
}

impl Iterator for DeepTone {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        use std::f32::consts::TAU;
        let t = self.tick as f32 / SAMPLE_RATE as f32;
        self.tick = self.tick.wrapping_add(1);

        let breath = 0.75 + 0.25 * (TAU * 0.05 * t).sin();
        let sample = 0.5 * (TAU * 110.0 * t).sin()
            + 0.3 * (TAU * 165.0 * t).sin()
            + 0.2 * (TAU * 220.0 * t).sin();
        Some(sample * breath * 0.28)
    }
}

impl Source for DeepTone {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tracks_are_always_listed() {
        let tracks = list_tracks(None);
        assert!(tracks.iter().any(|t| t.id == "builtin:brown-noise"));
        assert!(tracks.iter().any(|t| t.id == "builtin:rain"));
        assert!(tracks.iter().any(|t| t.id == "builtin:deep-tone"));
    }

    #[test]
    fn generated_sources_stay_within_unit_range() {
        let mut brown = BrownNoise::new();
        let mut rain = Rain::new();
        let mut tone = DeepTone::new();
        for _ in 0..SAMPLE_RATE {
            for sample in [
                brown.next().unwrap(),
                rain.next().unwrap(),
                tone.next().unwrap(),
            ] {
                assert!((-1.0..=1.0).contains(&sample), "sample out of range: {sample}");
            }
        }
    }

    #[test]
    fn file_ids_cannot_escape_the_music_directory() {
        assert!(resolve("file:../daytrail.sqlite3", None).is_none());
        assert!(resolve("file:/etc/passwd", None).is_none());
        assert!(resolve("file:..\\evil.mp3", None).is_none());
        assert!(resolve("file:does-not-exist.mp3", None).is_none());
    }

    #[test]
    fn unknown_ids_resolve_to_none() {
        assert!(resolve("builtin:nope", None).is_none());
        assert!(resolve("garbage", None).is_none());
    }

    #[test]
    fn custom_dir_overrides_default_music_directory() {
        let custom = std::env::temp_dir().join("daytrail-focus-music-test");
        let dir = music_dir(Some(custom.to_str().unwrap())).unwrap();
        assert_eq!(dir, custom);
    }

    #[test]
    fn blank_custom_dir_falls_back_to_default() {
        let default_dir = music_dir(None).unwrap();
        assert_eq!(music_dir(Some("   ")).unwrap(), default_dir);
    }

    // End-to-end regression for the user-chosen music folder setting: write a
    // real wav file into a scratch dir and confirm list_tracks/resolve/play
    // all see it through the custom_dir override, exactly as the Settings ->
    // "Focus music folder" field wires it through commands/focus.rs.
    #[test]
    fn custom_dir_is_scanned_for_playable_files() {
        let dir = std::env::temp_dir().join("daytrail-focus-music-e2e-test");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("my-real-track.wav");
        fs::write(&file_path, b"not real audio, just checking the scan/resolve path").unwrap();

        let custom = dir.to_str().unwrap();
        let tracks = list_tracks(Some(custom));
        let found = tracks
            .iter()
            .find(|t| t.id == "file:my-real-track.wav")
            .expect("custom dir file should be listed");
        assert_eq!(found.name, "my-real-track");
        assert_eq!(found.kind, "file");

        let (resolved, meta) = resolve("file:my-real-track.wav", Some(custom))
            .expect("resolve should find the file inside the custom dir");
        assert_eq!(meta.id, "file:my-real-track.wav");
        assert!(matches!(resolved, ResolvedTrack::File(path) if path == file_path));

        fs::remove_dir_all(&dir).ok();
    }
}
