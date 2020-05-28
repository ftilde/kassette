use rfid_rs;
use rppal::gpio::{Gpio, InputPin};
use spidev;

use std::path::Path;
use std::time::Duration;

/// Mounting stuff etc.
fn general_setup() {
    let is_init = std::process::id() == 1;

    if is_init {
        println!("Running as init.");
        nix::mount::mount::<str, str, str, str>(
            None,
            "/dev",
            Some("devtmpfs"),
            nix::mount::MsFlags::empty(),
            None,
        )
        .unwrap();
        nix::mount::mount::<str, str, str, str>(
            None,
            "/proc",
            Some("proc"),
            nix::mount::MsFlags::empty(),
            None,
        )
        .unwrap();
    } else {
        println!("Running as regular process.");
    }
}

struct AudioOutput {
    pcm: alsa::pcm::PCM,
}

impl AudioOutput {
    fn new() -> Self {
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

    fn play_buf(&self, buf: &[i16]) {
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

        //use alsa::pcm::State;
        //if self.pcm.state() != State::Running {
        //    self.pcm.start().unwrap()
        //};
    }

    /// Wait for the stream to finish playback.
    fn drain(&self) {
        self.pcm.drain().unwrap();
    }
}

use lewton::inside_ogg::OggStreamReader;

fn play_file(file_path: impl AsRef<Path>) {
    let f = std::fs::File::open(file_path).expect("Can't open file");

    // Prepare the reading
    let mut srr = OggStreamReader::new(f).unwrap();

    // Prepare the playback.
    println!("Sample rate: {}", srr.ident_hdr.audio_sample_rate);

    let n_channels = srr.ident_hdr.audio_channels as usize;
    assert_eq!(n_channels, 2, "We require 2 channels for now");

    let out = AudioOutput::new();

    //let mut n = 0;
    while let Some(pck_samples) = srr.read_dec_packet_itl().unwrap() {
        //println!(
        //    "Decoded packet no {}, with {} samples.",
        //    n,
        //    pck_samples.len()
        //);
        //n += 1;
        if pck_samples.len() > 0 {
            out.play_buf(&pck_samples);
        }
    }

    out.drain();
}

#[derive(Copy, Clone, Debug)]
struct Uid(u64);

impl From<rfid_rs::Uid> for Uid {
    fn from(other: rfid_rs::Uid) -> Self {
        let mut val = 0;
        for b in other.bytes {
            val += b as u64;
            val = val << 8;
        }
        Uid(val)
    }
}

#[derive(Copy, Clone, Debug)]
enum RfidEvent {
    Removed,
    Added(Uid),
}

const RFID_INTERRUPT_PIN: u8 = 24;

struct RfidReader {
    mfrc: rfid_rs::MFRC522,
    interrupt_pin: InputPin,
}

impl RfidReader {
    fn new(device_path: impl AsRef<Path>, gpio: &Gpio) -> Result<Self, rfid_rs::Error> {
        let mut spi = spidev::Spidev::open(device_path)?;

        let mut options = spidev::SpidevOptions::new();
        let options = options.max_speed_hz(1_000_000);
        let options = options.mode(spidev::SpiModeFlags::SPI_MODE_0);
        spi.configure(&options)?;

        let mut mfrc = rfid_rs::MFRC522 { spi };

        mfrc.init()?;

        let interrupt_pin = gpio.get(RFID_INTERRUPT_PIN).unwrap().into_input();

        Ok(RfidReader {
            mfrc,
            interrupt_pin,
        })
    }

    fn read_uid(&mut self) -> Option<Uid> {
        let max_tries = 10; //TODO: not sure if this is a proper amount, yet!
        for _ in 0..max_tries {
            match self.mfrc.request_a(2) {
                Err(rfid_rs::Error::Timeout) => {
                    //println!("Wakeup: Timeout...");
                }
                Err(rfid_rs::Error::Communication) => {
                    //eprintln!("Read: communication error");
                }
                Err(o) => {
                    eprintln!("Wakeup: Other error: {:?}", o);
                }
                Ok(_) => match self.mfrc.read_card_serial() {
                    Ok(serial) => {
                        return Some(serial.into());
                    }
                    Err(rfid_rs::Error::Timeout) => {
                        //println!("Read: Timeout");
                    }
                    Err(rfid_rs::Error::Communication) => {
                        eprintln!("Read: communication error");
                    }
                    Err(o) => {
                        eprintln!("Read: Other error: {:?}", o);
                    }
                },
            }
        }
        None
    }

    fn clear_interrupts(&mut self) {
        self.mfrc
            .write_register(rfid_rs::Register::ComIrqReg, 0x80)
            .unwrap(); // clear interrupts
    }

    fn events(&mut self, check_timeout: Duration) -> impl Iterator<Item = RfidEvent> + '_ {
        let mut previous = None;
        let mut previous_time = std::time::Instant::now();
        self.clear_interrupts();

        self.mfrc
            .write_register(rfid_rs::Register::ComlEnReg, 0x7f)
            .unwrap(); // enable interrupts
        self.mfrc
            .write_register(rfid_rs::Register::DivlEnReg, 0x14)
            .unwrap(); // ???

        std::iter::from_fn(move || loop {
            self.clear_interrupts();
            self.interrupt_pin
                .set_interrupt(rppal::gpio::Trigger::RisingEdge)
                .unwrap();
            if self
                .interrupt_pin
                .poll_interrupt(true, Some(check_timeout))
                .unwrap()
                .is_some()
            {
                println!("Interrupt!");
            }
            //let elapsed = previous_time.elapsed();
            //if elapsed < check_interval {
            //    let wait_time = check_interval - elapsed;
            //    //eprintln!("RFID waiting: {:?}", wait_time);
            //    std::thread::sleep(wait_time);
            //}
            //previous_time = std::time::Instant::now();
            //eprintln!("Trying to read..., (elapsed {:?})", elapsed);
            match (self.read_uid(), previous) {
                (Some(uid), None) => {
                    previous = Some(uid.clone());
                    return Some(RfidEvent::Added(uid));
                }
                (None, Some(_)) => {
                    previous = None;
                    return Some(RfidEvent::Removed);
                }
                _ => {}
            }
        })
    }
}

fn main() {
    general_setup();

    let gpio = Gpio::new().unwrap();
    let mut rfid_reader = RfidReader::new("/dev/spidev0.0", &gpio).unwrap();

    let rfid_thread = std::thread::Builder::new()
        .name("card_event_thread".to_owned())
        .spawn(move || {
            for e in rfid_reader.events(Duration::from_millis(500)) {
                println!("Event: {:0x?}", e);
            }
        })
        .unwrap();

    play_file("./mcd.ogg");

    rfid_thread.join().unwrap();
}
