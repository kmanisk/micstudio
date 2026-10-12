use anyhow::{Context, Result};
use libspa::sys as spa_sys;
use pipewire as pw;
use pw::properties::properties;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_pipewire_capture_stream() -> Result<()> {
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
        *pw::keys::NODE_NAME => "test-stream-capture",
        *pw::keys::NODE_DESCRIPTION => "Test Stream",
        *pw::keys::AUDIO_CHANNELS => "1",
        "audio.rate" => "48000",
        *pw::keys::NODE_AUTOCONNECT => "true",
    };

    let stream = pw::stream::StreamBox::new(&core, "test-stream", capture_props)
        .context("Failed to create PipeWire stream")?;

    let received_samples = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rec_clone = received_samples.clone();

    let _listener = stream
        .add_local_listener::<()>()
        .process(move |stream, _| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if !datas.is_empty() {
                    if let Some(slice) = datas[0].data() {
                        let n_frames = slice.len() / 4;
                        rec_clone.fetch_add(n_frames, Ordering::SeqCst);
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
    )?
    .0
    .into_inner();

    let pod = pw::spa::pod::Pod::from_bytes(&format_pod).context("parse pod")?;

    let (pw_sender, pw_receiver) = pw::channel::channel();
    let _pw_listener = pw_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1000));
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

    let total = received_samples.load(Ordering::SeqCst);
    println!("Total frames captured in 1 second: {}", total);
    assert!(total > 0, "Must capture audio frames!");
    Ok(())
}

#[test]
fn test_mic_monitor_spectrum() -> Result<()> {
    use micstudio::monitor::MicMonitor;

    let mut monitor = MicMonitor::new(true);
    thread::sleep(Duration::from_millis(600));

    let spec = monitor.get_spectrum();
    println!("MicMonitor captured RMS: {:.1} dBFS, Peak: {:.4}", spec.rms_db, spec.peak_level);
    assert_eq!(spec.bands.len(), 32);

    monitor.set_enabled(false);
    Ok(())
}
