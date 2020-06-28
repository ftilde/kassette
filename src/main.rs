use crate::rfid::Uid;
use std::sync::mpsc;
use std::time::Duration;

mod led;
mod media_definition;
mod pins;
mod player;
mod rfid;
mod rotary_encoder;
mod save_state;
mod sound;

const SAVESTATE_PATH: &str = "savestate.json";

fn is_init() -> bool {
    std::process::id() == 1
}

/// Mounting stuff etc.
fn general_setup() {
    if is_init() {
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

enum Event {
    Play(Uid),
    Stop,
    IncreaseVolume,
    DecreaseVolume,
    Shutdown,
}

fn main() {
    general_setup();

    let file_map = if is_init() {
        media_definition::load_media_definition("", "") //TODO
    } else {
        let f = std::io::Cursor::new(
            &r"
            0xa930fcb800 mcd.ogg
            0xc3aa960c00 mcd2.ogg
            "[..],
        );
        media_definition::parse_media_definition(f, "")
        //load_media_definition("./rfid_file_definition.txt", "")
    };

    let gpio = rppal::gpio::Gpio::new().unwrap();
    let mut rfid_reader =
        rfid::RfidReader::new("/dev/spidev0.0", gpio.get(pins::RFID_INTERRUPT).unwrap()).unwrap();

    let (event_sink, event_source) = mpsc::channel();
    let rfid_event_sink = event_sink.clone();
    let rotary_encoder_event_sink = event_sink.clone();
    let shutdown_event_sink = event_sink;

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

    let mut save_state = save_state::SaveState::load(SAVESTATE_PATH).unwrap_or_default();

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
    let mut player = player::Player::new(out, save_state.volume());

    let mut sw = gpio
        .get(pins::ROTARY_ENCODER_SWITCH)
        .unwrap()
        .into_input_pullup();

    sw.set_async_interrupt(rppal::gpio::Trigger::FallingEdge, move |_| {
        shutdown_event_sink.send(Event::Shutdown).unwrap();
    })
    .unwrap();

    let mut last_card = None;

    if let Some((uid, pos)) = save_state.playback_pos() {
        last_card = Some(uid);
        if let Some(file) = file_map.get(&uid) {
            player.play_file(file, Some(pos));
            player.pause();
        } else {
            eprintln!("Cannot resume for unknown uid: {:x}", uid.0);
        }
    }

    loop {
        match event_source.try_recv() {
            Ok(Event::IncreaseVolume) => {
                led_cmd_sink
                    .send(led::LedCommand::Blink(Duration::from_millis(100)))
                    .unwrap();
                *player.volume() += 1;
            }
            Ok(Event::DecreaseVolume) => {
                led_cmd_sink
                    .send(led::LedCommand::DoubleBlink(
                        Duration::from_millis(40),
                        Duration::from_millis(20),
                        Duration::from_millis(40),
                    ))
                    .unwrap();
                *player.volume() -= 1;
            }
            Ok(Event::Play(uid)) => {
                if last_card == Some(uid) && !player.idle() {
                    player.resume();
                } else {
                    if let Some(file) = file_map.get(&uid) {
                        player.play_file(file, None);
                    } else {
                        eprintln!("Unkown card: {:x}", uid.0);
                    }
                    last_card = Some(uid);
                }
                led_cmd_sink
                    .send(led::LedCommand::Blink(Duration::from_millis(500)))
                    .unwrap();
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
            Ok(Event::Shutdown) => {
                break;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                panic!("Player event channel closed unexpectedly")
            }
        }
        player.push_samples();
    }

    let playback_pos = if let (Some(uid), Some(pos)) = (last_card, player.playback_pos()) {
        Some((uid, pos))
    } else {
        None
    };
    save_state.set_playback_pos(playback_pos);
    save_state.set_volume(*player.volume());
    save_state.save(SAVESTATE_PATH);

    if is_init() {
        nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_POWER_OFF).unwrap();
    } else {
        eprintln!("Not powering off because not running as PID 1");
    }
}
