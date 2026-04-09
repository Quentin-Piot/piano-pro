use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use midi_file::midly::{MidiMessage, num::u4};

pub enum WebAudioCmd {
    Midi { channel: u8, message: MidiMessage },
    StopAll,
}

pub type WebAudioQueue = Rc<RefCell<VecDeque<WebAudioCmd>>>;

#[derive(Clone)]
pub struct WebOutputSender(WebAudioQueue);

impl WebOutputSender {
    pub fn new() -> (Self, WebAudioQueue) {
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        (Self(queue.clone()), queue)
    }

    pub fn midi_event(&self, channel: u4, msg: MidiMessage) {
        self.0.borrow_mut().push_back(WebAudioCmd::Midi {
            channel: channel.as_int(),
            message: msg,
        });
    }

    pub fn stop_all(&self) {
        self.0.borrow_mut().push_back(WebAudioCmd::StopAll);
    }

    pub fn set_gain(&self, _gain: f32) {}
}
