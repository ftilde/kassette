use std::time::{Duration, Instant};

pub struct AudioOutput {
    pcm: alsa::pcm::PCM,
    current_sample_num: u64,
    start_time: Instant,
    sample_rate: u64,
}

impl AudioOutput {
    pub fn new() -> Self {
        for card in alsa::card::Iter::new() {
            let card = card.unwrap();
            let name = card.get_name().unwrap();
            eprintln!(
                "Alsa card: {}, long: {}",
                name,
                card.get_longname().unwrap()
            );
        }
        let mixer = alsa::mixer::Mixer::new("hw:0", false).unwrap();
        for elm in mixer.iter() {
            let selm = alsa::mixer::Selem::new(elm).unwrap();
            let (_, maxvol) = selm.get_playback_volume_range();
            selm.set_playback_volume_all(maxvol).unwrap();
        }

        use alsa::pcm::{Access, Format, HwParams, PCM};
        use alsa::{Direction, ValueOr};

        // Open default playback device
        let pcm = PCM::new("default", Direction::Playback, false).unwrap();

        let sample_rate = 44100;

        // Set hardware parameters: 44100 Hz / Mono / 16 bit
        {
            // TODO: try to supporting setting this for media files?
            let hwp = HwParams::any(&pcm).unwrap();
            hwp.set_channels(2).unwrap();
            hwp.set_rate(sample_rate, ValueOr::Nearest).unwrap();
            hwp.set_format(Format::s16()).unwrap();
            hwp.set_access(Access::RWInterleaved).unwrap();
            pcm.hw_params(&hwp).unwrap();
        }

        // Make sure we don't start the stream too early
        {
            let hwp = pcm.hw_params_current().unwrap();
            let swp = pcm.sw_params_current().unwrap();
            swp.set_start_threshold(
                hwp.get_buffer_size().unwrap() - hwp.get_period_size().unwrap(),
            )
            .unwrap();
            pcm.sw_params(&swp).unwrap();
        }

        AudioOutput {
            pcm,
            current_sample_num: 0,
            start_time: Instant::now(),
            sample_rate: sample_rate as _,
        }
    }

    pub fn sample_rate(&self) -> u64 {
        self.sample_rate
    }

    fn recover(&mut self, e: alsa::Error) {
        log!("Trying to recover from error: {:?}", e);
        self.pcm.try_recover(e, false).unwrap(); // Not sure what to do after RECOVERY (!) fails...
        self.current_sample_num = 0;
        self.start_time = Instant::now();
    }

    pub fn play_buf(&mut self, buf: &[i16]) {
        let write_res = {
            let io = self.pcm.io_i16().unwrap(); // Not sure what to do if this fails...
            io.writei(&buf[..])
        };

        //let pre = std::time::Instant::now();
        let num_channels = 2;
        match write_res {
            Ok(frames) => {
                assert_eq!(frames, buf.len() / num_channels);
            }
            Err(e) => self.recover(e),
        }

        // start playing
        use alsa::pcm::State;
        if self.pcm.state() != State::Running {
            if let Err(e) = self.pcm.start() {
                self.recover(e);
            }
        };

        self.current_sample_num += (buf.len() / num_channels) as u64;

        let sample_buffer_time =
            Duration::from_micros(self.current_sample_num * 1_000_000 / self.sample_rate);
        let run_time = self.start_time.elapsed();

        if let Some(sleep_time) =
            sample_buffer_time.checked_sub(run_time + crate::config::AUDIO_BUF_SIZE)
        {
            std::thread::sleep(sleep_time);
        }
    }
}
