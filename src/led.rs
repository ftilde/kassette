use rppal::gpio::{Gpio, OutputPin};

use std::time::Duration;

pub struct Led {
    output_pin: OutputPin,
}

pub enum LedCommand {
    Blink(Duration),
}

impl Led {
    pub fn new(gpio: &Gpio) -> Result<Self, rppal::gpio::Error> {
        let output_pin = gpio.get(crate::pins::LED_OUTPUT_PIN).unwrap().into_output();

        Ok(Led { output_pin })
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
