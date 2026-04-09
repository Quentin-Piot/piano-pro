#[cfg(not(target_arch = "wasm32"))]
mod midi_backend;
#[cfg(not(target_arch = "wasm32"))]
use midi_backend::{MidiBackend, MidiPortInfo};

#[cfg(feature = "synth")]
mod synth_backend;

#[cfg(feature = "synth")]
use synth_backend::SynthBackend;

#[cfg(target_arch = "wasm32")]
pub mod web_backend;
#[cfg(target_arch = "wasm32")]
use web_backend::WebOutputSender;

use std::fmt::{self, Display, Formatter};
#[cfg(feature = "synth")]
use std::path::PathBuf;

use midi_file::midly::{MidiMessage, num::u4};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum OutputDescriptor {
    #[cfg(feature = "synth")]
    Synth(Option<PathBuf>),
    #[cfg(not(target_arch = "wasm32"))]
    MidiOut(MidiPortInfo),
    #[cfg(target_arch = "wasm32")]
    WebOutput,
    DummyOutput,
}

impl OutputDescriptor {
    pub fn is_dummy(&self) -> bool {
        matches!(self, Self::DummyOutput)
    }

    pub fn is_not_dummy(&self) -> bool {
        !self.is_dummy()
    }

    pub fn is_midi(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return matches!(self, OutputDescriptor::MidiOut(_));
        #[cfg(target_arch = "wasm32")]
        return false;
    }

    pub fn is_synth(&self) -> bool {
        #[cfg(feature = "synth")]
        return matches!(self, OutputDescriptor::Synth(_));
        #[cfg(not(feature = "synth"))]
        return false;
    }
}

impl Display for OutputDescriptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "synth")]
            OutputDescriptor::Synth(_) => write!(f, "Buildin Synth"),
            #[cfg(not(target_arch = "wasm32"))]
            OutputDescriptor::MidiOut(info) => write!(f, "{info}"),
            #[cfg(target_arch = "wasm32")]
            OutputDescriptor::WebOutput => write!(f, "Web Audio"),
            OutputDescriptor::DummyOutput => write!(f, "No Output"),
        }
    }
}

#[derive(Clone)]
pub enum OutputConnection {
    #[cfg(not(target_arch = "wasm32"))]
    Midi(midi_backend::MidiOutputConnection),
    #[cfg(feature = "synth")]
    Synth(synth_backend::SynthOutputConnection),
    #[cfg(target_arch = "wasm32")]
    Web(WebOutputSender),
    DummyOutput,
}

impl OutputConnection {
    pub fn midi_event(&self, channel: u4, msg: MidiMessage) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            OutputConnection::Midi(b) => b.midi_event(channel, msg),
            #[cfg(feature = "synth")]
            OutputConnection::Synth(b) => b.midi_event(channel, msg),
            #[cfg(target_arch = "wasm32")]
            OutputConnection::Web(b) => b.midi_event(channel, msg),
            OutputConnection::DummyOutput => {}
        }
    }
    pub fn set_gain(&self, gain: f32) {
        match self {
            #[cfg(feature = "synth")]
            OutputConnection::Synth(b) => b.set_gain(gain),
            #[cfg(target_arch = "wasm32")]
            OutputConnection::Web(b) => b.set_gain(gain),
            _ => {}
        }
    }
    pub fn stop_all(&self) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            OutputConnection::Midi(b) => b.stop_all(),
            #[cfg(feature = "synth")]
            OutputConnection::Synth(b) => b.stop_all(),
            #[cfg(target_arch = "wasm32")]
            OutputConnection::Web(b) => b.stop_all(),
            OutputConnection::DummyOutput => {}
        }
    }
}

pub struct OutputManager {
    #[cfg(feature = "synth")]
    synth_backend: Option<SynthBackend>,
    #[cfg(not(target_arch = "wasm32"))]
    midi_backend: Option<MidiBackend>,
    #[cfg(target_arch = "wasm32")]
    web_sender: Option<WebOutputSender>,

    output_connection: (OutputDescriptor, OutputConnection),
}

impl Default for OutputManager {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputManager {
    pub fn new() -> Self {
        #[cfg(all(feature = "synth", not(target_arch = "wasm32")))]
        let synth_backend = match SynthBackend::new() {
            Ok(synth_backend) => Some(synth_backend),
            Err(err) => {
                log::error!("{err:?}");
                None
            }
        };

        #[cfg(not(target_arch = "wasm32"))]
        let midi_backend = match MidiBackend::new() {
            Ok(midi_device_manager) => Some(midi_device_manager),
            Err(e) => {
                log::error!("{e}");
                None
            }
        };

        Self {
            #[cfg(all(feature = "synth", not(target_arch = "wasm32")))]
            synth_backend,
            #[cfg(not(target_arch = "wasm32"))]
            midi_backend,
            #[cfg(target_arch = "wasm32")]
            web_sender: None,

            output_connection: (OutputDescriptor::DummyOutput, OutputConnection::DummyOutput),
        }
    }

    /// Connect a web audio sender (WASM only).
    #[cfg(target_arch = "wasm32")]
    pub fn connect_web(&mut self, sender: WebOutputSender) {
        self.web_sender = Some(sender.clone());
        self.output_connection = (OutputDescriptor::WebOutput, OutputConnection::Web(sender));
    }

    pub fn outputs(&self) -> Vec<OutputDescriptor> {
        let mut outs = Vec::new();

        #[cfg(all(feature = "synth", not(target_arch = "wasm32")))]
        if let Some(synth) = &self.synth_backend {
            outs.append(&mut synth.get_outputs());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(midi) = &self.midi_backend {
            outs.append(&mut midi.get_outputs());
        }
        #[cfg(target_arch = "wasm32")]
        outs.push(OutputDescriptor::WebOutput);

        outs.push(OutputDescriptor::DummyOutput);

        outs
    }

    pub fn connect(&mut self, desc: OutputDescriptor) {
        if desc != self.output_connection.0 {
            match desc {
                #[cfg(all(feature = "synth", not(target_arch = "wasm32")))]
                OutputDescriptor::Synth(ref font) => {
                    if let Some(ref mut synth) = self.synth_backend {
                        let resolved = if let Some(font) = font.clone() {
                            if font.exists() {
                                Some(font)
                            } else {
                                log::warn!(
                                    "Configured soundfont not found ({}), falling back to default",
                                    font.display()
                                );
                                crate::utils::resources::default_sf2()
                            }
                        } else {
                            crate::utils::resources::default_sf2()
                        };

                        if let Some(path) = resolved {
                            self.output_connection = (
                                desc,
                                OutputConnection::Synth(synth.new_output_connection(&path)),
                            );
                        } else {
                            log::warn!(
                                "No soundfont could be resolved. Expected a bundled resource or a local default.sf2 file."
                            );
                        }
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                OutputDescriptor::MidiOut(ref info) => {
                    if let Some(conn) = MidiBackend::new_output_connection(info) {
                        self.output_connection = (desc, OutputConnection::Midi(conn));
                    }
                }
                #[cfg(target_arch = "wasm32")]
                OutputDescriptor::WebOutput => {
                    if let Some(sender) = self.web_sender.clone() {
                        self.output_connection = (desc, OutputConnection::Web(sender));
                    }
                }
                OutputDescriptor::DummyOutput => {
                    self.output_connection = (desc, OutputConnection::DummyOutput);
                }
            }
        }
    }

    pub fn connection(&self) -> &OutputConnection {
        &self.output_connection.1
    }
}
