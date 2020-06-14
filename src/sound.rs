pub struct AudioOutput {
    pcm: alsa::pcm::PCM,
}

impl AudioOutput {
    pub fn new() -> Self {
        for card in alsa::card::Iter::new() {
            let card = card.unwrap();
            let name = card.get_name().unwrap();
            println!(
                "Alsa card: {}, long: {}",
                name,
                card.get_longname().unwrap()
            );
        }
        let mixer = alsa::mixer::Mixer::new("hw:0", false).unwrap();
        //println!("Mixer: {:?}", mixer);
        for elm in mixer.iter() {
            //println!("MixerElm: {:?}", elm);
            let selm = alsa::mixer::Selem::new(elm).unwrap();
            //dbg!(selm.has_volume());
            //dbg!(selm.can_playback());
            //dbg!(selm.can_playback());
            //let channelid = alsa::mixer::SelemChannelId::mono();
            //dbg!(selm.get_playback_volume(channelid).unwrap());
            //dbg!(selm.get_playback_vol_db(channelid).unwrap());
            let (_, maxvol) = selm.get_playback_volume_range();
            selm.set_playback_volume_all(maxvol).unwrap();
            //dbg!(selm.get_playback_volume(channelid).unwrap());
            //dbg!(selm.get_playback_vol_db(channelid).unwrap());
        }

        use alsa::pcm::{Access, Format, HwParams, PCM};
        use alsa::{Direction, ValueOr};

        // Open default playback device
        let pcm = PCM::new("default", Direction::Playback, false).unwrap();

        // Set hardware parameters: 44100 Hz / Mono / 16 bit
        {
            // TODO: try to supporting setting this for media files?
            let hwp = HwParams::any(&pcm).unwrap();
            hwp.set_channels(2).unwrap();
            hwp.set_rate(44100, ValueOr::Nearest).unwrap();
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

        AudioOutput { pcm }
    }

    pub fn play_buf(&self, buf: &[i16]) {
        let io = self.pcm.io_i16().unwrap();

        //let pre = std::time::Instant::now();
        let num_channels = 2;
        match io.writei(&buf[..]) {
            Ok(frames) => {
                assert_eq!(frames, buf.len() / num_channels);
                //eprintln!("Write: {:?}", pre.elapsed());
            }
            Err(e) => {
                eprintln!("OI Error: {:?}", e);
                self.pcm.try_recover(e, false).unwrap();
            }
        }

        // start playing
        use alsa::pcm::State;
        if self.pcm.state() != State::Running {
            self.pcm.start().unwrap()
        };
    }

    ///// Wait for the stream to finish playback.
    //pub fn drain(&self) {
    //    self.pcm.drain().unwrap();
    //}
}
