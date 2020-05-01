//use rfid_rs;
//use spidev;

use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
use cpal::{SampleFormat, StreamData, UnknownTypeOutputBuffer};

#[no_mangle]
pub unsafe extern "C" fn __nanosleep_time64(
    rqtp: *const libc::timespec,
    rmtp: *mut libc::timespec,
) -> libc::c_int {
    libc::nanosleep(rqtp, rmtp)
}

#[no_mangle]
pub unsafe extern "C" fn __gettimeofday_time64(
    tp: *mut libc::timeval,
    tz: *mut core::ffi::c_void,
) -> libc::c_int {
    libc::gettimeofday(tp, tz)
}

#[no_mangle]
pub unsafe extern "C" fn __clock_gettime64(
    clk_id: libc::clockid_t,
    tp: *mut libc::timespec,
) -> libc::c_int {
    libc::clock_gettime(clk_id, tp)
}

#[no_mangle]
pub unsafe extern "C" fn __dlsym_time64(
    handle: *mut libc::c_void,
    symbol: *const libc::c_char,
) -> *mut core::ffi::c_void {
    libc::dlsym(handle, symbol)
}

#[no_mangle]
pub unsafe extern "C" fn __stat_time64(
    path: *const libc::c_char,
    buf: *mut libc::stat64,
) -> libc::c_int {
    libc::stat64(path, buf)
}

fn main() {
    let host = cpal::default_host();
    let event_loop = host.event_loop();

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
    //let sample_rate = format.sample_rate.0;
    //let initial_bpm = 60.0;

    let stream_id = event_loop.build_output_stream(&device, &format).unwrap();

    event_loop.play_stream(stream_id).unwrap();

    std::thread::Builder::new()
        .name("renderer".to_owned())
        .spawn(move || {
            event_loop.run(move |_stream_id, stream_data| {
                if let StreamData::Output {
                    buffer: UnknownTypeOutputBuffer::F32(mut fbuffer),
                } = stream_data.unwrap()
                {
                    for elm in fbuffer.iter_mut() {
                        *elm = 0.0;
                    }
                } else {
                    panic!("Invalid format");
                }
            });
        })
        .unwrap();
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
