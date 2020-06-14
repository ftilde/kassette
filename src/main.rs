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

fn play_file(file_path: impl AsRef<Path>) {
    let f = std::fs::File::open(file_path).expect("Can't open file");

    // Prepare the reading
    let mut srr = OggStreamReader::new(f).unwrap();

    // Prepare the playback.
    println!("Sample rate: {}", srr.ident_hdr.audio_sample_rate);

    let n_channels = srr.ident_hdr.audio_channels as usize;
    assert_eq!(n_channels, 2, "We require 2 channels for now");

    let out = sound::AudioOutput::new();

    let vol_div = 16;

    //let mut n = 0;
    while let Some(mut pck_samples) = srr.read_dec_packet_itl().unwrap() {
        //println!(
        //    "Decoded packet no {}, with {} samples.",
        //    n,
        //    pck_samples.len()
        //);
        //n += 1;
        for s in &mut pck_samples {
            *s /= vol_div;
        }
        if pck_samples.len() > 0 {
            out.play_buf(&pck_samples);
        }
    }

    out.drain();
}

fn main() {
    general_setup();

    let gpio = rppal::gpio::Gpio::new().unwrap();
    let mut rfid_reader =
        rfid::RfidReader::new("/dev/spidev0.0", gpio.get(pins::RFID_INTERRUPT).unwrap()).unwrap();

    let rfid_thread = std::thread::Builder::new()
        .name("card_event_thread".to_owned())
        .spawn(move || {
            for e in rfid_reader.events(Duration::from_millis(100)) {
                println!("Event: {:0x?}", e);
            }
        })
        .unwrap();

    let mut rotary_encoder = rotary_encoder::RotaryEncoder::new(
        gpio.get(pins::ROTARY_ENCODER_EVENT).unwrap(),
        gpio.get(pins::ROTARY_ENCODER_DIRECTION).unwrap(),
    );

    let _ = std::thread::Builder::new()
        .name("rotary_encoder_thread".to_owned())
        .spawn(move || {
            for e in rotary_encoder.events(Duration::from_millis(25)) {
                println!("Event: {:0x?}", e);
            }
        })
        .unwrap();

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

    play_file("./mcd2.ogg");

    rfid_thread.join().unwrap(); //TODO this will never happen!
}
