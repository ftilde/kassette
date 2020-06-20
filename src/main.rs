use std::sync::mpsc;
use std::time::Duration;

mod led;
mod pins;
mod player;
mod rfid;
mod rotary_encoder;
mod sound;

const INITIAL_VOLUME: usize = 13;

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

use rfid::Uid;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
fn parse_num(s: &str) -> Option<u64> {
    match s.as_bytes() {
        [b'0', b'b', ..] => u64::from_str_radix(&s[2..], 2),
        [b'0', b'x', ..] => u64::from_str_radix(&s[2..], 16),
        [b'0', b'o', ..] => u64::from_str_radix(&s[2..], 8),
        _ => u64::from_str_radix(s, 10),
    }
    .ok()
}
fn parse_line(l: &str) -> Option<(Uid, PathBuf)> {
    let end = l.find(" ")?;
    let uid_str = &l[..end];
    let path_str = l[end..].trim();
    let uid = Uid(parse_num(uid_str)?);
    let path = PathBuf::from(path_str);
    Some((uid, path))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_num() {
        assert_eq!(parse_num("aa"), None);
        assert_eq!(parse_num("0xdeadbeef"), Some(0xdeadbeef));
        assert_eq!(parse_num("0b11"), Some(0b11));
        assert_eq!(parse_num("0o11"), Some(0o11));
        assert_eq!(parse_num("37"), Some(37));
    }

    #[test]
    fn test_parse_line() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line(" "), None);
        assert_eq!(parse_line("bla"), None);
        assert_eq!(parse_line("123"), None);
        assert_eq!(
            parse_line("123 /foo/bar"),
            Some((Uid(123), PathBuf::from("/foo/bar")))
        );
        assert_eq!(
            parse_line("0x42 baz"),
            Some((Uid(0x42), PathBuf::from("baz")))
        );
    }

    #[test]
    fn test_parse_media_definition() {
        let f = std::io::Cursor::new(
            &r"
            0x123 foo/bar
            456 /bla/

            #013 commented out

            # just some comment
            0xcafe cafe.ogg
            "[..],
        );
        let m = parse_media_definition(f, "/root/");
        assert_eq!(m.len(), 3);
        assert_eq!(m.get(&Uid(0x123)).unwrap(), &PathBuf::from("/root/foo/bar"));
        assert_eq!(m.get(&Uid(456)).unwrap(), &PathBuf::from("/bla"));
        assert_eq!(
            m.get(&Uid(0xcafe)).unwrap(),
            &PathBuf::from("/root/cafe.ogg")
        );
    }
}
fn load_media_definition(
    map_definition_file: impl AsRef<Path>,
    media_file_root: impl AsRef<Path>,
) -> HashMap<Uid, PathBuf> {
    let f = std::fs::File::open(map_definition_file).unwrap();
    parse_media_definition(f, media_file_root)
}

fn parse_media_definition(
    src: impl std::io::Read,
    media_file_root: impl AsRef<Path>,
) -> HashMap<Uid, PathBuf> {
    let f = BufReader::new(src);
    let media_file_root = media_file_root.as_ref();

    let mut map = HashMap::new();

    for l in f.lines() {
        let l = match l {
            Ok(l) => l,
            Err(_) => continue,
        };
        let l = l.trim_start();
        if l.is_empty() || l.starts_with("#") {
            continue;
        }
        if let Some((uid, path)) = parse_line(l) {
            map.insert(uid, media_file_root.join(path));
        }
    }
    map
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
        load_media_definition("", "") //TODO
    } else {
        let f = std::io::Cursor::new(
            &r"
            0xa930fcb800 mcd.ogg
            0xc3aa960c00 mcd2.ogg
            "[..],
        );
        parse_media_definition(f, "")
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
    let mut player = player::Player::new(out, INITIAL_VOLUME);

    let mut sw = gpio
        .get(pins::ROTARY_ENCODER_SWITCH)
        .unwrap()
        .into_input_pullup();

    sw.set_async_interrupt(rppal::gpio::Trigger::FallingEdge, move |_| {
        shutdown_event_sink.send(Event::Shutdown).unwrap();
    })
    .unwrap();

    let mut last_card = None;

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
            Ok(Event::Play(uid)) => {
                if last_card == Some(uid) && !player.idle() {
                    player.resume();
                } else {
                    if let Some(file) = file_map.get(&uid) {
                        player.play_file(file);
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

    //TODO: Save state etc

    if is_init() {
        nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_POWER_OFF).unwrap();
    } else {
        eprintln!("Not powering off because not running as PID 1");
    }
}
