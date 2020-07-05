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

impl AudioSource {
    fn new(file_path: impl AsRef<Path>, output_sample_rate: u64) -> Self {
        let f = std::fs::File::open(file_path).expect("Can't open file");

        // Prepare the reading
        let srr = OggStreamReader::new(f).unwrap();

        // Prepare the playback.
        println!("Sample rate: {}", srr.ident_hdr.audio_sample_rate);

        let n_channels = srr.ident_hdr.audio_channels as usize;
        assert_eq!(n_channels, 2, "We require 2 channels for now");

        let resampler = Resampler::new(srr.ident_hdr.audio_sample_rate as _, output_sample_rate);

        AudioSource {
            stream: srr,
            resampler,
            seek_pos: 0,
        }
    }

    fn sample_rate(&self) -> u64 {
        self.stream.ident_hdr.audio_sample_rate as u64
    }

    fn current_pos(&self) -> PlaybackPos {
        PlaybackPos(Duration::from_micros(
            1_000_000 * self.stream.get_last_absgp().unwrap_or(self.seek_pos) / self.sample_rate(),
        ))
    }

    fn seek(&mut self, d: PlaybackPos) {
        let pos = d.0.as_micros() as u64 * self.sample_rate() / 1_000_000;
        self.seek_pos = pos;
        self.stream.seek_absgp_pg(pos).unwrap();
    }

    fn next_chunk(&mut self) -> Option<Vec<i16>> {
        match self.stream.read_dec_packet_itl() {
            Ok(pck_samples) => {
                pck_samples.map(|pck_samples| self.resampler.resample_nearest(&pck_samples))
            }
            Err(lewton::VorbisError::BadAudio(lewton::audio::AudioReadError::AudioIsHeader)) => {
                Some(Vec::new())
            }
            Err(e) => {
                eprintln!("Error reading chunk: {}", e);
                Some(Vec::new())
            }
        }
    }
}

const MAX_VOLUME: u8 = 15;
const FADE_TIME: Duration = Duration::from_millis(500);
const DEFAULT_VOLUME: u8 = 11;

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct Volume {
    amt: u8,
}

impl Default for Volume {
    fn default() -> Self {
        Volume {
            amt: DEFAULT_VOLUME,
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
impl std::ops::Mul<Volume> for Volume {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        let reduction1 = MAX_VOLUME - self.amt;
        let reduction2 = MAX_VOLUME - other.amt;
        let reduction = reduction1 + reduction2;
        Volume::new(MAX_VOLUME.saturating_sub(reduction))
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

    pub fn load_file(&mut self, file_path: impl AsRef<Path>, start_pos: Option<PlaybackPos>) {
        let mut source = AudioSource::new(file_path, self.output.sample_rate());

        if let Some(start_pos) = start_pos {
            source.seek(start_pos);
        }

        self.state = PlayerState::Paused(source);
    }

    pub fn rewind(&mut self, time: Duration) {
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
                s.seek(seek_pos);
            }
            PlayerState::Idle => {}
        }
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

        fn fade_step(begin: PlaybackPos, current: PlaybackPos, max_step: u64) -> u64 {
            let diff = current.0 - begin.0;
            (diff.as_millis() as u64 * max_step / FADE_TIME.as_millis() as u64).min(max_step)
        }

        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);

        self.state = match dummy {
            PlayerState::FadeIn(mut srr, begin) => {
                let max_step = MAX_VOLUME as u64;
                let step = fade_step(begin, srr.current_pos(), max_step);
                let fade_vol = Volume::new(step as u8);

                if let Some(s) = play_chunk(&mut srr, &mut self.output, fade_vol * self.volume) {
                    s
                } else {
                    if step == max_step {
                        PlayerState::Playing(srr)
                    } else {
                        PlayerState::FadeIn(srr, begin)
                    }
                }
            }
            PlayerState::FadeOut(mut srr, begin) => {
                let max_step = MAX_VOLUME as u64;
                let step = fade_step(begin, srr.current_pos(), max_step);
                let fade_vol = Volume::new((max_step - step) as u8);

                if let Some(s) = play_chunk(&mut srr, &mut self.output, fade_vol * self.volume) {
                    s
                } else {
                    if step == max_step {
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
