macro_rules! log {
    ($fmtstr:expr, $($arg:tt)+) => ({
        use std::io::Write;
        let mut l = crate::log::LOGGER.lock().unwrap(); // Lock shouldn't be poisened.
        let l = l.as_mut().unwrap(); // We only log between init_logger and deinit_logger

        println!(std::concat!("{:?}: ", $fmtstr), l.0.elapsed(), $($arg)+);

        let _ = writeln!(l.1, std::concat!("{:?}: ", $fmtstr), l.0.elapsed(), $($arg)+);
        let _ = l.1.flush();
    });
    ($fmtstr:expr) => ({
        use std::io::Write;
        let mut l = crate::log::LOGGER.lock().unwrap(); // Lock shouldn't be poisened.
        let l = l.as_mut().unwrap(); // We only log between init_logger and deinit_logger

        println!(std::concat!("{:?}: ", $fmtstr), l.0.elapsed());

        let _ = writeln!(l.1, std::concat!("{:?}: ", $fmtstr), l.0.elapsed());
        let _ = l.1.flush();
    });
}

macro_rules! log_err {
    ($msg:expr, $e:expr) => {{
        if let Err(e) = $e {
            log!("{}: {:?}", $msg, e);
        }
    }};
}

use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::Instant;
pub static LOGGER: Lazy<Mutex<Option<(Instant, std::fs::File)>>> = Lazy::new(|| Mutex::new(None));

pub fn init_logger(log_file_path: impl AsRef<std::path::Path>) {
    match std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_file_path)
    {
        Ok(f) => {
            let mut l = LOGGER.lock().unwrap();
            *l = Some((Instant::now(), f));
        }
        Err(e) => {
            panic!("Unable to initialize logger: {}", e);
        }
    }
}

pub fn deinit_logger() {
    let mut l = LOGGER.lock().unwrap();
    *l = None;
}
