//! Low-overhead audio monitor for live clean mic level telemetry and sidetone routing.
//! Reads audio directly from ghelper-audio via PipeWire without creating virtual devices.

use crate::dsp::pipeline::AudioTelemetry;
use anyhow::{Context, Result};
use libspa::sys as spa_sys;
use pipewire as pw;
use pw::properties::properties;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

pub struct MicMonitor {
    running: Arc<AtomicBool>,
    telemetry: Arc<RwLock<AudioTelemetry>>,
    worker: Option<JoinHandle<()>>,
}

impl MicMonitor {
    pub fn new(enabled: bool) -> Self {
        let running = Arc::new(AtomicBool::new(enabled));
        let telemetry = Arc::new(RwLock::new(AudioTelemetry::default()));

        let worker = if enabled {
            let run_clone = running.clone();
            let tele_clone = telemetry.clone();
            thread::Builder::new()
                .name("micstudio-telemetry".into())
                .spawn(move || {
                    let _ = Self::run_stream(run_clone, tele_clone);
                })
                .ok()
        } else {
            None
        };

        Self {
            running,
            telemetry,
            worker,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled && self.worker.is_none() {
            self.running.store(true, Ordering::SeqCst);
            let run_clone = self.running.clone();
            let tele_clone = self.telemetry.clone();
            self.worker = thread::Builder::new()
                .name("micstudio-telemetry".into())
                .spawn(move || {
                    let _ = Self::run_stream(run_clone, tele_clone);
                })
                .ok();
        } else if !enabled && self.worker.is_some() {
            self.running.store(false, Ordering::SeqCst);
            if let Some(w) = self.worker.take() {
                let _ = w.join();
            }
        }
    }

    pub fn get_telemetry(&self) -> AudioTelemetry {
        self.telemetry.read().map(|t| *t).unwrap_or_default()
    }

    fn run_stream(running: Arc<AtomicBool>, telemetry: Arc<RwLock<AudioTelemetry>>) -> Result<()> {
        pw::init();

        let mainloop = pw::main_loop::MainLoopRc::new(None)
            .context("Failed to create PipeWire mainloop")?;
        let context = pw::context::ContextRc::new(&mainloop, None)
            .context("Failed to create PipeWire context")?;
        let core = context.connect_rc(None)
            .context("Failed to connect to PipeWire core")?;

        let capture_props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::NODE_NAME => "micstudio-meter-capture",
            *pw::keys::NODE_DESCRIPTION => "MicStudio Audio Meter",
            *pw::keys::AUDIO_CHANNELS => "1",
            "audio.rate" => "48000",
            "target.object" => "ghelper-audio",
            *pw::keys::NODE_PASSIVE => "true",
            *pw::keys::NODE_AUTOCONNECT => "true",
        };

        let stream = pw::stream::StreamBox::new(&core, "micstudio-meter", capture_props)
            .context("Failed to create PipeWire meter stream")?;

        let mut sum_sq = 0.0f32;
        let mut peak_val = 0.0f32;
        let mut sample_count = 0usize;

        let _listener = stream
            .add_local_listener::<()>()
            .process(move |stream, _| {
                if let Some(mut buffer) = stream.dequeue_buffer() {
                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }
                    if let Some(slice) = datas[0].data() {
                        let stride = std::mem::size_of::<f32>();
                        let n_frames = slice.len() / stride;
                        for i in 0..n_frames {
                            let start = i * stride;
                            let end = start + stride;
                            let bytes: [u8; 4] = slice[start..end].try_into().unwrap_or([0; 4]);
                            let s = f32::from_ne_bytes(bytes);
                            let abs = s.abs();
                            sum_sq += s * s;
                            if abs > peak_val {
                                peak_val = abs;
                            }
                            sample_count += 1;
                        }

                        if sample_count >= 1200 {
                            let rms = (sum_sq / sample_count as f32).sqrt();
                            let db = if rms > 1e-5 { 20.0 * rms.log10() } else { -96.0 };
                            if let Ok(mut t) = telemetry.write() {
                                t.input_rms_db = db;
                                t.output_rms_db = db;
                                t.peak_level = peak_val;
                                t.vad_probability = if db > -35.0 { 0.95 } else if db > -50.0 { 0.50 } else { 0.0 };
                            }
                            sum_sq = 0.0;
                            peak_val = 0.0;
                            sample_count = 0;
                        }
                    }
                }
            })
            .register()?;

        let mut audio_info = libspa::param::audio::AudioInfoRaw::new();
        audio_info.set_format(libspa::param::audio::AudioFormat::F32LE);
        audio_info.set_rate(48000);
        audio_info.set_channels(1);
        let mut position = [0; libspa::param::audio::MAX_CHANNELS];
        position[0] = spa_sys::SPA_AUDIO_CHANNEL_MONO;
        audio_info.set_position(position);

        let format_pod: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &pw::spa::pod::Value::Object(pw::spa::pod::Object {
                type_: spa_sys::SPA_TYPE_OBJECT_Format,
                id: spa_sys::SPA_PARAM_EnumFormat,
                properties: audio_info.into(),
            }),
        )
        .context("Failed to serialize format pod")?
        .0
        .into_inner();

        let pod = pw::spa::pod::Pod::from_bytes(&format_pod)
            .context("Failed to parse format pod")?;

        let (pw_sender, pw_receiver) = pw::channel::channel();
        let _pw_listener = pw_receiver.attach(mainloop.loop_(), {
            let mainloop = mainloop.clone();
            move |_| mainloop.quit()
        });

        // Spawn watchdog thread to stop mainloop when running becomes false
        let run_watch = running.clone();
        thread::spawn(move || {
            while run_watch.load(Ordering::Relaxed) {
                thread::sleep(std::time::Duration::from_millis(50));
            }
            let _ = pw_sender.send(());
        });

        let mut params = [pod];
        stream.connect(
            pw::spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )?;

        mainloop.run();
        Ok(())
    }
}

impl Drop for MicMonitor {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
