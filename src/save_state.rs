use crate::player::Volume;
use crate::rfid::Uid;
use miniserde::{json, Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

#[derive(Serialize, Deserialize, Default)]
pub struct SaveState {
    playback_pos: Option<(u64, u64)>,
    volume: Volume,
}

impl SaveState {
    pub fn load(f: impl AsRef<Path>) -> Option<Self> {
        let mut f = File::open(f).ok()?;
        let mut buf = String::new();
        f.read_to_string(&mut buf).ok()?;
        json::from_str(&buf).ok()
    }
    pub fn save(&self, f: impl AsRef<Path>) {
        let mut f = File::create(f).unwrap();
        let buf = json::to_string(self);
        f.write_all(buf.as_bytes()).unwrap();
    }
    pub fn playback_pos(&self) -> Option<(Uid, Duration)> {
        self.playback_pos
            .map(|(uid, duration)| (Uid(uid), Duration::from_millis(duration)))
    }
    pub fn set_playback_pos(&mut self, playback_pos: Option<(Uid, Duration)>) {
        self.playback_pos = playback_pos.map(|(uid, duration)| (uid.0, duration.as_millis() as u64))
    }

    pub fn volume(&self) -> Volume {
        self.volume
    }

    pub fn set_volume(&mut self, vol: Volume) {
        self.volume = vol;
    }
}
