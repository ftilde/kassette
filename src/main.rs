use std::sync::mpsc;
use std::time::Duration;

mod led;
mod pins;
mod player;
mod rfid;
mod rotary_encoder;
mod sound;

const INITIAL_VOLUME: usize = 13;

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
    let mut player = player::Player::new(out, INITIAL_VOLUME);

    player.play_file("./mcd2.ogg");

    //let mut sw = gpio
    //    .get(pins::ROTARY_ENCODER_SWITCH)
    //    .unwrap()
    //    .into_input_pullup();

    //sw.set_async_interrupt(rppal::gpio::Trigger::FallingEdge, |_| {
    //    println!("SW pressed!")
    //})
    //.unwrap();

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
