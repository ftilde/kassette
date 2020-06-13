use rfid_rs;
use rppal::gpio::{Gpio, InputPin, OutputPin};
use spidev;

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

mod pins;

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

#[derive(Debug)]
enum RfIdError {
    Rfid(rfid_rs::Error),
    //Io(std::io::Error),
    Gpio(rppal::gpio::Error),
}

impl From<rfid_rs::Error> for RfIdError {
    fn from(error: rfid_rs::Error) -> Self {
        RfIdError::Rfid(error)
    }
}

impl From<rppal::gpio::Error> for RfIdError {
    fn from(error: rppal::gpio::Error) -> Self {
        RfIdError::Gpio(error)
    }
}

//impl From<std::io::Error> for RfIdError {
//    fn from(error: std::io::Error) -> Self {
//        RfIdError::Io(error)
//    }
//}

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

        let interrupt_pin = gpio.get(pins::RFID_INTERRUPT).unwrap().into_input_pullup();

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

    fn check_card_present(&mut self, check_timeout: Duration) -> Result<bool, RfIdError> {
        self.mfrc.init()?;

        // Clear previous interrupt bits
        self.mfrc
            .write_register(rfid_rs::Register::ComIrqReg, 0x00)?;

        // Enable Rx interrupt and invert IRQ (i.e., we wait for low)
        self.mfrc
            .write_register(rfid_rs::Register::ComlEnReg, 0b1010_0000)?;

        // Write 0x26 (Request for card activation) to fifo buffer
        self.mfrc
            .write_register(rfid_rs::Register::FIFODataReg, 0x26)?;

        // Issue transmission of Card activation request
        self.mfrc.write_register(
            rfid_rs::Register::CommandReg,
            rfid_rs::Command::Transceive as _,
        )?;
        // Describe transmission (start transmission of 7 bits (i.e., 0x26))
        self.mfrc
            .write_register(rfid_rs::Register::BitFramingReg, 0b1_000_0_111)?;

        // Wait for interrupt to get low (i.e., Rx event, see above)
        self.interrupt_pin
            .set_interrupt(rppal::gpio::Trigger::FallingEdge)?;
        if let Some(rppal::gpio::Level::Low) = self
            .interrupt_pin
            .poll_interrupt(true, Some(check_timeout))?
        {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn events(&mut self, check_timeout: Duration) -> impl Iterator<Item = RfidEvent> + '_ {
        let mut previous = None;
        let mut previous_time = std::time::Instant::now();

        std::iter::from_fn(move || loop {
            let elapsed = previous_time.elapsed();
            if elapsed < check_timeout {
                let wait_time = check_timeout - elapsed;
                std::thread::sleep(wait_time);
            }
            previous_time = std::time::Instant::now();

            let present = loop {
                if let Ok(val) = self.check_card_present(check_timeout) {
                    break val;
                }
            };

            match (previous, present) {
                (Some(_), false) => {
                    previous = None;
                    return Some(RfidEvent::Removed);
                }
                (None, true) => {
                    if let Some(uid) = self.read_uid() {
                        previous = Some(uid.clone());
                        return Some(RfidEvent::Added(uid));
                    }
                }
                _ => {}
            }
        })
    }
}

#[derive(Copy, Clone, Debug)]
enum RotaryEncoderEvent {
    TurnLeft,
    TurnRight,
}

struct RotaryEncoder {
    event_pin: InputPin,
    direction_pin: InputPin,
}

impl RotaryEncoder {
    fn new(gpio: &Gpio) -> Result<Self, rppal::gpio::Error> {
        let event_pin = gpio.get(pins::ROTARY_ENCODER_EVENT).unwrap().into_input();
        let direction_pin = gpio
            .get(pins::ROTARY_ENCODER_DIRECTION)
            .unwrap()
            .into_input();

        Ok(RotaryEncoder {
            event_pin,
            direction_pin,
        })
    }

    fn wait_for_event(&mut self) -> Result<(), rppal::gpio::Error> {
        loop {
            self.event_pin
                .set_interrupt(rppal::gpio::Trigger::FallingEdge)?;
            if let Some(rppal::gpio::Level::Low) = self.event_pin.poll_interrupt(true, None)? {
                return Ok(());
            }
        }
    }

    fn events(&mut self, debounce_time: Duration) -> impl Iterator<Item = RotaryEncoderEvent> + '_ {
        let mut previous_time = std::time::Instant::now();

        std::iter::from_fn(move || loop {
            if self.wait_for_event().is_err() {
                continue;
            }
            let elapsed = previous_time.elapsed();
            if elapsed < debounce_time {
                continue;
            }
            previous_time = std::time::Instant::now();

            match self.direction_pin.read() {
                rppal::gpio::Level::Low => break Some(RotaryEncoderEvent::TurnLeft),
                rppal::gpio::Level::High => break Some(RotaryEncoderEvent::TurnRight),
            }
        })
    }
}

struct Led {
    output_pin: OutputPin,
}

enum LedCommand {
    Blink(Duration),
}

impl Led {
    fn new(gpio: &Gpio) -> Result<Self, rppal::gpio::Error> {
        let output_pin = gpio.get(pins::LED_OUTPUT_PIN).unwrap().into_output();

        Ok(Led { output_pin })
    }

    fn on(&mut self) {
        self.output_pin.set_high();
    }

    fn off(&mut self) {
        self.output_pin.set_low();
    }

    fn execute(&mut self, cmd: LedCommand) {
        match cmd {
            LedCommand::Blink(len) => {
                self.on();
                std::thread::sleep(len);
                self.off();
            }
        }
    }
}

fn main() {
    general_setup();

    let gpio = Gpio::new().unwrap();
    let mut rfid_reader = RfidReader::new("/dev/spidev0.0", &gpio).unwrap();

    let rfid_thread = std::thread::Builder::new()
        .name("card_event_thread".to_owned())
        .spawn(move || {
            for e in rfid_reader.events(Duration::from_millis(100)) {
                println!("Event: {:0x?}", e);
            }
        })
        .unwrap();

    let mut rotary_encoder = RotaryEncoder::new(&gpio).unwrap();

    let _ = std::thread::Builder::new()
        .name("rotary_encoder_thread".to_owned())
        .spawn(move || {
            for e in rotary_encoder.events(Duration::from_millis(25)) {
                println!("Event: {:0x?}", e);
            }
        })
        .unwrap();

    let mut led = Led::new(&gpio).unwrap();

    let (led_cmd_sink, led_cmd_source) = mpsc::channel();

    let _ = std::thread::Builder::new()
        .name("led_thread".to_owned())
        .spawn(move || {
            while let Ok(cmd) = led_cmd_source.recv() {
                led.execute(cmd);
            }
        })
        .unwrap();

    led_cmd_sink
        .send(LedCommand::Blink(Duration::from_millis(500)))
        .unwrap();

    play_file("./mcd.ogg");

    rfid_thread.join().unwrap(); //TODO this will never happen!
}
