use anyhow::Result;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;

const REC_START: &[u8] = include_bytes!("../assets/sounds/rec_start.wav");
const TRANSCRIBE: &[u8] = include_bytes!("../assets/sounds/transcribe.wav");
const CLAUDE: &[u8] = include_bytes!("../assets/sounds/claude.wav");
const FINISH: &[u8] = include_bytes!("../assets/sounds/finish.wav");

enum AudioCmd {
    PlayOnce(&'static [u8]),
    StartLoop { id: u64, buf: &'static [u8] },
    StopLoop { id: u64 },
}

#[derive(Clone)]
pub struct AudioPlayer {
    tx: Sender<AudioCmd>,
    next_id: Arc<AtomicU64>,
}

pub struct LoopHandle {
    tx: Sender<AudioCmd>,
    id: u64,
    stopped: AtomicBool,
}

impl LoopHandle {
    fn stop_inner(&self) {
        if !self.stopped.swap(true, Ordering::Relaxed) {
            let _ = self.tx.send(AudioCmd::StopLoop { id: self.id });
        }
    }
}

impl Drop for LoopHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

impl AudioPlayer {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel::<AudioCmd>();
        thread::Builder::new()
            .name("audio-player".into())
            .spawn(move || run(rx))?;
        Ok(Self {
            tx,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn play_rec_start(&self) {
        let _ = self.tx.send(AudioCmd::PlayOnce(REC_START));
    }

    pub fn play_finish(&self) {
        let _ = self.tx.send(AudioCmd::PlayOnce(FINISH));
    }

    pub fn start_transcribe_loop(&self) -> LoopHandle {
        self.start_loop(TRANSCRIBE)
    }

    pub fn start_claude_loop(&self) -> LoopHandle {
        self.start_loop(CLAUDE)
    }

    fn start_loop(&self, buf: &'static [u8]) -> LoopHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(AudioCmd::StartLoop { id, buf });
        LoopHandle {
            tx: self.tx.clone(),
            id,
            stopped: AtomicBool::new(false),
        }
    }
}

struct LoopState {
    sink: Sink,
    id: u64,
}

fn run(rx: mpsc::Receiver<AudioCmd>) {
    let (stream, handle) = match OutputStream::try_default() {
        Ok(x) => x,
        Err(e) => {
            crate::logln!("[audio] failed to open output stream: {}", e);
            return;
        }
    };
    let _stream = stream;

    let mut current: Option<LoopState> = None;
    let mut oneshots: Vec<Sink> = Vec::new();

    while let Ok(cmd) = rx.recv() {
        oneshots.retain(|s| !s.empty());

        match cmd {
            AudioCmd::PlayOnce(buf) => {
                if let Err(e) = play_once(&handle, buf, &mut oneshots) {
                    crate::logln!("[audio] play_once: {}", e);
                }
            }
            AudioCmd::StartLoop { id, buf } => {
                if let Some(c) = current.take() {
                    c.sink.stop();
                }
                match start_loop(&handle, buf) {
                    Ok(sink) => current = Some(LoopState { sink, id }),
                    Err(e) => crate::logln!("[audio] start_loop: {}", e),
                }
            }
            AudioCmd::StopLoop { id } => {
                if let Some(c) = &current {
                    if c.id == id {
                        let taken = current.take().unwrap();
                        taken.sink.stop();
                    }
                }
            }
        }
    }
}

fn play_once(
    handle: &OutputStreamHandle,
    buf: &'static [u8],
    oneshots: &mut Vec<Sink>,
) -> Result<()> {
    let src = Decoder::new(Cursor::new(buf))?;
    let sink = Sink::try_new(handle)?;
    sink.append(src);
    oneshots.push(sink);
    Ok(())
}

fn start_loop(handle: &OutputStreamHandle, buf: &'static [u8]) -> Result<Sink> {
    let src = Decoder::new(Cursor::new(buf))?.repeat_infinite();
    let sink = Sink::try_new(handle)?;
    sink.append(src);
    Ok(sink)
}
