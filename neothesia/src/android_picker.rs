use std::sync::Mutex;

pub enum PickResult {
    File { name: String, bytes: Vec<u8> },
    Cancelled,
}

static RESULT: Mutex<Option<PickResult>> = Mutex::new(None);
static LAUNCH_FN: Mutex<Option<fn()>> = Mutex::new(None);

pub fn register_launcher(f: fn()) {
    *LAUNCH_FN.lock().unwrap() = Some(f);
}

pub fn launch() {
    if let Some(f) = *LAUNCH_FN.lock().unwrap() {
        f();
    }
}

pub fn deliver(name: String, bytes: Vec<u8>) {
    *RESULT.lock().unwrap() = Some(PickResult::File { name, bytes });
}

pub fn cancel() {
    *RESULT.lock().unwrap() = Some(PickResult::Cancelled);
}

pub fn poll() -> Option<PickResult> {
    RESULT.lock().unwrap().take()
}
