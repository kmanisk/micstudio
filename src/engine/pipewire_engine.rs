//! Native PipeWire Real-Time Audio Engine.
//! Captures from the hardware microphone, processes voice effects through DspPipeline,
//! and exposes virtual source `ghelper-audio-fx` to applications.

use crate::dsp::pipeline::{AudioTelemetry, DspPipeline};
use crate::engine::ring_buffer::SpscRingBuffer;
use crate::ipc::{socket_path, IpcRequest, IpcResponse};
use crate::presets::Preset;
use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use libspa::pod::Pod;
use libspa::sys as spa_sys;
use pipewire as pw;
use pw::properties::properties;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u32 = 1;

pub struct EngineHandles {
    pub stop_signal: Arc<AtomicBool>,
}

pub fn run_engine(initial_preset: Preset, stop_signal: Arc<AtomicBool>) -> Result<()> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .context("Failed to create PipeWire mainloop")?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .context("Failed to create PipeWire context")?;
    let core = context.connect_rc(None)
        .context("Failed to connect to PipeWire core")?;

    // Ring buffer capacity: 8192 samples (~170 ms safety headroom)
    let ring_buffer = Arc::new(SpscRingBuffer::new(8192));
    for _ in 0..480 {
        ring_buffer.push(0.0);
    }
    let ring_capture = ring_buffer.clone();
    let ring_playback = ring_buffer.clone();

    // IPC channels
    let (preset_tx, preset_rx): (Sender<Preset>, Receiver<Preset>) = bounded(16);
    let (telemetry_tx, telemetry_rx): (Sender<AudioTelemetry>, Receiver<AudioTelemetry>) = bounded(16);

    let active_preset_name = Arc::new(std::sync::RwLock::new(initial_preset.name.clone()));
    let active_name_ipc = active_preset_name.clone();

    // Start IPC Socket Thread
    let sock_path = socket_path();
    let _ = fs::remove_file(&sock_path);
    if let Some(parent) = sock_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let listener = UnixListener::bind(&sock_path)
        .with_context(|| format!("Failed to bind UNIX domain socket at {}", sock_path.display()))?;

    let stop_signal_ipc = stop_signal.clone();
    let (pw_sender, pw_receiver) = pipewire::channel::channel();
    let _pw_receiver_listener = pw_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });

    thread::spawn(move || {
        let mut last_telemetry = AudioTelemetry::default();
        listener.set_nonblocking(false).ok();

        for stream in listener.incoming() {
            if stop_signal_ipc.load(Ordering::Relaxed) {
                break;
            }
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_ok() {
                if let Ok(req) = serde_json::from_str::<IpcRequest>(&line) {
                    let resp = match req {
                        IpcRequest::ApplyPreset(p) => {
                            if let Ok(mut lock) = active_name_ipc.write() {
                                *lock = p.name.clone();
                            }
                            let _ = preset_tx.try_send(p);
                            IpcResponse::Success
                        }
                        IpcRequest::GetStatus => {
                            while let Ok(t) = telemetry_rx.try_recv() {
                                last_telemetry = t;
                            }
                            let active_str = active_name_ipc.read().map(|l| l.clone()).unwrap_or_else(|_| "Unknown".to_string());
                            IpcResponse::Status {
                                active_preset: active_str,
                                telemetry: last_telemetry,
                            }
                        }
                        IpcRequest::StopEngine => {
                            stop_signal_ipc.store(true, Ordering::SeqCst);
                            let _ = pw_sender.send(());
                            IpcResponse::Success
                        }
                    };
                    if let Ok(data) = serde_json::to_string(&resp) {
                        let _ = stream.write_all(data.as_bytes());
                        let _ = stream.write_all(b"\n");
                        let _ = stream.flush();
                    }
                }
            }
        }
    });

    // 1. Setup Virtual Source Stream (`ghelper-audio`)
    let source_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::MEDIA_CLASS => "Audio/Source",
        *pw::keys::NODE_NAME => "ghelper-audio",
        *pw::keys::NODE_DESCRIPTION => "MicStudio Clean",
        *pw::keys::NODE_NICK => "MicStudio Clean",
        *pw::keys::NODE_VIRTUAL => "true",
        *pw::keys::AUDIO_CHANNELS => "1",
        "audio.position" => "MONO",
        "audio.rate" => "48000",
        *pw::keys::PRIORITY_SESSION => "1000",
        "node.latency" => "480/48000",
    };

    let source_stream = pw::stream::StreamBox::new(&core, "ghelper-audio", source_props)
        .context("Failed to create PipeWire virtual source stream")?;

    let _source_listener = source_stream
        .add_local_listener::<()>()
        .process(move |stream, _| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let req = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                let stride = std::mem::size_of::<f32>();
                let mut written_bytes = 0;
                if let Some(slice) = datas[0].data() {
                    let n_avail = slice.len() / stride;
                    let n_frames = if req > 0 && req <= n_avail {
                        req
                    } else if n_avail > 0 {
                        480.min(n_avail)
                    } else {
                        0
                    };
                    for i in 0..n_frames {
                        let val = ring_playback.pop().unwrap_or(0.0);
                        let start = i * stride;
                        let end = start + stride;
                        slice[start..end].copy_from_slice(&val.to_ne_bytes());
                    }
                    written_bytes = n_frames * stride;
                }
                let chunk = datas[0].chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = stride as _;
                *chunk.size_mut() = written_bytes as _;
            }
        })
        .register()?;

    // 2. Setup Microphone Capture Stream (`capture.ghelper-audio`)
    let capture_props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => "capture.ghelper-audio",
        *pw::keys::NODE_DESCRIPTION => "MicStudio Microphone Capture",
        *pw::keys::AUDIO_CHANNELS => "1",
        "audio.position" => "MONO",
        "audio.rate" => "48000",
        "target.object" => "alsa_input.pci-0000_00_1f.3.analog-stereo",
        "node.dont-reconnect" => "true",
        "node.latency" => "480/48000",
        "node.autoconnect" => "true",
    };

    let capture_stream = pw::stream::StreamBox::new(&core, "capture.ghelper-audio", capture_props)
        .context("Failed to create PipeWire mic capture stream")?;

    // Create DSP Pipeline
    let mut pipeline = DspPipeline::new(SAMPLE_RATE as f32);
    pipeline.apply_preset(&initial_preset);

    let mut telemetry_counter = 0usize;

    let _capture_listener = capture_stream
        .add_local_listener::<()>()
        .process(move |stream, _| {
            // Check for preset updates from IPC
            while let Ok(new_preset) = preset_rx.try_recv() {
                pipeline.apply_preset(&new_preset);
            }

            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                let stride = std::mem::size_of::<f32>();
                let offset = datas[0].chunk().offset() as usize;
                let size = datas[0].chunk().size() as usize;
                if let Some(slice) = datas[0].data() {
                    if size > 0 {
                        let valid_end = (offset + size).min(slice.len());
                        if valid_end > offset {
                            let valid_slice = &slice[offset..valid_end];
                            let n_frames = valid_slice.len() / stride;
                            for i in 0..n_frames {
                                let start = i * stride;
                                let end = start + stride;
                                let bytes: [u8; 4] = valid_slice[start..end].try_into().unwrap_or([0; 4]);
                                let in_sample = f32::from_ne_bytes(bytes);

                                let out_sample = pipeline.process_sample(in_sample);
                                ring_capture.push(out_sample);
                            }

                            telemetry_counter += n_frames;
                            if telemetry_counter >= 2400 { // ~50ms
                                let _ = telemetry_tx.try_send(pipeline.get_telemetry());
                                telemetry_counter = 0;
                            }
                        }
                    }
                }
            }
        })
        .register()?;

    // Build format parameter pod for Mono 48kHz Float32
    let mut audio_info = libspa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(libspa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(SAMPLE_RATE);
    audio_info.set_channels(CHANNELS);
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
    .unwrap()
    .0
    .into_inner();

    let mut params_source = [Pod::from_bytes(&format_pod).unwrap()];
    let mut params_capture = [Pod::from_bytes(&format_pod).unwrap()];

    // Connect streams:
    // source_stream must NOT autoconnect to audio sinks (speakers)
    source_stream.connect(
        libspa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params_source,
    )?;

    // capture_stream autoconnects to target.object (hardware mic)
    capture_stream.connect(
        libspa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params_capture,
    )?;

    // Run PipeWire loop until stop signal
    mainloop.run();

    // Cleanup socket on exit
    let _ = fs::remove_file(&sock_path);
    Ok(())
}
