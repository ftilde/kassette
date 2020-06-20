use std::path::Path;

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

const MAX_VOLUME: usize = 16;
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

    fn next_chunk(&mut self) -> Option<Vec<i16>> {
        self.stream
            .read_dec_packet_itl()
            .unwrap()
            .map(|pck_samples| self.resampler.resample_nearest(&pck_samples))
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
    volume: usize,
}

impl Player {
    pub fn new(output: crate::sound::AudioOutput, volume: usize) -> Self {
        Player {
            output,
            state: PlayerState::Idle,
            volume,
        }
    }
    pub fn increase_volume(&mut self) {
        self.volume += 1;
        self.volume = self.volume.min(MAX_VOLUME);
    }

    pub fn decrease_volume(&mut self) {
        self.volume = self.volume.saturating_sub(1);
    }

    pub fn play_file(&mut self, file_path: impl AsRef<Path>) {
        let source = AudioSource::new(file_path, self.output.sample_rate());

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

    pub fn push_samples(&mut self) {
        match &mut self.state {
            PlayerState::Playing(srr) => {
                if let Some(mut pck_samples) = srr.next_chunk() {
                    for s in &mut pck_samples {
                        *s = *s >> (MAX_VOLUME - self.volume);
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
