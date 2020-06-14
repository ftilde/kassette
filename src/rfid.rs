use rfid_rs;
use rppal::gpio::{InputPin, Level, Pin};
use spidev;

use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, TrySendError};
use std::time::Duration;

#[derive(Copy, Clone, Debug)]
pub struct Uid(u64);

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
pub enum RfidEvent {
    Removed,
    Added(Uid),
}

#[derive(Debug)]
pub enum RfIdError {
    Rfid(rfid_rs::Error),
    Io(std::io::Error),
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

impl From<std::io::Error> for RfIdError {
    fn from(error: std::io::Error) -> Self {
        RfIdError::Io(error)
    }
}

pub struct RfidReader {
    mfrc: rfid_rs::MFRC522,
    _interrupt_pin: InputPin,
    interrupts: Receiver<()>,
}

impl RfidReader {
    pub fn new(device_path: impl AsRef<Path>, interrupt_pin: Pin) -> Result<Self, RfIdError> {
        let mut spi = spidev::Spidev::open(device_path)?;

        let mut options = spidev::SpidevOptions::new();
        let options = options.max_speed_hz(1_000_000);
        let options = options.mode(spidev::SpiModeFlags::SPI_MODE_0);
        spi.configure(&options)?;

        let mut mfrc = rfid_rs::MFRC522 { spi };

        mfrc.init()?;

        let mut interrupt_pin = interrupt_pin.into_input_pullup();

        let (sink, source) = sync_channel(1);

        interrupt_pin.set_async_interrupt(rppal::gpio::Trigger::FallingEdge, move |level| {
            if level != Level::Low {
                return;
            }
            match sink.try_send(()) {
                Ok(_) => {}
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => {
                    panic!("Pipe disconnected before interrupt func cleared!")
                }
            }
        })?;

        Ok(RfidReader {
            mfrc,
            _interrupt_pin: interrupt_pin,
            interrupts: source,
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
        // Clear previous interrupt
        let _ = self.interrupts.try_recv();

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
        match self.interrupts.recv_timeout(check_timeout) {
            Ok(()) => Ok(true),
            Err(RecvTimeoutError::Timeout) => Ok(false),
            Err(_) => panic!("Channel should only be dropped on object destruction!"),
        }
    }

    pub fn events(&mut self, check_timeout: Duration) -> impl Iterator<Item = RfidEvent> + '_ {
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
