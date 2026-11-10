//! Music rejection (docs/design/speakers.md, "Music"): the worker's sound classifier (CED-tiny,
//! AudioSet) scores each event, and an event with music or singing but no speech is kept as a
//! collapsed Music row instead of being transcribed.
//!
//! Thresholds from the 2026-11-10 calibration (Wikimedia Commons music: solo piano, a cappella
//! choir, a sung anthem, a radio jingle; AMI meetings and a NASA podcast for speech), on 3 s and
//! 10 s windows: speech never scored music or singing above 0.04; music, choir and anthem never
//! below 0.22 (choir) and never above 0.06 for speech; speech over music scored speech 0.44+.

use super::protocol::Sound;

/// Below this, there's no speech worth transcribing in the event.
pub const SPEECH_BELOW: f32 = 0.2;
/// At or above this for music or singing, the event is music.
pub const MUSIC_AT_LEAST: f32 = 0.2;

/// Music or singing with no speech. Speech over music (a podcast's backing track) is speech.
pub fn is_music(sound: &Sound) -> bool {
    sound.speech < SPEECH_BELOW && sound.music.max(sound.singing) >= MUSIC_AT_LEAST
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(speech: f32, music: f32, singing: f32) -> Sound {
        Sound { speech, music, singing }
    }

    #[test]
    fn calibration_cases() {
        assert!(is_music(&s(0.01, 0.83, 0.0)), "solo piano");
        assert!(is_music(&s(0.00, 0.27, 0.14)), "a cappella choir, weakest window");
        assert!(is_music(&s(0.01, 0.84, 0.03)), "sung anthem");
        assert!(!is_music(&s(0.64, 0.63, 0.0)), "speech over music stays speech");
        assert!(!is_music(&s(0.44, 0.73, 0.0)), "quieter speech over louder music stays speech");
        assert!(!is_music(&s(0.81, 0.00, 0.0)), "meeting speech");
        assert!(!is_music(&s(0.01, 0.04, 0.0)), "a pause in speech, no music");
    }
}
