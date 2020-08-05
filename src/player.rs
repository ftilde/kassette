use miniserde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

struct Resampler {
    source_sample_counter: u64,
    source_sample_rate: u64,
    sink_sample_counter: u64,
    sink_sample_rate: u64,
}

impl Resampler {
    fn new(source_sample_rate: u64, sink_sample_rate: u64) -> Self {
        Resampler {
            source_sample_counter: 0,
            source_sample_rate,
            sink_sample_counter: 0,
            sink_sample_rate,
        }
    }

    fn resample_nearest(&mut self, input: &[i16]) -> Vec<i16> {
        assert!(input.len() % 2 == 0, "Only two channels supported");
        let mut output = Vec::new();
        for slice in input.chunks(2) {
            let l = slice[0];
            let r = slice[1];

            self.source_sample_counter += 1;

            let new_sink_sample_counter = self.source_sample_counter * self.sink_sample_rate as u64
                / self.source_sample_rate as u64;

            for _ in 0..(new_sink_sample_counter - self.sink_sample_counter) {
                output.push(l);
                output.push(r);
            }
            self.sink_sample_counter = new_sink_sample_counter;
        }
        output
    }
}

use lewton::inside_ogg::OggStreamReader;

const MUTED_BUF: &[i16] = &[0; 1024];

struct AudioSource {
    stream: OggStreamReader<std::fs::File>,
    resampler: Resampler,
    seek_pos: u64,
    current_pos: u64,
}

#[derive(Copy, Clone, Debug)]
pub struct PlaybackPos(Duration);

impl PlaybackPos {
    pub fn from_millis(millis: u64) -> Self {
        Self(Duration::from_millis(millis))
    }

    pub fn as_millis(&self) -> u64 {
        self.0.as_millis() as _
    }
}

#[derive(Debug)]
pub enum AudioSourceError {
    Vorbis(lewton::VorbisError),
    Io(std::io::Error),
}

impl AudioSource {
    fn new(file_path: impl AsRef<Path>, output_sample_rate: u64) -> Result<Self, AudioSourceError> {
        let f = std::fs::File::open(file_path).map_err(AudioSourceError::Io)?;

        // Prepare the reading
        let srr = OggStreamReader::new(f).map_err(AudioSourceError::Vorbis)?;

        // Prepare the playback.
        let n_channels = srr.ident_hdr.audio_channels as usize;
        assert_eq!(n_channels, 2, "We require 2 channels for now");

        let resampler = Resampler::new(srr.ident_hdr.audio_sample_rate as _, output_sample_rate);

        Ok(AudioSource {
            stream: srr,
            resampler,
            seek_pos: 0,
            current_pos: 0,
        })
    }

    fn sample_rate(&self) -> u64 {
        self.stream.ident_hdr.audio_sample_rate as u64
    }

    fn current_pos(&self) -> PlaybackPos {
        PlaybackPos(Duration::from_micros(
            1_000_000 * self.current_pos / self.sample_rate(),
        ))
    }

    fn seek(&mut self, d: PlaybackPos) -> Result<(), AudioSourceError> {
        let pos = d.0.as_micros() as u64 * self.sample_rate() / 1_000_000;
        self.seek_pos = pos;
        self.stream
            .seek_absgp_pg(pos)
            .map_err(AudioSourceError::Vorbis)?;
        self.current_pos = pos;
        Ok(())
    }

    fn next_chunk(&mut self) -> Option<Vec<i16>> {
        match self.stream.read_dec_packet_itl() {
            Ok(Some(pck_samples)) => {
                self.current_pos += pck_samples.len() as u64 / 2;
                Some(self.resampler.resample_nearest(&pck_samples))
            }
            Ok(None) => None,
            Err(lewton::VorbisError::BadAudio(lewton::audio::AudioReadError::AudioIsHeader)) => {
                Some(Vec::new())
            }
            Err(e) => {
                log!("Error reading chunk: {}", e);
                Some(Vec::new())
            }
        }
    }
}

const MAX_VOLUME: u8 = 15;

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct Volume {
    amt: u8,
}

impl Default for Volume {
    fn default() -> Self {
        Volume {
            amt: crate::config::DEFAULT_VOLUME,
        }
    }
}

impl Volume {
    fn new(amt: u8) -> Self {
        assert!(amt <= MAX_VOLUME);
        Volume { amt }
    }
    fn apply(&self, i: i16) -> i16 {
        i >> (MAX_VOLUME.saturating_sub(self.amt))
    }
}

impl std::ops::AddAssign<u8> for Volume {
    fn add_assign(&mut self, other: u8) {
        self.amt = (self.amt + other).min(MAX_VOLUME);
    }
}
impl std::ops::SubAssign<u8> for Volume {
    fn sub_assign(&mut self, other: u8) {
        self.amt = self.amt.saturating_sub(other);
    }
}

enum PlayerState {
    FadeIn(AudioSource, PlaybackPos),
    Playing(AudioSource),
    FadeOut(AudioSource, PlaybackPos),
    Paused(AudioSource),
    Idle,
}

pub struct Player {
    output: crate::sound::AudioOutput,
    state: PlayerState,
    volume: Volume,
}

impl Player {
    pub fn new(output: crate::sound::AudioOutput, volume: Volume) -> Self {
        Player {
            output,
            state: PlayerState::Idle,
            volume,
        }
    }
    pub fn volume(&mut self) -> &mut Volume {
        &mut self.volume
    }

    pub fn load_file(
        &mut self,
        file_path: impl AsRef<Path>,
        start_pos: Option<PlaybackPos>,
    ) -> Result<(), AudioSourceError> {
        let mut source = AudioSource::new(file_path, self.output.sample_rate())?;

        if let Some(start_pos) = start_pos {
            source.seek(start_pos)?;
        }

        self.state = PlayerState::Paused(source);
        Ok(())
    }

    pub fn rewind(&mut self, time: Duration) -> Result<(), AudioSourceError> {
        match self.state {
            PlayerState::Paused(ref mut s)
            | PlayerState::FadeOut(ref mut s, _)
            | PlayerState::Playing(ref mut s)
            | PlayerState::FadeIn(ref mut s, _) => {
                let seek_pos = PlaybackPos(
                    s.current_pos()
                        .0
                        .checked_sub(time)
                        .unwrap_or(Duration::from_millis(0)),
                );
                s.seek(seek_pos)?;
            }
            PlayerState::Idle => {}
        }
        Ok(())
    }

    pub fn pause(&mut self) {
        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);
        self.state = match dummy {
            PlayerState::Playing(i) | PlayerState::FadeIn(i, _) => {
                let pos = i.current_pos();
                PlayerState::FadeOut(i, pos)
            }
            o => o,
        }
    }

    pub fn play(&mut self) {
        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);
        self.state = match dummy {
            PlayerState::Paused(i) => {
                let pos = i.current_pos();
                PlayerState::FadeIn(i, pos)
            }
            PlayerState::FadeOut(i, pos) => PlayerState::FadeIn(i, pos),
            o => o,
        }
    }

    pub fn idle(&self) -> bool {
        if let PlayerState::Idle = self.state {
            true
        } else {
            false
        }
    }
    pub fn playing(&self) -> bool {
        match self.state {
            PlayerState::FadeOut(_, _) | PlayerState::FadeIn(_, _) | PlayerState::Playing(_) => {
                true
            }
            _ => false,
        }
    }

    pub fn playback_pos(&self) -> Option<PlaybackPos> {
        match self.state {
            PlayerState::Paused(ref s)
            | PlayerState::FadeOut(ref s, _)
            | PlayerState::Playing(ref s)
            | PlayerState::FadeIn(ref s, _) => Some(s.current_pos()),
            PlayerState::Idle => None,
        }
    }

    pub fn push_samples(&mut self) {
        fn play_chunk(
            srr: &mut AudioSource,
            output: &mut crate::sound::AudioOutput,
            volume: Volume,
        ) -> Option<PlayerState> {
            if let Some(mut pck_samples) = srr.next_chunk() {
                for s in &mut pck_samples {
                    *s = volume.apply(*s);
                }
                if pck_samples.len() > 0 {
                    output.play_buf(&pck_samples);
                }
                None
            } else {
                Some(PlayerState::Idle)
            }
        }

        fn fade_factor(begin: PlaybackPos, current: PlaybackPos) -> f32 {
            let diff = current
                .0
                .checked_sub(begin.0)
                .unwrap_or(Duration::from_millis(0));
            (diff.as_millis() as f32 / crate::config::FADE_TIME.as_millis() as f32).min(1.0)
        }

        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);

        self.state = match dummy {
            PlayerState::FadeIn(mut srr, begin) => {
                let factor = fade_factor(begin, srr.current_pos());
                let fade_vol = Volume::new((self.volume.amt as f32 * factor).round() as u8);

                if let Some(s) = play_chunk(&mut srr, &mut self.output, fade_vol) {
                    s
                } else {
                    if factor >= 1.0 {
                        PlayerState::Playing(srr)
                    } else {
                        PlayerState::FadeIn(srr, begin)
                    }
                }
            }
            PlayerState::FadeOut(mut srr, begin) => {
                let factor = fade_factor(begin, srr.current_pos());
                let fade_vol = Volume::new((self.volume.amt as f32 * (1.0 - factor)).round() as u8);

                if let Some(s) = play_chunk(&mut srr, &mut self.output, fade_vol) {
                    s
                } else {
                    if factor >= 1.0 {
                        PlayerState::Paused(srr)
                    } else {
                        PlayerState::FadeOut(srr, begin)
                    }
                }
            }
            PlayerState::Playing(mut srr) => {
                if let Some(s) = play_chunk(&mut srr, &mut self.output, self.volume) {
                    s
                } else {
                    PlayerState::Playing(srr)
                }
            }
            s @ PlayerState::Paused(_) | s @ PlayerState::Idle => {
                self.output.play_buf(MUTED_BUF);
                s
            }
        }
    }
}
