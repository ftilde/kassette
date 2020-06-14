use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

mod led;
mod pins;
mod rfid;
mod rotary_encoder;
mod sound;

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

use lewton::inside_ogg::OggStreamReader;

const MAX_VOLUME: usize = 16;
const INITIAL_VOLUME: usize = 13;
const MUTED_BUF: &[i16] = &[0; 1024];

enum PlayerState {
    Playing(OggStreamReader<std::fs::File>),
    Paused(OggStreamReader<std::fs::File>),
    Idle,
}

struct Player {
    output: sound::AudioOutput,
    state: PlayerState,
    volume: usize,
}

impl Player {
    pub fn new(output: sound::AudioOutput, volume: usize) -> Self {
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

    fn play_file(&mut self, file_path: impl AsRef<Path>) {
        let f = std::fs::File::open(file_path).expect("Can't open file");

        // Prepare the reading
        let srr = OggStreamReader::new(f).unwrap();

        // Prepare the playback.
        println!("Sample rate: {}", srr.ident_hdr.audio_sample_rate);

        let n_channels = srr.ident_hdr.audio_channels as usize;
        assert_eq!(n_channels, 2, "We require 2 channels for now");

        self.state = PlayerState::Playing(srr);
    }

    fn pause(&mut self) {
        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);
        self.state = match dummy {
            PlayerState::Playing(i) => PlayerState::Paused(i),
            o => o,
        }
    }

    fn resume(&mut self) {
        let mut dummy = PlayerState::Idle;
        std::mem::swap(&mut dummy, &mut self.state);
        self.state = match dummy {
            PlayerState::Paused(i) => PlayerState::Playing(i),
            o => o,
        }
    }

    pub fn push_samples(&mut self) {
        match &mut self.state {
            PlayerState::Playing(srr) => {
                if let Some(mut pck_samples) = srr.read_dec_packet_itl().unwrap() {
                    for s in &mut pck_samples {
                        *s = *s >> (MAX_VOLUME - self.volume);
                    }
                    //TODO resampling
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

enum Event {
    Play(rfid::Uid),
    Stop,
    IncreaseVolume,
    DecreaseVolume,
}

fn main() {
    general_setup();

    let gpio = rppal::gpio::Gpio::new().unwrap();
    let mut rfid_reader =
        rfid::RfidReader::new("/dev/spidev0.0", gpio.get(pins::RFID_INTERRUPT).unwrap()).unwrap();

    let (event_sink, event_source) = mpsc::channel();
    let rfid_event_sink = event_sink.clone();
    let rotary_encoder_event_sink = event_sink;

    let _rfid_thread = std::thread::Builder::new()
        .name("card_event_thread".to_owned())
        .spawn(move || {
            for e in rfid_reader.events(Duration::from_millis(100)) {
                println!("Event: {:0x?}", e);
                let event = match e {
                    rfid::RfidEvent::Added(uid) => Event::Play(uid),
                    rfid::RfidEvent::Removed => Event::Stop,
                };
                rfid_event_sink.send(event).unwrap();
            }
        })
        .unwrap();

    let rotary_encoder = rotary_encoder::RotaryEncoder::new(
        gpio.get(pins::ROTARY_ENCODER_EVENT).unwrap(),
        gpio.get(pins::ROTARY_ENCODER_DIRECTION).unwrap(),
    );

    let _guard = rotary_encoder.start_events(Duration::from_millis(25), move |e| {
        let event = match e {
            rotary_encoder::RotaryEncoderEvent::TurnLeft => Event::DecreaseVolume,
            rotary_encoder::RotaryEncoderEvent::TurnRight => Event::IncreaseVolume,
        };
        rotary_encoder_event_sink.send(event).unwrap();
        println!("Event: {:0x?}", e)
    });

    let mut led = led::Led::new(gpio.get(pins::LED_OUTPUT_PIN).unwrap());

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
        .send(led::LedCommand::Blink(Duration::from_millis(500)))
        .unwrap();

    let out = sound::AudioOutput::new();
    let mut player = Player::new(out, INITIAL_VOLUME);

    player.play_file("./mcd2.ogg");

    loop {
        match event_source.try_recv() {
            Ok(Event::IncreaseVolume) => {
                led_cmd_sink
                    .send(led::LedCommand::Blink(Duration::from_millis(100)))
                    .unwrap();
                player.increase_volume();
            }
            Ok(Event::DecreaseVolume) => {
                led_cmd_sink
                    .send(led::LedCommand::DoubleBlink(
                        Duration::from_millis(40),
                        Duration::from_millis(20),
                        Duration::from_millis(40),
                    ))
                    .unwrap();
                player.decrease_volume();
            }
            Ok(Event::Play(_)) => {
                led_cmd_sink
                    .send(led::LedCommand::Blink(Duration::from_millis(500)))
                    .unwrap();
                player.resume();
            }
            Ok(Event::Stop) => {
                led_cmd_sink
                    .send(led::LedCommand::DoubleBlink(
                        Duration::from_millis(200),
                        Duration::from_millis(100),
                        Duration::from_millis(200),
                    ))
                    .unwrap();
                player.pause();
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                panic!("Player event channel closed unexpectedly")
            }
        }
        player.push_samples();
    }
}
