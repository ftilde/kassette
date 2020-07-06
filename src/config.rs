use core::time::Duration;

pub const MIN_TIME_FOR_CONTEXT: Duration = Duration::from_secs(10);
pub const MAX_CONTEXT_TIME: Duration = Duration::from_secs(60);
pub const PAUSE_TO_CONTEXT_RATION: u32 = 10;
pub const SAVESTATE_PATH: &str = "savestate.json";
pub const DEFAULT_VOLUME: u8 = 11;
pub const FADE_TIME: Duration = Duration::from_millis(500);
pub const AUDIO_BUF_SIZE: Duration = Duration::from_millis(100);
