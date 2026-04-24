use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host};
use hound::{SampleFormat as HSampleFormat, WavSpec, WavWriter};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;

pub struct Recorder {
    stop_flag: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<Result<PathBuf>>>,
}

pub fn start(out_path: PathBuf) -> Result<Recorder> {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_for_thread = stop_flag.clone();

    let handle = thread::spawn(move || -> Result<PathBuf> {
        let host = cpal::default_host();
        let device = pick_builtin_mic(&host)?;
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();
        crate::logln!(
            "[cpal] device={:?} rate={} ch={} fmt={:?}",
            device.name().unwrap_or_default(),
            sample_rate,
            channels,
            sample_format
        );

        let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::with_capacity(
            (sample_rate as usize) * 60,
        )));
        let samples_cb = samples.clone();

        let err_fn = |err| crate::logln!("[cpal] stream error: {}", err);

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    let mut buf = samples_cb.lock().unwrap();
                    for frame in data.chunks(channels as usize) {
                        let mono: f32 = frame.iter().sum::<f32>() / channels as f32;
                        let s = (mono * i16::MAX as f32)
                            .clamp(i16::MIN as f32, i16::MAX as f32)
                            as i16;
                        buf.push(s);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    let mut buf = samples_cb.lock().unwrap();
                    for frame in data.chunks(channels as usize) {
                        let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                        let mono = (sum / channels as i32) as i16;
                        buf.push(mono);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    let mut buf = samples_cb.lock().unwrap();
                    for frame in data.chunks(channels as usize) {
                        let sum: i32 = frame
                            .iter()
                            .map(|&s| s as i32 - i16::MAX as i32)
                            .sum();
                        let mono = (sum / channels as i32) as i16;
                        buf.push(mono);
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err(anyhow!("unsupported sample format")),
        };

        stream.play()?;

        while !stop_for_thread.load(Ordering::Relaxed) {
            thread::sleep(std::time::Duration::from_millis(40));
        }

        drop(stream);

        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: HSampleFormat::Int,
        };
        let mut writer = WavWriter::create(&out_path, spec)?;
        let buf = samples.lock().unwrap();
        for &s in buf.iter() {
            writer.write_sample(s)?;
        }
        writer.finalize()?;

        Ok(out_path)
    });

    Ok(Recorder {
        stop_flag,
        handle: Some(handle),
    })
}

fn pick_builtin_mic(host: &Host) -> Result<Device> {
    let preferred = [
        "macbook pro microphone",
        "macbook air microphone",
        "built-in microphone",
        "macbook",
        "built-in",
    ];
    let devices: Vec<Device> = host
        .input_devices()
        .map_err(|e| anyhow!("list input devices: {}", e))?
        .collect();

    for p in preferred {
        for d in &devices {
            if let Ok(name) = d.name() {
                if name.to_lowercase().contains(p) {
                    crate::logln!("[cpal] picked built-in: {}", name);
                    return Ok(d.clone());
                }
            }
        }
    }

    crate::logln!("[cpal] no built-in mic found, falling back to default");
    host.default_input_device()
        .ok_or_else(|| anyhow!("no input device"))
}

impl Recorder {
    pub fn stop(mut self) -> Result<PathBuf> {
        self.stop_flag.store(true, Ordering::Relaxed);
        let handle = self
            .handle
            .take()
            .ok_or_else(|| anyhow!("recorder already consumed"))?;
        handle
            .join()
            .map_err(|_| anyhow!("recorder thread panicked"))?
    }
}
