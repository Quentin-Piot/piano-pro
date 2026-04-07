use std::path::PathBuf;

use crate::{
    scene::menu_scene::{MsgFn, on_async},
    song::Song,
    utils::BoxFuture,
};

use super::{UiState, state::Page};
use neothesia_core::config::MidiEntryV1;

#[derive(Debug, Clone)]
pub struct PendingImport {
    pub stored_path: PathBuf,
    pub entry: MidiEntryV1,
}

pub fn open_midi_file_picker(data: &mut UiState) -> BoxFuture<MsgFn> {
    data.is_loading = true;
    on_async(open_midi_file_picker_fut(), |pending, data, ctx| {
        if let Some(import) = pending {
            // Store in UiState for MenuScene to pick up
            data.pending_import = Some(import);
            // Request focus back to the window after file dialog closes
            ctx.window.focus_window();
        }
        data.is_loading = false;
    })
}

pub fn load_from_library(stored_name: String) -> BoxFuture<MsgFn> {
    on_async(
        load_from_library_fut(stored_name.clone()),
        move |res, data, ctx| {
            if let Some((midi, path)) = res {
                ctx.config.set_last_opened_song(Some(path));
                ctx.config.save();
                let display_name = ctx.config.lookup_display_name(&stored_name);

                let song = if let Some(name) = display_name {
                    Song::with_display_name(midi, name)
                } else {
                    Song::new(midi)
                };
                data.song = Some(song);
                data.go_to(Page::PlayConfirm);
            }
        },
    )
}

async fn load_from_library_fut(stored_name: String) -> Option<(midi_file::MidiFile, PathBuf)> {
    let lib_dir = neothesia_core::utils::resources::midi_library_dir()?;
    let file_path = lib_dir.join(&stored_name);

    let thread = crate::utils::task::thread::spawn("midi-loader".into(), move || {
        let midi = midi_file::MidiFile::new(&file_path);

        if let Err(e) = &midi {
            log::error!("{e}");
        }

        midi.ok().map(|midi| (midi, file_path))
    });

    thread.join().await.ok().flatten()
}

async fn open_midi_file_picker_fut() -> Option<PendingImport> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("midi", &["mid", "midi"])
        .pick_file()
        .await;

    if let Some(file) = file {
        log::info!("File path = {:?}", file.path());

        let thread = crate::utils::task::thread::spawn("midi-loader".into(), move || {
            let original_path = file.path().to_path_buf();
            let file_stem = original_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled");

            let _midi = midi_file::MidiFile::new(&original_path);
            if let Err(e) = &_midi {
                log::error!("{e}");
                return None;
            }

            let lib_dir = neothesia_core::utils::resources::midi_library_dir()?;
            if std::fs::create_dir_all(&lib_dir).is_err() {
                log::error!("Failed to create library directory");
                return None;
            }

            let display_name = midi_file::extract_midi_metadata(&original_path)
                .unwrap_or_else(|| file_stem.to_string());
            let entry = MidiEntryV1::new(display_name, file_stem.to_string());
            let stored_path = lib_dir.join(&entry.stored_name);

            if std::fs::copy(&original_path, &stored_path).is_err() {
                log::error!("Failed to copy MIDI file to library");
                return None;
            }

            Some(PendingImport { stored_path, entry })
        });

        thread.join().await.ok().flatten()
    } else {
        log::info!("User canceled dialog");
        None
    }
}
