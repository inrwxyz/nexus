//! Command normalization, intent parsing, and Windows/media execution pipeline.

mod media_cmds;
mod parser;
mod window_cmds;

use std::collections::HashMap;

use crate::app_launcher::App;
use crate::vocab::Vocab;
use crate::voice;
use crate::window_mgr::is_app_open;

pub use parser::{is_timer_command, media_nav_intent, parse, Parsed};

pub fn execute_single_command(
    raw_cmd: &str,
    vocab: &Vocab,
    apps: &mut [App],
    aliases: &mut HashMap<String, String>,
    voice_inst: Option<&voice::Voice>,
) -> bool {
    let line = raw_cmd.trim();
    if line.is_empty() {
        return false;
    }
    let lnorm = parser::normalize_command(line);

    if let Some(ok) = window_cmds::try_system_command(&lnorm) {
        return ok;
    }

    if let Some(ok) = window_cmds::try_scroll_minimize_close(&lnorm) {
        return ok;
    }

    if let Some(ok) = media_cmds::try_video_pick(&lnorm, apps) {
        return ok;
    }

    if let Some(ok) = window_cmds::try_focus_window(&lnorm) {
        return ok;
    }

    let youtube_open = is_app_open("youtube");
    let spotify_open = is_app_open("spotify");

    if let Some(ok) = media_cmds::try_fullscreen_and_transport(&lnorm, youtube_open, spotify_open) {
        return ok;
    }

    if let Some(ok) = media_cmds::try_search_and_play(&lnorm, apps, youtube_open, spotify_open) {
        return ok;
    }

    window_cmds::try_app_and_vocab_command(line, &lnorm, vocab, apps, aliases, voice_inst)
}

pub fn execute_command(
    line: &str,
    vocab: &Vocab,
    apps: &mut [App],
    aliases: &mut HashMap<String, String>,
    voice_inst: Option<&voice::Voice>,
) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    execute_single_command(line, vocab, apps, aliases, voice_inst)
}
