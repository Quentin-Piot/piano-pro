use std::path::PathBuf;

use crate::{
    scene::menu_scene::{MsgFn, on_async},
    song::Song,
    utils::BoxFuture,
};

use super::{UiState, state::Page};
use neothesia_core::config::MidiEntryV1;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
const WEB_MIDI_STORAGE_PREFIX: &str = "pianopro.midi.";

#[cfg(target_arch = "wasm32")]
thread_local! {
    // In-memory store: stored_name -> raw MIDI bytes
    static WEB_MIDI_STORE: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    // Result slot for the async file picker: None = not ready, Some(v) = done
    static WEB_IMPORT_RESULT: RefCell<Option<Option<PendingImport>>> = const { RefCell::new(None) };
}

#[cfg(target_arch = "wasm32")]
fn store_midi(stored_name: &str, bytes: Vec<u8>) {
    WEB_MIDI_STORE.with(|store| {
        store
            .borrow_mut()
            .insert(stored_name.to_string(), bytes.clone());
    });

    persist_midi(stored_name, &bytes);
}

#[cfg(target_arch = "wasm32")]
fn load_midi(stored_name: &str) -> Option<Vec<u8>> {
    let cached = WEB_MIDI_STORE.with(|store| store.borrow().get(stored_name).cloned());
    if cached.is_some() {
        return cached;
    }

    let bytes = load_persisted_midi(stored_name)?;
    WEB_MIDI_STORE.with(|store| {
        store
            .borrow_mut()
            .insert(stored_name.to_string(), bytes.clone());
    });
    Some(bytes)
}

#[cfg(target_arch = "wasm32")]
pub fn remove_web_midi(stored_name: &str) {
    WEB_MIDI_STORE.with(|store| {
        store.borrow_mut().remove(stored_name);
    });

    if let Some(storage) = local_storage() {
        let _ = storage.remove_item(&midi_storage_key(stored_name));
    }
}

#[cfg(target_arch = "wasm32")]
fn persist_midi(stored_name: &str, bytes: &[u8]) {
    let Some(storage) = local_storage() else {
        return;
    };

    let serialized = match serde_json::to_string(bytes) {
        Ok(serialized) => serialized,
        Err(err) => {
            log::error!("Failed to serialize web MIDI bytes: {err}");
            return;
        }
    };

    if let Err(err) = storage.set_item(&midi_storage_key(stored_name), &serialized) {
        log::error!("Failed to persist web MIDI bytes for {stored_name}: {err:?}");
    }
}

#[cfg(target_arch = "wasm32")]
fn load_persisted_midi(stored_name: &str) -> Option<Vec<u8>> {
    let storage = local_storage()?;
    let raw = storage
        .get_item(&midi_storage_key(stored_name))
        .ok()
        .flatten()?;

    match serde_json::from_str(&raw) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            log::error!("Failed to parse persisted web MIDI bytes for {stored_name}: {err}");
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn midi_storage_key(stored_name: &str) -> String {
    format!("{WEB_MIDI_STORAGE_PREFIX}{stored_name}")
}

#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|window| window.local_storage().ok().flatten())
}

/// Future that polls the WEB_IMPORT_RESULT thread-local each frame.
/// Returns Pending until the spawn_local file-picker task fills the slot.
#[cfg(target_arch = "wasm32")]
struct WasmImportFuture;

#[cfg(target_arch = "wasm32")]
impl std::future::Future for WasmImportFuture {
    type Output = MsgFn;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<MsgFn> {
        let has_result = WEB_IMPORT_RESULT.with(|c| c.borrow().is_some());
        if !has_result {
            return std::task::Poll::Pending;
        }
        let import = WEB_IMPORT_RESULT.with(|c| c.borrow_mut().take().flatten());
        std::task::Poll::Ready(Box::new(
            move |data: &mut UiState, ctx: &mut crate::context::Context| {
                if let Some(import) = import {
                    data.pending_import = Some(import);
                    ctx.window.focus_window();
                }
                data.is_loading = false;
            },
        ))
    }
}

// SAFETY: WASM is single-threaded; Send is vacuously satisfied.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for WasmImportFuture {}

#[derive(Debug, Clone)]
pub struct PendingImport {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub stored_path: PathBuf,
    pub entry: MidiEntryV1,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_midi_file_picker(data: &mut UiState) -> BoxFuture<MsgFn> {
    data.is_loading = true;
    on_async(open_midi_file_picker_fut(), |pending, data, ctx| {
        if let Some(import) = pending {
            data.pending_import = Some(import);
            ctx.window.focus_window();
        }
        data.is_loading = false;
    })
}

#[cfg(target_arch = "wasm32")]
pub fn open_midi_file_picker(data: &mut UiState) -> BoxFuture<MsgFn> {
    data.is_loading = true;

    // Reset the result slot before launching a new pick
    WEB_IMPORT_RESULT.with(|c| *c.borrow_mut() = None);

    // Launch the non-Send rfd future on the JS microtask queue
    wasm_bindgen_futures::spawn_local(async {
        let result = open_midi_file_picker_fut().await;
        WEB_IMPORT_RESULT.with(|c| *c.borrow_mut() = Some(result));
    });

    // Return a polling future that reads the result slot each frame
    Box::pin(WasmImportFuture)
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

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
async fn load_from_library_fut(stored_name: String) -> Option<(midi_file::MidiFile, PathBuf)> {
    let bytes = load_midi(&stored_name)?;
    let midi = midi_file::MidiFile::from_bytes(&stored_name, &bytes).ok()?;
    Some((midi, PathBuf::from(&stored_name)))
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
async fn open_midi_file_picker_fut() -> Option<PendingImport> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("midi", &["mid", "midi"])
        .pick_file()
        .await?;

    let file_name = file.file_name();
    let bytes = file.read().await;

    let display_name = midi_file::extract_midi_metadata_from_bytes(&file_name, &bytes)
        .unwrap_or_else(|| {
            std::path::Path::new(&file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

    midi_file::MidiFile::from_bytes(&file_name, &bytes)
        .map_err(|e| log::error!("Invalid MIDI file: {e}"))
        .ok()?;

    let stem = file_name
        .strip_suffix(".mid")
        .or_else(|| file_name.strip_suffix(".midi"))
        .unwrap_or(&file_name);
    let entry = MidiEntryV1::new(display_name, stem.to_string());
    let stored_path = PathBuf::from(&entry.stored_name);

    store_midi(&entry.stored_name, bytes);

    Some(PendingImport { stored_path, entry })
}
