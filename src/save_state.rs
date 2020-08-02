use crate::player::{PlaybackPos, Volume};
use crate::rfid::Uid;
use miniserde::{json, Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

#[derive(Serialize, Deserialize)]
struct SerPlaybackState {
    uid: u32,
    playback_pos: u64,
    stop_time: u64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct SaveState {
    playback_state: Option<SerPlaybackState>,
    volume: Volume,
}

impl SaveState {
    pub fn load(f: impl AsRef<Path>) -> Option<Self> {
        let mut f = File::open(f).ok()?;
        let mut buf = String::new();
        f.read_to_string(&mut buf).ok()?;
        json::from_str(&buf).ok()
    }
    pub fn save(&self, f: impl AsRef<Path>) -> std::io::Result<()> {
        let mut f = File::create(f)?;
        let buf = json::to_string(self);
        f.write_all(buf.as_bytes())?;
        Ok(())
    }
    pub fn playback_state(&self) -> Option<(Uid, PlaybackPos, SystemTime)> {
        self.playback_state.as_ref().map(|sbp| {
            (
                Uid(sbp.uid),
                PlaybackPos::from_millis(sbp.playback_pos),
                SystemTime::UNIX_EPOCH + Duration::from_millis(sbp.stop_time),
            )
        })
    }
    pub fn set_playback_state(&mut self, playback_pos: Option<(Uid, PlaybackPos, SystemTime)>) {
        self.playback_state = playback_pos.map(|(uid, playback_pos, stop_time)| SerPlaybackState {
            uid: uid.0,
            playback_pos: playback_pos.as_millis() as u64,
            stop_time: stop_time
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        })
    }

    pub fn volume(&self) -> Volume {
        self.volume
    }

    pub fn set_volume(&mut self, vol: Volume) {
        self.volume = vol;
    }
}
