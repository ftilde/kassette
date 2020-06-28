use rppal::gpio::{InputPin, Level, Pin};
use std::sync::Mutex;
use std::sync::{Arc, Weak};

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
    _state: Arc<Mutex<State>>,
}

struct State {
    p1: InputPin,
    p2: InputPin,
    event_handler: Box<dyn FnMut(RotaryEncoderEvent) + Send + 'static>,
    transition: usize,
    last_valid_transitions: usize,
}

fn update_state(state: &Weak<Mutex<State>>) {
    let state = if let Some(state) = state.upgrade() {
        state
    } else {
        return;
    };
    let mut state = if let Ok(state) = state.try_lock() {
        state
    } else {
        return;
    };

    let l1 = state.p1.read();
    let l2 = state.p2.read();

    // Kudos to https://www.best-microcontroller-projects.com/rotary-encoder.html for a proper
    // debouncing technique!

    state.transition <<= 2;
    if l1 == Level::High {
        state.transition |= 1;
    }
    if l2 == Level::High {
        state.transition |= 2;
    }
    state.transition &= 0xf;

    const VALID_TRANSITION_TABLE: [u8; 16] = [0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0];
    if VALID_TRANSITION_TABLE[state.transition] == 1 {
        state.last_valid_transitions <<= 4;
        state.last_valid_transitions |= state.transition;
        state.last_valid_transitions &= 0xff;
        let event = match state.last_valid_transitions {
            0b0010_1011 => Some(RotaryEncoderEvent::TurnLeft), // First p2 pos edge, then p1 pos edge
            0b0001_0111 => Some(RotaryEncoderEvent::TurnRight), // First p1 pos edge, then p2 pos edge
            _ => None,
        };

        if let Some(event) = event {
            (&mut state.event_handler)(event);
        }
    }
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
        event_handler: impl FnMut(RotaryEncoderEvent) + Send + 'static,
    ) -> EventGuard {
        let state = Arc::new(Mutex::new(State {
            p1: self.event_pin,
            p2: self.direction_pin,
            event_handler: Box::new(event_handler),
            transition: 0,
            last_valid_transitions: 0,
        }));

        let s1 = Arc::downgrade(&state);
        let s2 = Arc::downgrade(&state);

        {
            let mut state = state.lock().unwrap();

            state
                .p1
                .set_async_interrupt(rppal::gpio::Trigger::Both, move |_| update_state(&s1))
                .unwrap();
            state
                .p2
                .set_async_interrupt(rppal::gpio::Trigger::Both, move |_| update_state(&s2))
                .unwrap();
        }
        EventGuard { _state: state }
    }
}
