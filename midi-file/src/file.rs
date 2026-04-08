use crate::{MidiTrack, program_track::ProgramTrack, tempo_track::TempoTrack};
use midly::{Format, MetaMessage, Smf, Timing};
use std::{fs, path::Path, sync::Arc};

#[derive(Debug, Clone)]
pub struct MidiFile {
    pub name: String,
    pub format: Format,
    pub tracks: Arc<[MidiTrack]>,
    pub program_track: ProgramTrack,
    pub tempo_track: TempoTrack,
    pub measures: Arc<[std::time::Duration]>,
}

impl MidiFile {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let name = path
            .as_ref()
            .file_name()
            .ok_or(String::from("File not found"))?
            .to_string_lossy()
            .to_string();

        let data = match fs::read(path) {
            Ok(buff) => buff,
            Err(_) => return Err(String::from("Could Not Open File")),
        };

        Self::from_bytes(name, &data)
    }

    pub fn from_bytes(name: impl Into<String>, data: &[u8]) -> Result<Self, String> {
        let name = name.into();
        let smf = Smf::parse(data).map_err(|_| String::from("Midi Parsing Error (midly lib)"))?;

        let u_per_quarter_note: u16 = match smf.header.timing {
            Timing::Metrical(t) => t.as_int(),
            Timing::Timecode(_fps, _u) => {
                return Err(String::from("Midi With Timecode Timing, Not Supported!"));
            }
        };

        if smf.tracks.is_empty() {
            return Err(String::from("Midi File Has No Tracks"));
        }

        let tempo_track = TempoTrack::build(&smf.tracks, u_per_quarter_note);

        let mut track_color_id = 0;
        let tracks: Vec<MidiTrack> = smf
            .tracks
            .iter()
            .enumerate()
            .map(|(id, events)| {
                let track = MidiTrack::new(id, track_color_id, &tempo_track, events);

                if !track.notes.is_empty() {
                    track_color_id += 1;
                }

                track
            })
            .collect();

        let measures = {
            let last_note_end = tracks
                .iter()
                .fold(std::time::Duration::ZERO, |last, track| {
                    if let Some(note) = track.notes.last() {
                        last.max(note.start + note.duration)
                    } else {
                        last
                    }
                });

            let mut masures = Vec::new();
            let mut time = std::time::Duration::ZERO;
            let mut id = 0;
            while time <= last_note_end {
                time = tempo_track.pulses_to_duration(id * u_per_quarter_note as u64 * 4);
                masures.push(time);
                id += 1;
            }

            masures
        };

        let program_track = ProgramTrack::new(&tracks);

        Ok(Self {
            name,
            format: smf.header.format,
            tracks: tracks.into(),
            program_track,
            tempo_track,
            measures: measures.into(),
        })
    }
}

pub fn extract_midi_metadata(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;
    let smf = Smf::parse(&data).ok()?;

    let mut track_name: Option<String> = None;
    let mut text: Option<String> = None;
    let mut instrument_name: Option<String> = None;

    for track in &smf.tracks {
        for event in track.iter() {
            if let midly::TrackEventKind::Meta(msg) = &event.kind {
                match msg {
                    MetaMessage::TrackName(data) => {
                        let s = String::from_utf8_lossy(data).trim().to_string();
                        if !s.is_empty() && track_name.is_none() {
                            track_name = Some(s);
                        }
                    }
                    MetaMessage::Text(data) => {
                        let s = String::from_utf8_lossy(data).trim().to_string();
                        if !s.is_empty() && text.is_none() {
                            text = Some(s);
                        }
                    }
                    MetaMessage::InstrumentName(data) => {
                        let s = String::from_utf8_lossy(data).trim().to_string();
                        if !s.is_empty() && instrument_name.is_none() {
                            instrument_name = Some(s);
                        }
                    }
                    _ => {}
                }
            }
        }

        if track_name.is_some() {
            break;
        }
    }

    track_name
        .or(text)
        .or(instrument_name)
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
}
