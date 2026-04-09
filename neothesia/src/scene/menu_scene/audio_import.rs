use std::path::{Path, PathBuf};

use crate::{
    scene::menu_scene::{MsgFn, on_async},
    song::Song,
    utils::BoxFuture,
};

use super::{UiState, state::Page};
use neothesia_core::config::MidiEntryV1;

#[derive(Debug, Clone)]
pub enum AudioImportState {
    Empty,
    Selected {
        path: PathBuf,
    },
    Converting {
        path: PathBuf,
    },
    Error {
        path: Option<PathBuf>,
        message: String,
    },
}

impl Default for AudioImportState {
    fn default() -> Self {
        Self::Empty
    }
}

impl AudioImportState {
    pub fn selected_path(&self) -> Option<&Path> {
        match self {
            Self::Selected { path } | Self::Converting { path } => Some(path),
            Self::Error { path, .. } => path.as_deref(),
            Self::Empty => None,
        }
    }

    pub fn is_converting(&self) -> bool {
        matches!(self, Self::Converting { .. })
    }
}

pub struct ConvertedAudioImport {
    pub stored_path: PathBuf,
    pub entry: MidiEntryV1,
    pub midi: midi_file::MidiFile,
}

pub fn open_audio_file_picker(data: &mut UiState) -> BoxFuture<MsgFn> {
    data.is_loading = true;
    on_async(open_audio_file_picker_fut(), |path, data, ctx| {
        if let Some(path) = path {
            data.audio_import = AudioImportState::Selected { path };
            data.go_to(Page::AudioImport);
            ctx.window.focus_window();
        }
        data.is_loading = false;
    })
}

pub fn convert_selected_audio(data: &mut UiState) -> Option<BoxFuture<MsgFn>> {
    let path = data.audio_import.selected_path()?.to_path_buf();
    data.audio_import = AudioImportState::Converting { path: path.clone() };

    Some(on_async(
        convert_audio_file_fut(path.clone()),
        move |result, data, ctx| match result {
            Ok(converted) => {
                ctx.config.add_midi_to_library(converted.entry.clone());
                ctx.config.set_last_opened_song(Some(converted.stored_path));
                ctx.config.save();

                data.song = Some(Song::with_display_name(
                    converted.midi,
                    converted.entry.display_name.clone(),
                ));
                // Navigation is locked while converting (Back/Escape disabled),
                // so current page is always AudioImport at this point.
                data.audio_import = AudioImportState::Empty;
                data.go_back(); // pop AudioImport
                data.go_to(Page::PlayConfirm);
                ctx.window.focus_window();
            }
            Err(message) => {
                log::error!("Audio import failed: {message}");
                data.audio_import = AudioImportState::Error {
                    path: Some(path),
                    message,
                };
                ctx.window.focus_window();
            }
        },
    ))
}

#[cfg(not(target_arch = "wasm32"))]
async fn open_audio_file_picker_fut() -> Option<PathBuf> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("audio", &["wav", "mp3"])
        .pick_file()
        .await;

    match file {
        Some(file) => Some(file.path().to_path_buf()),
        None => {
            log::info!("User canceled audio import dialog");
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
async fn open_audio_file_picker_fut() -> Option<PathBuf> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
async fn convert_audio_file_fut(path: PathBuf) -> Result<ConvertedAudioImport, String> {
    let thread = crate::utils::task::thread::spawn("audio-import".into(), move || {
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio-import")
            .to_string();

        let display_name = file_stem.clone();
        let mut entry = MidiEntryV1::new(display_name, file_stem);

        let lib_dir = neothesia_core::utils::resources::midi_library_dir()
            .ok_or_else(|| "MIDI library directory is unavailable".to_string())?;
        std::fs::create_dir_all(&lib_dir)
            .map_err(|err| format!("Failed to create MIDI library directory: {err}"))?;

        let mut stored_path = lib_dir.join(&entry.stored_name);
        let stored_stem = entry
            .stored_name
            .strip_suffix(".mid")
            .unwrap_or(&entry.stored_name)
            .to_string();
        let mut suffix = 1;
        while stored_path.exists() {
            entry.stored_name = format!("{stored_stem}_{suffix}.mid");
            stored_path = lib_dir.join(&entry.stored_name);
            suffix += 1;
        }

        let midi = neothesia_ai::transcribe_audio_to_midi(&path)
            .map_err(|err| format!("Failed to convert audio to MIDI: {err}"))?;
        midi.save(&stored_path)
            .map_err(|err| format!("Failed to save generated MIDI: {err}"))?;

        let midi = midi_file::MidiFile::new(&stored_path)
            .map_err(|err| format!("Failed to load generated MIDI: {err}"))?;

        Ok(ConvertedAudioImport {
            stored_path,
            entry,
            midi,
        })
    });

    thread
        .join()
        .await
        .map_err(|_| "Audio import worker panicked".to_string())?
}

#[cfg(target_arch = "wasm32")]
async fn convert_audio_file_fut(_path: PathBuf) -> Result<ConvertedAudioImport, String> {
    Err("Audio import is not available on web".to_string())
}
