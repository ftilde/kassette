//use rfid_rs;
//use spidev;

//use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
//use cpal::{SampleFormat, StreamData, UnknownTypeOutputBuffer};

//#[no_mangle]
//pub unsafe extern "C" fn __nanosleep_time64(
//    rqtp: *const libc::timespec,
//    rmtp: *mut libc::timespec,
//) -> libc::c_int {
//    libc::nanosleep(rqtp, rmtp)
//}
//
//#[no_mangle]
//pub unsafe extern "C" fn __gettimeofday_time64(
//    tp: *mut libc::timeval,
//    tz: *mut core::ffi::c_void,
//) -> libc::c_int {
//    libc::gettimeofday(tp, tz)
//}
//
//#[no_mangle]
//pub unsafe extern "C" fn __clock_gettime64(
//    clk_id: libc::clockid_t,
//    tp: *mut libc::timespec,
//) -> libc::c_int {
//    libc::clock_gettime(clk_id, tp)
//}
//
//#[no_mangle]
//pub unsafe extern "C" fn __dlsym_time64(
//    handle: *mut libc::c_void,
//    symbol: *const libc::c_char,
//) -> *mut core::ffi::c_void {
//    libc::dlsym(handle, symbol)
//}
//
//#[no_mangle]
//pub unsafe extern "C" fn __stat_time64(
//    path: *const libc::c_char,
//    buf: *mut libc::stat64,
//) -> libc::c_int {
//    libc::stat64(path, buf)
//}

fn main() {
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

    /*
    let host = cpal::default_host();
    let event_loop = host.event_loop();

    println!("000000000000000000000000000000000000000000000000000000000000000000000");
    println!("dev/snd: ");
    for file in std::fs::read_dir("/dev/snd").unwrap() {
        print!("{} ", file.unwrap().path().to_string_lossy());
    }

    println!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    println!("proc: ");
    for file in std::fs::read_dir("/proc").unwrap() {
        print!("{} ", file.unwrap().path().to_string_lossy());
    }

    println!("111111111111111111111111111111111111111111111111111111111111111111111");
    let devices = host.devices().unwrap();
    for device in devices {
        println!("Device: {}", device.name().unwrap());
    }
    println!("222222222222222222222222222222222222222222222222222222222222222222222");

    let device = host
        .default_output_device()
        .expect("no output device available");

    let mut supported_formats_range = device
        .supported_output_formats()
        .expect("error while querying formats");
    let mut format = supported_formats_range
        .find(|f| f.data_type == SampleFormat::F32 && f.channels == 1)
        .expect("no supported format?!")
        .with_max_sample_rate();

    let sample_rate = Some(44000);
    if let Some(sample_rate) = sample_rate {
        format.sample_rate.0 = format.sample_rate.0.min(sample_rate);
    }
    println!("Format: {:?}", format);
    let sample_rate = format.sample_rate.0;

    let stream_id = event_loop.build_output_stream(&device, &format).unwrap();

    event_loop.play_stream(stream_id).unwrap();

    let thread = std::thread::Builder::new()
        .name("renderer".to_owned())
        .spawn(move || {
            let mut samples = (0..)
                .into_iter()
                .map(|s| (s as f32 * 440.0 * 2.0 * 3.141592654 / sample_rate as f32).sin());
            event_loop.run(move |_stream_id, stream_data| {
                if let StreamData::Output {
                    buffer: UnknownTypeOutputBuffer::F32(mut fbuffer),
                } = stream_data.unwrap()
                {
                    for elm in fbuffer.iter_mut() {
                        *elm = samples.next().unwrap();
                    }
                } else {
                    panic!("Invalid format");
                }
            });
        })
        .unwrap();

    thread.join().unwrap();
    */
}

//fn main() {
//    let mut spi = spidev::Spidev::open("/dev/spidev0.0").unwrap();
//
//    let mut options = spidev::SpidevOptions::new();
//    let options = options.max_speed_hz(1_000_000);
//    let options = options.mode(spidev::SpiModeFlags::SPI_MODE_0);
//    spi.configure(&options).unwrap();
//
//    let mut mrfc = rfid_rs::MFRC522 { spi };
//
//    println!("Foo");
//    mrfc.init().unwrap();
//    println!("Bar");
//    //mrfc.enable_antenna().unwrap();
//    loop {
//        match mrfc.request_a(2) {
//            Err(rfid_rs::Error::Timeout) => {
//                println!("Wakeup: Timeout...");
//                continue;
//            }
//            Err(o) => {
//                panic!("Wakeup: Other error: {:?}", o);
//            }
//            Ok(_) => match mrfc.read_card_serial() {
//                Ok(serial) => {
//                    println!("serial: {:?}", serial);
//                    break;
//                }
//                Err(rfid_rs::Error::Timeout) => {
//                    println!("Read: Timeout");
//                }
//                Err(o) => {
//                    panic!("Read: Other error: {:?}", o);
//                }
//            },
//        }
//    }
//}
