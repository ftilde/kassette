use rppal::gpio::{Gpio, InputPin};

use std::time::Duration;

#[derive(Copy, Clone, Debug)]
pub enum RotaryEncoderEvent {
    TurnLeft,
    TurnRight,
}

pub struct RotaryEncoder {
    event_pin: InputPin,
    direction_pin: InputPin,
}

impl RotaryEncoder {
    pub fn new(gpio: &Gpio) -> Result<Self, rppal::gpio::Error> {
        let event_pin = gpio
            .get(crate::pins::ROTARY_ENCODER_EVENT)
            .unwrap()
            .into_input();
        let direction_pin = gpio
            .get(crate::pins::ROTARY_ENCODER_DIRECTION)
            .unwrap()
            .into_input();

        Ok(RotaryEncoder {
            event_pin,
            direction_pin,
        })
    }

    fn wait_for_event(&mut self) -> Result<(), rppal::gpio::Error> {
        loop {
            self.event_pin
                .set_interrupt(rppal::gpio::Trigger::FallingEdge)?;
            if let Some(rppal::gpio::Level::Low) = self.event_pin.poll_interrupt(true, None)? {
                return Ok(());
            }
        }
    }

    pub fn events(
        &mut self,
        debounce_time: Duration,
    ) -> impl Iterator<Item = RotaryEncoderEvent> + '_ {
        let mut previous_time = std::time::Instant::now();

        std::iter::from_fn(move || loop {
            if self.wait_for_event().is_err() {
                continue;
            }
            let elapsed = previous_time.elapsed();
            if elapsed < debounce_time {
                continue;
            }
            previous_time = std::time::Instant::now();

            match self.direction_pin.read() {
                rppal::gpio::Level::Low => break Some(RotaryEncoderEvent::TurnLeft),
                rppal::gpio::Level::High => break Some(RotaryEncoderEvent::TurnRight),
            }
        })
    }
}
