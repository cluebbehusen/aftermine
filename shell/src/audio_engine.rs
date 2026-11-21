//! Playback through kira: every clip decoded once at load, one-shots fired
//! and forgotten, loops held so their volume can follow the mix. The score's
//! sections are decoded as they come due and start on the audio clock.
//!
//! Clips play into three buses, music, effects and interface, each at its
//! settings volume. The effects bus glues stacked reports with a compressor,
//! and a limiter on the master keeps the sum under full scale. Settings
//! volumes apply after that processing, so turning a bus down never changes
//! how it is compressed.
//!
//! Without an output device every call is silent rather than an error, so
//! automation and machines without sound still run.

mod dynamics;

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use kira::clock::{ClockHandle, ClockSpeed, ClockTime};
use kira::effect::eq_filter::{EqFilterBuilder, EqFilterKind};
use kira::effect::filter::{FilterBuilder, FilterMode};
use kira::effect::reverb::ReverbBuilder;
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::track::{MainTrackBuilder, SendTrackBuilder, SendTrackHandle, TrackBuilder, TrackHandle};
use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Frame, Mix, Panning,
    PlaybackRate, StartTime, Tween,
};

/// Sounds playing at once on the effects bus. A big fight fires dozens of
/// one-shots per second beside up to 16 motor loops; past this, new clips are
/// dropped.
const EFFECTS_CAPACITY: usize = 256;
/// Score sections on one of its tracks at once: the one playing, the tail
/// of the one before and the next, waiting for its start.
const SCORE_CAPACITY: usize = 8;
/// The score's clock rate: one tick per frame of its 44.1 kHz music.
pub(crate) const SCORE_RATE: u32 = 44_100;
const INTERFACE_CAPACITY: usize = 32;
/// The music's energy below this does nothing on tablet speakers and only
/// pushes the limiter.
const MUSIC_HIGH_PASS_HZ: f64 = 40.0;
/// A gentle cut to the music's top, which sets it back behind the battle.
const MUSIC_SHELF_HZ: f64 = 2500.0;
const MUSIC_SHELF_DB: f32 = -4.0;
/// A shelf's slope: a Butterworth-like corner with no bump.
const SHELF_Q: f64 = 0.7;
/// The master's ceiling, below full scale for the overshoot between samples.
const CEILING_DB: f32 = -1.0;

/// One step of distance on the effects bus: how much quieter and duller a
/// sound plays and how much of it reaches the shared room.
struct DistanceBand {
    /// The direct sound's level.
    level_db: f32,
    /// The low-pass corner, or `None` to keep the whole clip.
    cutoff_hz: Option<f64>,
    /// The send into the room, relative to the direct sound. The room
    /// returns a clip 5 to 9 dB below what it is sent.
    room_db: f32,
}

/// From beside the listener to as far off as the camera hears. Farther
/// sounds play quieter and duller, with more of the room against less of
/// the direct sound.
const DISTANCE_BANDS: [DistanceBand; 4] = [
    DistanceBand {
        level_db: 0.0,
        cutoff_hz: None,
        room_db: -10.0,
    },
    DistanceBand {
        level_db: -2.0,
        cutoff_hz: Some(7000.0),
        room_db: -3.0,
    },
    DistanceBand {
        level_db: -4.0,
        cutoff_hz: Some(4500.0),
        room_db: 0.0,
    },
    DistanceBand {
        level_db: -6.0,
        cutoff_hz: Some(3000.0),
        room_db: 2.0,
    },
];
/// Butterworth damping in kira's filter, so the corner adds no peak.
const FLAT_RESONANCE: f64 = 0.3;
/// The room's decay and how fast its top dies, for a tail of about 0.4 s.
const ROOM_FEEDBACK: f64 = 0.6;
const ROOM_DAMPING: f64 = 0.6;

/// Which settings bus a clip plays through. The music has its own path,
/// through [`play_score_layer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bus {
    /// Everything the battlefield makes.
    Effects,
    /// Chrome sounds.
    Ui,
}

/// Which of the score's tracks a section layer plays through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScoreTrack {
    /// A match song's calm layer.
    Calm,
    /// A match song's fight layer, in step with the calm one.
    Fight,
    /// The menu theme and the endings, which have one layer.
    Solo,
}

/// The score's tracks, children of the music bus, and the clock its
/// sections start on.
struct Score {
    calm: TrackHandle,
    fight: TrackHandle,
    solo: TrackHandle,
    clock: ClockHandle,
    volumes: Option<[f32; 3]>,
}

struct Engine {
    manager: AudioManager<DefaultBackend>,
    music: TrackHandle,
    score: Score,
    effects: TrackHandle,
    /// Children of the effects bus, one per entry of `DISTANCE_BANDS`.
    distance: Vec<TrackHandle>,
    /// The shared room the distance bands send into. It returns to the master
    /// beside the effects bus, so it follows the effects volume itself.
    room: SendTrackHandle,
    ui: TrackHandle,
    volumes: Option<crate::config::Volumes>,
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

fn open() -> Result<Engine> {
    let mut main = MainTrackBuilder::new();
    main.add_effect(dynamics::LimiterBuilder {
        ceiling: Decibels(CEILING_DB).as_amplitude(),
        lookahead: Duration::from_millis(5),
        release: Duration::from_millis(150),
    });
    let settings = AudioManagerSettings {
        main_track_builder: main,
        ..AudioManagerSettings::default()
    };
    let mut manager = AudioManager::<DefaultBackend>::new(settings)?;
    let mut music = manager.add_sub_track(
        TrackBuilder::new()
            .sound_capacity(SCORE_CAPACITY)
            .with_effect(
                FilterBuilder::new()
                    .mode(FilterMode::HighPass)
                    .cutoff(MUSIC_HIGH_PASS_HZ),
            )
            .with_effect(EqFilterBuilder::new(
                EqFilterKind::HighShelf,
                MUSIC_SHELF_HZ,
                Decibels(MUSIC_SHELF_DB),
                SHELF_Q,
            )),
    )?;
    let room = manager.add_send_track(
        SendTrackBuilder::new().with_effect(
            ReverbBuilder::new()
                .feedback(ROOM_FEEDBACK)
                .damping(ROOM_DAMPING)
                .stereo_width(1.0)
                .mix(Mix::WET),
        ),
    )?;
    let mut effects = manager.add_sub_track(
        TrackBuilder::new()
            .sound_capacity(EFFECTS_CAPACITY)
            .with_effect(dynamics::CompressorBuilder {
                threshold_db: -18.0,
                ratio: 2.5,
                knee_db: 6.0,
                attack: Duration::from_millis(5),
                release: Duration::from_millis(150),
            }),
    )?;
    let distance = DISTANCE_BANDS
        .iter()
        .map(|band| {
            let mut builder = TrackBuilder::new()
                .sound_capacity(EFFECTS_CAPACITY)
                .volume(Decibels(band.level_db))
                .with_send(&room, Decibels(band.room_db));
            if let Some(cutoff) = band.cutoff_hz {
                builder = builder.with_effect(
                    FilterBuilder::new()
                        .mode(FilterMode::LowPass)
                        .cutoff(cutoff)
                        .resonance(FLAT_RESONANCE),
                );
            }
            effects.add_sub_track(builder)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ui = manager.add_sub_track(TrackBuilder::new().sound_capacity(INTERFACE_CAPACITY))?;
    let mut score_track =
        || music.add_sub_track(TrackBuilder::new().sound_capacity(SCORE_CAPACITY));
    let (calm, fight, solo) = (score_track()?, score_track()?, score_track()?);
    let mut clock = manager.add_clock(ClockSpeed::TicksPerSecond(f64::from(SCORE_RATE)))?;
    clock.start();
    Ok(Engine {
        score: Score {
            calm,
            fight,
            solo,
            clock,
            volumes: None,
        },
        manager,
        music,
        effects,
        distance,
        room,
        ui,
        volumes: None,
    })
}

/// Opens the output device, once. Call it before the first clip plays.
pub(crate) fn start() {
    ENGINE.with_borrow_mut(|engine| {
        if engine.is_some() {
            return;
        }
        aftermine_apple_bridge::application::mix_audio_with_other_apps();
        match open() {
            Ok(opened) => *engine = Some(opened),
            Err(error) => eprintln!("no sound: {error}"),
        }
    });
}

/// Sets each bus to its settings volume.
pub(crate) fn set_volumes(volumes: crate::config::Volumes) {
    let volumes = volumes.clamped();
    ENGINE.with_borrow_mut(|engine| {
        let Some(engine) = engine else {
            return;
        };
        if engine.volumes == Some(volumes) {
            return;
        }
        engine.volumes = Some(volumes);
        let tween = Tween::default();
        engine
            .manager
            .main_track()
            .set_volume(decibels(volumes.master), tween);
        engine.music.set_volume(decibels(volumes.music), tween);
        engine.effects.set_volume(decibels(volumes.effects), tween);
        engine.room.set_volume(decibels(volumes.effects), tween);
        engine.ui.set_volume(decibels(volumes.ui), tween);
    });
}

/// One decoded clip and the loops it is playing.
pub(crate) struct Sound {
    data: StaticSoundData,
    loops: RefCell<Vec<StaticSoundHandle>>,
}

impl Sound {
    /// Decodes a 16-bit PCM WAV file, mono or stereo.
    pub(crate) fn from_wav(bytes: &[u8]) -> Result<Self> {
        let (sample_rate, frames) = decode_wav(bytes)?;
        Ok(Self {
            data: StaticSoundData {
                sample_rate,
                frames: frames.into(),
                settings: StaticSoundSettings::default(),
                slice: None,
            },
            loops: RefCell::new(Vec::new()),
        })
    }

    /// Another voice of the same clip, sharing its decoded samples. Stopping
    /// a voice stops only the loops that voice plays.
    pub(crate) fn voice(&self) -> Self {
        Self {
            data: self.data.clone(),
            loops: RefCell::new(Vec::new()),
        }
    }
}

/// How one playback starts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaySoundParams {
    pub bus: Bus,
    pub looped: bool,
    /// Linear gain, where 1 plays the clip as recorded.
    pub volume: f32,
    /// From -1, the left speaker only, to 1, the right.
    pub pan: f32,
    /// Speed and pitch together, where 1 plays the clip as recorded.
    pub rate: f32,
    /// How far off an effect sits, from 0 to 1, or `None` to play it as
    /// recorded.
    pub remoteness: Option<f32>,
}

impl PlaySoundParams {
    /// A centered playback at the recorded pitch.
    pub(crate) fn new(bus: Bus, looped: bool, volume: f32) -> Self {
        Self {
            bus,
            looped,
            volume,
            pan: 0.0,
            rate: 1.0,
            remoteness: None,
        }
    }
}

fn distance_band(remoteness: f32) -> usize {
    let bands = DISTANCE_BANDS.len();
    crate::numeric::to_usize((remoteness.clamp(0.0, 1.0) * bands as f32).floor()).min(bands - 1)
}

/// The share of its level a sound keeps in the distance band for
/// `remoteness`, before the room.
pub(crate) fn distance_level(remoteness: f32) -> f32 {
    10f32.powf(DISTANCE_BANDS[distance_band(remoteness)].level_db / 20.0)
}

fn decibels(volume: f32) -> Decibels {
    if volume > 0.0 {
        Decibels((20.0 * volume.log10()).max(Decibels::SILENCE.0))
    } else {
        Decibels::SILENCE
    }
}

/// Starts a playback of `sound`. A one-shot runs to its end on its own; a
/// loop runs until [`stop_sound`].
pub(crate) fn play_sound(sound: &Sound, params: PlaySoundParams) {
    ENGINE.with_borrow_mut(|engine| {
        let Some(engine) = engine else {
            return;
        };
        let mut data = sound
            .data
            .volume(decibels(params.volume))
            .panning(Panning(params.pan.clamp(-1.0, 1.0)))
            .playback_rate(PlaybackRate(f64::from(params.rate.clamp(0.5, 2.0))));
        if params.looped {
            data = data.loop_region(..);
        }
        let track = match (params.bus, params.remoteness) {
            (Bus::Effects, None) => &mut engine.effects,
            (Bus::Effects, Some(remoteness)) => &mut engine.distance[distance_band(remoteness)],
            (Bus::Ui, _) => &mut engine.ui,
        };
        let Ok(handle) = track.play(data) else {
            return;
        };
        if params.looped {
            let mut loops = sound.loops.borrow_mut();
            loops.retain(|playing| playing.state() != PlaybackState::Stopped);
            loops.push(handle);
        }
    });
}

/// Sets the gain of every loop `sound` is playing.
pub(crate) fn set_sound_volume(sound: &Sound, volume: f32) {
    for playing in sound.loops.borrow_mut().iter_mut() {
        playing.set_volume(decibels(volume), Tween::default());
    }
}

/// Stops every loop `sound` is playing.
pub(crate) fn stop_sound(sound: &Sound) {
    for mut playing in sound.loops.borrow_mut().drain(..) {
        playing.stop(Tween::default());
    }
}

/// The score clock's tick, or `None` without an output device.
pub(crate) fn score_tick() -> Option<u64> {
    ENGINE.with_borrow(|engine| {
        engine
            .as_ref()
            .map(|engine| engine.score.clock.time().ticks)
    })
}

/// Sets the score tracks' gains: the calm and fight layers and the
/// single-layer pieces.
pub(crate) fn set_score_volumes(calm: f32, fight: f32, solo: f32) {
    ENGINE.with_borrow_mut(|engine| {
        let Some(engine) = engine else {
            return;
        };
        let score = &mut engine.score;
        let volumes = [calm, fight, solo];
        if score.volumes == Some(volumes) {
            return;
        }
        score.volumes = Some(volumes);
        let tween = Tween {
            duration: Duration::from_millis(50),
            ..Tween::default()
        };
        score.calm.set_volume(decibels(calm), tween);
        score.fight.set_volume(decibels(fight), tween);
        score.solo.set_volume(decibels(solo), tween);
    });
}

/// One layer of a score section as it plays.
pub(crate) struct ScoreVoice(StaticSoundHandle);

impl ScoreVoice {
    /// Whether it has played out or been faded away.
    pub(crate) fn finished(&self) -> bool {
        self.0.state() == PlaybackState::Stopped
    }

    /// Fades it out and lets it go.
    pub(crate) fn fade_out(&mut self, over: Duration) {
        self.0.stop(Tween {
            duration: over,
            ..Tween::default()
        });
    }
}

/// Starts one layer of a score section when the score clock reaches `tick`,
/// at once if it already has.
pub(crate) fn play_score_layer(
    track: ScoreTrack,
    frames: Arc<[Frame]>,
    gain_db: f32,
    tick: u64,
) -> Option<ScoreVoice> {
    ENGINE.with_borrow_mut(|engine| {
        let engine = engine.as_mut()?;
        let score = &mut engine.score;
        let data = StaticSoundData {
            sample_rate: SCORE_RATE,
            frames,
            settings: StaticSoundSettings::default(),
            slice: None,
        }
        .volume(Decibels(gain_db))
        .start_time(StartTime::ClockTime(ClockTime {
            clock: score.clock.id(),
            ticks: tick,
            fraction: 0.0,
        }));
        let track = match track {
            ScoreTrack::Calm => &mut score.calm,
            ScoreTrack::Fight => &mut score.fight,
            ScoreTrack::Solo => &mut score.solo,
        };
        track.play(data).ok().map(ScoreVoice)
    })
}

/// Decodes an Ogg Vorbis file at the score's rate.
pub(crate) fn decode_ogg(bytes: &[u8]) -> Result<Arc<[Frame]>> {
    let data = StaticSoundData::from_cursor(std::io::Cursor::new(bytes))?;
    ensure!(
        data.sample_rate == SCORE_RATE,
        "expected music at {SCORE_RATE} Hz"
    );
    Ok(data.frames)
}

fn decode_wav(bytes: &[u8]) -> Result<(u32, Vec<Frame>)> {
    ensure!(
        bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "not a WAV file"
    );
    let mut format = None;
    let mut data = None;
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let length = usize::try_from(u32::from_le_bytes(header[4..].try_into()?))?;
        let body = bytes
            .get(at + 8..at + 8 + length)
            .context("a WAV chunk runs past the end of the file")?;
        match &header[..4] {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + length + length % 2;
    }
    let format = format.context("a WAV file without a format chunk")?;
    let data = data.context("a WAV file without a data chunk")?;
    ensure!(format.len() >= 16, "a short WAV format chunk");
    let word = |offset: usize| u16::from_le_bytes([format[offset], format[offset + 1]]);
    let (encoding, channels, bits) = (word(0), word(2), word(14));
    let sample_rate = u32::from_le_bytes(format[4..8].try_into()?);
    ensure!(
        encoding == 1 && bits == 16 && matches!(channels, 1 | 2),
        "expected 16-bit PCM in one or two channels"
    );
    let samples: Vec<f32> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&sample| f32::from(i16::from_le_bytes(sample)) / 32768.0)
        .collect();
    let frames = if channels == 1 {
        samples.into_iter().map(Frame::from_mono).collect()
    } else {
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[left, right]| Frame::new(left, right))
            .collect()
    };
    Ok((sample_rate, frames))
}

#[cfg(test)]
mod tests;
