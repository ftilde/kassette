use rfid_rs;
use spidev;

use std::path::Path;
use std::time::{Duration, Instant};

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

#[allow(unused)]
fn play_sine() {
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
    println!("Mixer: {:?}", mixer);
    for elm in mixer.iter() {
        println!("MixerElm: {:?}", elm);
        let selm = alsa::mixer::Selem::new(elm).unwrap();
        dbg!(selm.has_volume());
        dbg!(selm.can_playback());
        dbg!(selm.can_playback());
        let channelid = alsa::mixer::SelemChannelId::mono();
        dbg!(selm.get_playback_volume(channelid).unwrap());
        dbg!(selm.get_playback_vol_db(channelid).unwrap());
        let (_, maxvol) = selm.get_playback_volume_range();
        selm.set_playback_volume_all(maxvol).unwrap();
        dbg!(selm.get_playback_volume(channelid).unwrap());
        dbg!(selm.get_playback_vol_db(channelid).unwrap());
    }

    use alsa::pcm::{Access, Format, HwParams, State, PCM};
    use alsa::{Direction, ValueOr};

    // Open default playback device
    let pcm = PCM::new("default", Direction::Playback, false).unwrap();

    // Set hardware parameters: 44100 Hz / Mono / 16 bit
    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(1).unwrap();
    hwp.set_rate(44100, ValueOr::Nearest).unwrap();
    hwp.set_format(Format::s16()).unwrap();
    hwp.set_access(Access::RWInterleaved).unwrap();
    pcm.hw_params(&hwp).unwrap();
    let io = pcm.io_i16().unwrap();

    // Make sure we don't start the stream too early
    let hwp = pcm.hw_params_current().unwrap();
    let swp = pcm.sw_params_current().unwrap();
    swp.set_start_threshold(hwp.get_buffer_size().unwrap() - hwp.get_period_size().unwrap())
        .unwrap();
    pcm.sw_params(&swp).unwrap();

    // Make a sine wave
    let mut buf = [0i16; 1024];
    for (i, a) in buf.iter_mut().enumerate() {
        *a = ((i as f32 * 2.0 * ::std::f32::consts::PI / 128.0).sin() * 8192.0) as i16
    }

    let len = 100;

    // Play it back for 2 seconds.
    for _ in 0..len * 44100 / 1024 {
        assert_eq!(io.writei(&buf[..]).unwrap(), 1024);
    }

    // In case the buffer was larger than 2 seconds, start the stream manually.
    if pcm.state() != State::Running {
        pcm.start().unwrap()
    };
    // Wait for the stream to finish playback.
    pcm.drain().unwrap();
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

struct RfidReader {
    mfrc: rfid_rs::MFRC522,
}

impl RfidReader {
    fn new(device_path: impl AsRef<Path>) -> Result<Self, rfid_rs::Error> {
        let mut spi = spidev::Spidev::open(device_path)?;

        let mut options = spidev::SpidevOptions::new();
        let options = options.max_speed_hz(1_000_000);
        let options = options.mode(spidev::SpiModeFlags::SPI_MODE_0);
        spi.configure(&options)?;

        let mut mfrc = rfid_rs::MFRC522 { spi };

        mfrc.init()?;

        Ok(RfidReader { mfrc })
    }

    fn read_uid(&mut self, timeout: Duration) -> Option<Uid> {
        let start = Instant::now();
        while start.elapsed() < timeout {
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

    fn events(&mut self, check_interval: Duration) -> impl Iterator<Item = RfidEvent> + '_ {
        let mut previous = None;
        std::iter::from_fn(move || loop {
            match (self.read_uid(check_interval), previous) {
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

    let mut rfid_reader = RfidReader::new("/dev/spidev0.0").unwrap();

    for e in rfid_reader.events(Duration::from_millis(50)) {
        println!("Event: {:0x?}", e);
    }
}
