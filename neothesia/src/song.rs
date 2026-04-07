use midi_file::MidiTrack;

use crate::context::Context;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PlayerConfig {
    Mute,
    Auto,
    Human,
}

#[derive(Debug, Clone)]
pub struct TrackConfig {
    pub track_id: usize,
    pub player: PlayerConfig,
    pub visible: bool,
}

#[derive(Default, Debug, Clone)]
pub struct SongConfig {
    pub tracks: Box<[TrackConfig]>,
}

impl SongConfig {
    fn new(tracks: &[MidiTrack]) -> Self {
        let tracks: Vec<_> = tracks
            .iter()
            .map(|t| {
                let is_drums = t.has_drums && !t.has_other_than_drums;
                TrackConfig {
                    track_id: t.track_id,
                    player: PlayerConfig::Auto,
                    visible: !is_drums,
                }
            })
            .collect();
        Self {
            tracks: tracks.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Song {
    pub file: midi_file::MidiFile,
    pub config: SongConfig,
    pub display_name: Option<String>,
}

impl Song {
    pub fn new(file: midi_file::MidiFile) -> Self {
        let config = SongConfig::new(&file.tracks);
        Self {
            file,
            config,
            display_name: None,
        }
    }

    pub fn with_display_name(file: midi_file::MidiFile, display_name: String) -> Self {
        let config = SongConfig::new(&file.tracks);
        Self {
            file,
            config,
            display_name: Some(display_name),
        }
    }

    pub fn from_env(ctx: &Context) -> Option<Self> {
        let args: Vec<String> = std::env::args().collect();

        let (midi_file, display_name) = if args.len() > 1 {
            (midi_file::MidiFile::new(&args[1]).ok(), None)
        } else if let Some(last_path) = ctx.config.last_opened_song() {
            let midi = midi_file::MidiFile::new(&last_path).ok();
            let display = last_path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|stored_name| ctx.config.lookup_display_name(stored_name));
            (midi, display)
        } else {
            (None, None)
        };

        let midi = midi_file?;

        Some(if let Some(name) = display_name {
            Self::with_display_name(midi, name)
        } else {
            Self::new(midi)
        })
    }
}
