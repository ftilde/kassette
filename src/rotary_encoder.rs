use rppal::gpio::{InputPin, Level, Pin};

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

#[must_use]
pub struct EventGuard {
    _event_pin: InputPin,
}

impl RotaryEncoder {
    pub fn new(event_pin: Pin, direction_pin: Pin) -> Self {
        let event_pin = event_pin.into_input();
        let direction_pin = direction_pin.into_input();

        RotaryEncoder {
            event_pin,
            direction_pin,
        }
    }

    pub fn start_events(
        self,
        debounce_time: Duration,
        mut event_handler: impl FnMut(RotaryEncoderEvent) + Send + 'static,
    ) -> EventGuard {
        let mut previous_time = std::time::Instant::now();
        let direction_pin = self.direction_pin;
        let mut event_pin = self.event_pin;

        event_pin
            .set_async_interrupt(rppal::gpio::Trigger::FallingEdge, move |level| {
                if level != Level::Low {
                    return;
                }
                let elapsed = previous_time.elapsed();
                if elapsed < debounce_time {
                    return;
                }
                previous_time = std::time::Instant::now();

                let event = match direction_pin.read() {
                    rppal::gpio::Level::Low => RotaryEncoderEvent::TurnLeft,
                    rppal::gpio::Level::High => RotaryEncoderEvent::TurnRight,
                };
                event_handler(event);
            })
            .unwrap();
        EventGuard {
            _event_pin: event_pin,
        }
    }
}
