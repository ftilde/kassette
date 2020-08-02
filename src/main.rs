use crate::rfid::Uid;
use argh::FromArgs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

mod config;
#[macro_use]
mod log;
mod led;
mod media_definition;
mod pins;
mod player;
mod polyfill;
mod rfid;
mod rotary_encoder;
mod save_state;
mod sound;

#[derive(FromArgs)]
/// Reach new heights.
struct Options {
    /// data block device that will be mounted on start when running as pid1
    #[argh(option, default = r#"PathBuf::from("/dev/mmcblk0p2")"#)]
    data_device: PathBuf,

    /// spi device that is used to communicate with the rfid reader
    #[argh(option, default = r#"PathBuf::from("/dev/spidev0.0")"#)]
    rfid_device: PathBuf,
}

fn is_init() -> bool {
    std::process::id() == 1
}

/// Mounting stuff etc.
fn setup(options: &Options) {
    if is_init() {
        nix::mount::mount::<str, str, str, str>(
            None,
            "/dev",
            Some("devtmpfs"),
            nix::mount::MsFlags::empty(),
            None,
        )
        .unwrap(); // If this fails we cannot do anything anyways.
        nix::mount::mount::<str, str, str, str>(
            None,
            "/proc",
            Some("proc"),
            nix::mount::MsFlags::empty(),
            None,
        )
        .unwrap(); // If this fails we cannot do anything anyways.

        let mut sleep_duration = Duration::from_millis(10);
        loop {
            match nix::mount::mount::<PathBuf, str, str, str>(
                Some(&options.data_device),
                config::DATA_MOUNT_PATH,
                Some("ext4"),
                nix::mount::MsFlags::empty(),
                None,
            ) {
                Ok(_) => break,
                Err(nix::Error::Sys(nix::errno::Errno::ENOENT)) => {
                    eprintln!(
                        "Waiting for sd card {}...",
                        options.data_device.to_string_lossy()
                    );
                    std::thread::sleep(sleep_duration);
                    sleep_duration *= 2;
                }
                Err(o) => {
                    panic!("Error mounting sd card: {}", o);
                }
            }
        }
    }
}

fn tear_down() {
    if is_init() {
        loop {
            match nix::mount::umount(config::DATA_MOUNT_PATH) {
                Ok(_) => break,
                Err(nix::Error::Sys(nix::errno::Errno::EBUSY)) => {
                    eprintln!("Waiting for sd to unmount...");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(o) => panic!("Unexpected error during umount: {}", o),
            }
        }
        nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_POWER_OFF).unwrap();
        // If poweroff fails we cannot do anything.
    }
}

enum Event {
    Play(Uid),
    Stop,
    IncreaseVolume,
    DecreaseVolume,
    Shutdown,
}

enum CardState {
    Current(Uid),
    Previous(Uid, SystemTime),
    Nothing,
}

fn resume_rewind_time(stop_time: Duration) -> Duration {
    let relevant = stop_time
        .checked_sub(config::MIN_TIME_FOR_CONTEXT)
        .unwrap_or(Duration::from_secs(0));

    let context = (relevant / config::PAUSE_TO_CONTEXT_RATIO).min(config::MAX_CONTEXT_TIME);

    // We fade in and out, so we have to rewind for the fade-out before the pause (because that
    // might not have been completely audible) and for the fade-in that will be done when resuming
    // (again, might not be completely audible).
    context + 2 * config::FADE_TIME
}

fn main() {
    let options: Options = Options {
        data_device: PathBuf::from("/dev/mmcblk0p2"),
        rfid_device: PathBuf::from("/dev/spidev0.0"),
    };

    setup(&options);
    log::init_logger(data_root().join(config::LOG_FILE));
    log!("=============== New log ===============");

    std::panic::set_hook(Box::new(|info| {
        log!("Panic in run: {}", info);
    }));

    let _ = std::panic::catch_unwind(|| run(options));

    let _ = std::panic::take_hook();

    log::deinit_logger();
    tear_down();
}

fn data_root() -> &'static Path {
    let data_root = if is_init() {
        config::DATA_MOUNT_PATH
    } else {
        "./"
    };
    &Path::new(data_root)
}

fn run(options: Options) {
    let data_root = data_root();
    let file_map = media_definition::load_media_definition(
        data_root.join(config::MEDIA_DEFINITION_FILE),
        data_root,
    );
    let save_state_path = data_root.join(config::SAVESTATE_FILE);

    let gpio = rppal::gpio::Gpio::new().unwrap();
    let mut rfid_reader =
        rfid::RfidReader::new(options.rfid_device, gpio.get(pins::RFID_INTERRUPT).unwrap())
            .unwrap();

    let (event_sink, event_source) = mpsc::channel();
    let rfid_event_sink = event_sink.clone();
    let rotary_encoder_event_sink = event_sink.clone();
    let shutdown_event_sink = event_sink;

    let _rfid_thread = std::thread::Builder::new()
        .name("card_event_thread".to_owned())
        .spawn(move || {
            for e in rfid_reader.events(Duration::from_millis(100)) {
                log!("Event: {:0x?}", e);
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

    let _guard = rotary_encoder.start_events(move |e| {
        let event = match e {
            rotary_encoder::RotaryEncoderEvent::TurnLeft => Event::DecreaseVolume,
            rotary_encoder::RotaryEncoderEvent::TurnRight => Event::IncreaseVolume,
        };
        rotary_encoder_event_sink.send(event).unwrap(); //RE thread never finishes.
    });

    let mut save_state = save_state::SaveState::load(&save_state_path).unwrap_or_default();

    let mut led = led::Led::new(gpio.get(pins::LED_OUTPUT_PIN).unwrap());

    let (led_cmd_sink, led_cmd_source) = mpsc::channel();

    let led_thread = std::thread::Builder::new()
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

    let mut card_state = CardState::Nothing;
    let mut silence_begin = Some(Instant::now());

    if let Some((uid, pos, stop_time)) = save_state.playback_state() {
        card_state = CardState::Previous(uid, stop_time);
        if let Some(file) = file_map.get(&uid) {
            log_err!("Load initial file", player.load_file(file, Some(pos)));
        } else {
            log!("Cannot load unknown uid: {:x}", uid.0);
        }
    }

    let mut stopped = false;
    while !(stopped && !player.playing()) {
        match event_source.try_recv() {
            Ok(Event::IncreaseVolume) => {
                led_cmd_sink
                    .send(led::LedCommand::Blink(Duration::from_millis(5)))
                    .unwrap();
                *player.volume() += 1;
            }
            Ok(Event::DecreaseVolume) => {
                led_cmd_sink
                    .send(led::LedCommand::DoubleBlink(
                        Duration::from_millis(5),
                        Duration::from_millis(40),
                        Duration::from_millis(5),
                    ))
                    .unwrap();
                *player.volume() -= 1;
            }
            Ok(Event::Play(uid)) => {
                let (old_uid, remove_time) = match card_state {
                    CardState::Previous(old_uid, remove_time) => (Some(old_uid), Some(remove_time)),
                    CardState::Current(old_uid) => (Some(old_uid), None),
                    CardState::Nothing => (None, None),
                };
                if old_uid == Some(uid) && !player.idle() {
                    if let Some(remove_time) = remove_time {
                        let stop_time = SystemTime::now()
                            .duration_since(remove_time)
                            .unwrap_or(Duration::from_millis(0));
                        log_err!(
                            "Rewind from remove time",
                            player.rewind(resume_rewind_time(stop_time))
                        );
                    }
                    player.play();
                } else {
                    if let Some(file) = file_map.get(&uid) {
                        log!("Starting to play {:?}", file);
                        log_err!("Load file for card", player.load_file(file, None));
                        player.play();
                    } else {
                        log!("Unkown card: {}", uid);
                    }
                }
                card_state = CardState::Current(uid);
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
                if let CardState::Current(uid) = card_state {
                    card_state = CardState::Previous(uid, SystemTime::now());
                }
                player.pause();
            }
            Ok(Event::Shutdown) => {
                player.pause();
                stopped = true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                panic!("Player event channel closed unexpectedly")
            }
        }
        if player.playing() {
            silence_begin = None;
        } else {
            if let Some(silence_begin) = silence_begin {
                if silence_begin.elapsed() >= config::IDLE_SLEEP_TIME {
                    log!("Idle sleep time reached");
                    break;
                }
            } else {
                silence_begin = Some(Instant::now());
            }
        }
        player.push_samples();
    }

    if silence_begin.is_none() || silence_begin.unwrap().elapsed() < config::IDLE_SLEEP_TIME {
        // Only blink led if not turned off automatically. We don't want to wake anyone up if they
        // actually went asleep.
        led_cmd_sink
            .send(led::LedCommand::DoubleBlink(
                Duration::from_millis(200),
                Duration::from_millis(100),
                Duration::from_millis(200),
            ))
            .unwrap();
    }

    let playback_pos = match (card_state, player.playback_pos()) {
        (CardState::Previous(uid, remove_time), Some(pos)) => Some((uid, pos, remove_time)),
        (CardState::Current(uid), Some(pos)) => Some((uid, pos, SystemTime::now())),
        _ => None,
    };
    save_state.set_playback_state(playback_pos);
    save_state.set_volume(*player.volume());
    log_err!(
        "Failed to write save state",
        save_state.save(&save_state_path)
    );

    // Make sure to execute all remaining led commands, then stop (with inactive led!).
    // As we only stop the thread here, all led command related unwraps above are fine.
    std::mem::drop(led_cmd_sink);
    led_thread.join().unwrap();
}
