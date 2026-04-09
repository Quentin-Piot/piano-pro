use midi_file::midly::{self, MidiMessage, live::LiveEvent};
use winit::event_loop::EventLoopProxy;

use crate::NeothesiaEvent;

pub struct InputManager {
    input: Option<midi_io::MidiInputManager>,
    tx: EventLoopProxy<NeothesiaEvent>,
    current_connection: Option<midi_io::MidiInputConnection>,
}

impl InputManager {
    pub fn new(tx: EventLoopProxy<NeothesiaEvent>) -> Self {
        let input = match midi_io::MidiInputManager::new() {
            Ok(m) => Some(m),
            Err(err) => {
                log::warn!("MIDI input unavailable: {err}");
                None
            }
        };
        Self {
            input,
            tx,
            current_connection: None,
        }
    }

    pub fn inputs(&self) -> Vec<midi_io::MidiInputPort> {
        self.input.as_ref().map(|i| i.inputs()).unwrap_or_default()
    }

    pub fn connect_input(&mut self, port: midi_io::MidiInputPort) {
        let tx = self.tx.clone();

        // Close the connection first, as Windows does not like it when we hold 2 connections
        self.current_connection = None;

        self.current_connection = midi_io::MidiInputManager::connect_input(port, move |message| {
            let event = LiveEvent::parse(message).unwrap();

            if let LiveEvent::Midi { channel, message } = event {
                match message {
                    // Some keyboards send NoteOn event with vel 0 instead of NoteOff
                    midly::MidiMessage::NoteOn { key, vel } if vel == 0 => {
                        tx.send_event(NeothesiaEvent::MidiInput {
                            channel: channel.as_int(),
                            message: MidiMessage::NoteOff { key, vel },
                        })
                        .ok();
                    }
                    message => {
                        tx.send_event(NeothesiaEvent::MidiInput {
                            channel: channel.as_int(),
                            message,
                        })
                        .ok();
                    }
                }
            }
        });
    }
}
