use rppal::gpio::{OutputPin, Pin};

use std::time::Duration;

pub struct Led {
    output_pin: OutputPin,
}

pub enum LedCommand {
    Blink(Duration),
}

impl Led {
    pub fn new(output_pin: Pin) -> Self {
        let output_pin = output_pin.into_output();

        Led { output_pin }
    }

    fn on(&mut self) {
        self.output_pin.set_high();
    }

    fn off(&mut self) {
        self.output_pin.set_low();
    }

    pub fn execute(&mut self, cmd: LedCommand) {
        match cmd {
            LedCommand::Blink(len) => {
                self.on();
                std::thread::sleep(len);
                self.off();
            }
        }
    }
}
