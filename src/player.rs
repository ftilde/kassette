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
        }
    }

    fn sample_rate(&self) -> u64 {
        self.stream.ident_hdr.audio_sample_rate as u64
    }

    fn current_pos(&self) -> Duration {
        Duration::from_micros(
            1_000_000 * self.stream.get_last_absgp().unwrap_or(0) / self.sample_rate(),
        )
    }

    fn seek(&mut self, d: Duration) {
        let pos = d.as_micros() as u64 * self.sample_rate() / 1_000_000;
        self.stream.seek_absgp_pg(pos).unwrap();
    }

    fn next_chunk(&mut self) -> Option<Vec<i16>> {
        self.stream
            .read_dec_packet_itl()
            .unwrap()
            .map(|pck_samples| self.resampler.resample_nearest(&pck_samples))
    }
}

const MAX_VOLUME: u8 = 15;
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
    Playing(AudioSource),
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

    pub fn play_file(&mut self, file_path: impl AsRef<Path>, start_pos: Option<Duration>) {
        let mut source = AudioSource::new(file_path, self.output.sample_rate());

        if let Some(start_pos) = start_pos {
            source.seek(start_pos);
        }

        self.state = PlayerState::Playing(source);
    }

    pub fn pause(&mut self) {
        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);
        self.state = match dummy {
            PlayerState::Playing(i) => PlayerState::Paused(i),
            o => o,
        }
    }

    pub fn resume(&mut self) {
        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);
        self.state = match dummy {
            PlayerState::Paused(i) => PlayerState::Playing(i),
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

    pub fn playback_pos(&self) -> Option<Duration> {
        match self.state {
            PlayerState::Paused(ref s) | PlayerState::Playing(ref s) => Some(s.current_pos()),
            PlayerState::Idle => None,
        }
    }

    pub fn push_samples(&mut self) {
        match &mut self.state {
            PlayerState::Playing(srr) => {
                if let Some(mut pck_samples) = srr.next_chunk() {
                    for s in &mut pck_samples {
                        *s = self.volume.apply(*s);
                    }
                    if pck_samples.len() > 0 {
                        self.output.play_buf(&pck_samples);
                    }
                } else {
                    self.state = PlayerState::Idle;
                }
            }
            PlayerState::Paused(_) | PlayerState::Idle => {
                self.output.play_buf(MUTED_BUF);
            }
        }
    }
}
