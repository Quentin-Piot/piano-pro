pub mod context;
pub mod icons;
pub mod input_manager;
pub mod output_manager;
pub mod scene;
pub mod song;
pub mod utils;

use midi_file::midly::MidiMessage;

// Re-export so submodules can use `crate::render` and `crate::config`
use neothesia_core::{config, render};
use wgpu_jumpstart::TransformUniform;

#[derive(Debug)]
pub enum NeothesiaEvent {
    /// Go to playing scene
    Play(song::Song),
    FreePlay(Option<song::Song>),
    /// Go to main menu scene
    MainMenu(Option<song::Song>),
    MidiInput {
        channel: u8,
        message: MidiMessage,
    },
    Exit,
}

pub use context::Context;
pub use scene::{Scene, freeplay, menu_scene, playing_scene};
pub use song::Song;
