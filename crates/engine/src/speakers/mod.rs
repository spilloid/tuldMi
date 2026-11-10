//! Remote speaker separation (docs/design/speakers.md): the `tuldmi-speakers` worker finds who
//! spoke when inside each system-audio event, `tracker` links those voices across the
//! conversation (in memory only), and `assign` gives transcript segments to speakers.

pub mod assign;
pub mod protocol;
pub mod tracker;
pub mod worker;

/// Label of the Nth remote voice shown in a conversation.
pub fn numbered_label(number: u32) -> String {
    format!("Speaker {number}")
}
