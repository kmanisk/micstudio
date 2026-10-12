use hound::{SampleFormat, WavSpec, WavWriter};
use micstudio::dsp::effects::{BandpassEffect, Limiter, StutterGate};
use micstudio::dsp::pipeline::DspPipeline;
use micstudio::dsp::pitch::PitchShifter;
use micstudio::dsp::vocoder::Vocoder;
use micstudio::presets::PresetManager;
use micstudio::renderer::{render_file, resample_to_48k, RenderOptions};
use std::fs;

fn generate_sine_wave(freq_hz: f32, duration_sec: f32, sample_rate: f32) -> Vec<f32> {
    let num_samples = (duration_sec * sample_rate) as usize;
    let mut out = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f32 / sample_rate;
        let s = (2.0 * std::f32::consts::PI * freq_hz * t).sin();
        out.push(s);
    }
    out
}

fn count_zero_crossings(samples: &[f32]) -> usize {
    let mut count = 0;
    for i in 1..samples.len() {
        if (samples[i - 1] < 0.0 && samples[i] >= 0.0) || (samples[i - 1] > 0.0 && samples[i] <= 0.0) {
            count += 1;
        }
    }
    count
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() { return 0.0; }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[test]
fn test_01_all_presets_exist_and_validate() {
    let mgr = PresetManager::new().expect("Failed to initialize PresetManager");
    let all = mgr.all_presets();
    assert!(all.len() >= 20, "Expected at least 20 voice presets, found {}", all.len());

    let expected_names = [
        "Natural Clean", "Deep Voice", "Chipmunk", "Daft Punk",
        "Walkie Talkie", "Robot"
    ];

    for name in expected_names {
        let preset = mgr.find_preset(name).unwrap_or_else(|| panic!("Preset '{}' missing", name));
        assert!(!preset.name.is_empty());
        assert!(!preset.description.is_empty());
        assert!(!preset.category.is_empty());
        assert!(preset.pitch_semitones >= -24.0 && preset.pitch_semitones <= 24.0);
        assert!(preset.vocoder_mix >= 0.0 && preset.vocoder_mix <= 1.0);
        assert!(preset.matrix_ring_mod >= 0.0 && preset.matrix_ring_mod <= 1.0);
    }
}

#[test]
fn test_02_pitch_shifter_direction_and_zero_crossing() {
    let sample_rate = 48000.0;
    let input = generate_sine_wave(440.0, 0.5, sample_rate); // 440 Hz
    let _in_crossings = count_zero_crossings(&input);

    // Test Octave Down (-12 semitones)
    let mut pitch_down = PitchShifter::new();
    pitch_down.set_semitones(-12.0);
    let mut out_down = Vec::new();
    for &s in &input {
        out_down.push(pitch_down.process(s));
    }
    // Skip warm-up grain period
    let skip = 2048;
    let down_crossings = count_zero_crossings(&out_down[skip..]);
    let in_sub_crossings = count_zero_crossings(&input[skip..]);
    assert!(
        down_crossings < in_sub_crossings,
        "Octave down must reduce zero-crossings (got {}, expected < {})",
        down_crossings, in_sub_crossings
    );

    // Test Octave Up (+12 semitones)
    let mut pitch_up = PitchShifter::new();
    pitch_up.set_semitones(12.0);
    let mut out_up = Vec::new();
    for &s in &input {
        out_up.push(pitch_up.process(s));
    }
    let up_crossings = count_zero_crossings(&out_up[skip..]);
    assert!(
        up_crossings > in_sub_crossings,
        "Octave up must increase zero-crossings (got {}, expected > {})",
        up_crossings, in_sub_crossings
    );
}

#[test]
fn test_03_bandpass_filter_attenuates_out_of_band() {
    let sample_rate = 48000.0;
    let mut bp = BandpassEffect::new(sample_rate);
    bp.set_params(450.0, 3200.0, sample_rate);

    // 100 Hz sub-bass (out of band, below 450 Hz)
    let sub = generate_sine_wave(100.0, 0.2, sample_rate);
    let sub_out: Vec<f32> = sub.iter().map(|&s| bp.process(s)).collect();
    let sub_rms = compute_rms(&sub_out[1000..]);

    // 1500 Hz mid-speech (in band, inside 450-3200 Hz)
    bp.reset();
    let mid = generate_sine_wave(1500.0, 0.2, sample_rate);
    let mid_out: Vec<f32> = mid.iter().map(|&s| bp.process(s)).collect();
    let mid_rms = compute_rms(&mid_out[1000..]);

    // 10 kHz treble (out of band, above 3200 Hz)
    bp.reset();
    let high = generate_sine_wave(10000.0, 0.2, sample_rate);
    let high_out: Vec<f32> = high.iter().map(|&s| bp.process(s)).collect();
    let high_rms = compute_rms(&high_out[1000..]);

    assert!(
        sub_rms < mid_rms * 0.25,
        "Bandpass failed to attenuate 100 Hz sub-bass (sub: {}, mid: {})",
        sub_rms, mid_rms
    );
    assert!(
        high_rms < mid_rms * 0.35,
        "Bandpass failed to attenuate 10 kHz highs (high: {}, mid: {})",
        high_rms, mid_rms
    );
}

#[test]
fn test_04_vocoder_and_stutter_transformations() {
    let sample_rate = 48000.0;
    let input = generate_sine_wave(300.0, 0.5, sample_rate);

    // Test Stutter Gate (8 Hz)
    let mut stutter = StutterGate::new(sample_rate);
    stutter.set_params(8.0, 0.5);
    let st_out: Vec<f32> = input.iter().map(|&s| stutter.process(s)).collect();
    let min_val = st_out.iter().map(|s| s.abs()).fold(1.0f32, f32::min);
    assert!(min_val < 0.05, "Stutter gate must produce near-zero troughs");

    // Test Vocoder
    let mut vocoder = Vocoder::new(sample_rate);
    vocoder.set_params(0.8, 110.0, 3.5, 0.0);
    let voc_out: Vec<f32> = input.iter().map(|&s| vocoder.process(s)).collect();
    let voc_rms = compute_rms(&voc_out[2048..]);
    assert!(voc_rms > 0.01, "Vocoder output must be audible and non-silent");
    assert!(!voc_rms.is_nan() && !voc_rms.is_infinite());
}

#[test]
fn test_05_limiter_brickwall_ceiling() {
    let mut limiter = Limiter::new();
    let sample_rate = 48000.0;
    // Overdriven signal (+12 dBFS)
    let hot_input: Vec<f32> = generate_sine_wave(1000.0, 0.1, sample_rate)
        .into_iter()
        .map(|s| s * 4.0)
        .collect();

    let mut max_peak = 0.0f32;
    for s in hot_input {
        let out = limiter.process(s);
        assert!(!out.is_nan() && !out.is_infinite());
        if out.abs() > max_peak {
            max_peak = out.abs();
        }
    }

    assert!(
        max_peak <= 1.0001,
        "Limiter ceiling violated! Peak was {}",
        max_peak
    );
}

#[test]
fn test_06_dsp_pipeline_state_isolation() {
    let sample_rate = 48000.0;
    let input = generate_sine_wave(440.0, 0.2, sample_rate);

    let mut pipe_clean = DspPipeline::new(sample_rate);
    let clean_preset = PresetManager::new().unwrap().find_preset("Natural Clean").unwrap();
    pipe_clean.apply_preset(&clean_preset);
    let out_clean1: Vec<f32> = input.iter().map(|&s| pipe_clean.process_sample(s)).collect();

    // Now corrupt state with heavy Robot preset
    let robot = PresetManager::new().unwrap().find_preset("Robot").unwrap();
    pipe_clean.apply_preset(&robot);
    for &s in &input {
        pipe_clean.process_sample(s);
    }

    // Reset and reapply clean
    pipe_clean.reset();
    pipe_clean.apply_preset(&clean_preset);
    let out_clean2: Vec<f32> = input.iter().map(|&s| pipe_clean.process_sample(s)).collect();

    // Verify out_clean1 matches out_clean2 sample-for-sample
    assert_eq!(out_clean1.len(), out_clean2.len());
    for i in 0..out_clean1.len() {
        let diff = (out_clean1[i] - out_clean2[i]).abs();
        assert!(diff < 1e-4, "State isolation failed at sample {}: diff = {}", i, diff);
    }
}

#[test]
fn test_07_all_13_presets_produce_finite_audible_output() {
    let mgr = PresetManager::new().unwrap();
    let sample_rate = 48000.0;
    let input = generate_sine_wave(350.0, 0.3, sample_rate);

    for preset in mgr.all_presets() {
        let mut pipeline = DspPipeline::new(sample_rate);
        pipeline.apply_preset(&preset);
        let mut max_abs = 0.0f32;
        let mut sum_sq = 0.0f32;

        for &s in &input {
            let out = pipeline.process_sample(s);
            assert!(!out.is_nan(), "Preset '{}' generated NaN", preset.name);
            assert!(!out.is_infinite(), "Preset '{}' generated Inf", preset.name);
            let abs_o = out.abs();
            if abs_o > max_abs { max_abs = abs_o; }
            sum_sq += out * out;
        }

        let rms = (sum_sq / input.len() as f32).sqrt();
        assert!(
            rms > 0.001,
            "Preset '{}' produced silence (RMS = {})",
            preset.name, rms
        );
        assert!(
            max_abs <= 1.01,
            "Preset '{}' exceeded limiter safety ceiling (Peak = {})",
            preset.name, max_abs
        );
    }
}

#[test]
fn test_08_hermite_resampler_accuracy() {
    // Resample 44100 Hz -> 48000 Hz
    let in_samples = generate_sine_wave(1000.0, 0.1, 44100.0);
    let out_samples = resample_to_48k(&in_samples, 44100);

    let expected_len = (in_samples.len() as f64 * 48000.0 / 44100.0).ceil() as usize;
    assert!((out_samples.len() as isize - expected_len as isize).abs() <= 2);

    for &s in &out_samples {
        assert!(!s.is_nan() && !s.is_infinite());
    }
}

#[test]
fn test_09_offline_render_does_not_modify_source() {
    let temp_dir = std::env::temp_dir().join("micstudio_test_safety");
    let _ = fs::create_dir_all(&temp_dir);
    let input_wav = temp_dir.join("test_input.wav");
    let output_wav = temp_dir.join("test_output.wav");

    // Write a test input WAV
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    {
        let mut writer = WavWriter::create(&input_wav, spec).unwrap();
        for &s in &generate_sine_wave(440.0, 0.5, 48000.0) {
            writer.write_sample((s * 32767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    let initial_bytes = fs::read(&input_wav).unwrap();

    let preset = PresetManager::new().unwrap().find_preset("Deep Voice").unwrap();
    let options = RenderOptions {
        input_path: input_wav.clone(),
        output_path: output_wav.clone(),
        preset,
        ..Default::default()
    };

    let report = render_file(&options).expect("Render failed");
    assert_eq!(report.preset_name, "Deep Voice");
    assert!(output_wav.exists());

    let final_bytes = fs::read(&input_wav).unwrap();
    assert_eq!(
        initial_bytes, final_bytes,
        "CRITICAL ERROR: Input file was modified by rendering process!"
    );

    // Clean up
    let _ = fs::remove_file(input_wav);
    let _ = fs::remove_file(output_wav);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn test_10_clean_reference_has_zero_creative_fx() {
    let mgr = PresetManager::new().unwrap();
    let clean = mgr.find_preset("Natural Clean").unwrap();
    assert_eq!(clean.category, "Natural");
    assert_eq!(clean.pitch_semitones, 0.0);
    assert_eq!(clean.vocoder_mix, 0.0);
    assert_eq!(clean.matrix_ring_mod, 0.0);
    assert_eq!(clean.stutter_rate, 0.0);
    assert_eq!(clean.bandpass_hpf, 0.0);
    assert_eq!(clean.bandpass_lpf, 0.0);
    assert_eq!(clean.bitcrush_bits, 24);
    assert_eq!(clean.bitcrush_downsample, 1);
    assert!(clean.noise_suppression, "Natural Clean must retain noise suppression");
}

#[test]
fn test_11_wav_stereo_to_mono_and_float_conversions() {
    let temp_dir = std::env::temp_dir().join("micstudio_test_stereo");
    let _ = fs::create_dir_all(&temp_dir);
    let stereo_wav = temp_dir.join("test_stereo.wav");

    // Write a stereo 16-bit WAV with L = +0.5, R = -0.5 (mono downmix must cancel to 0.0)
    let spec = WavSpec {
        channels: 2,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    {
        let mut writer = WavWriter::create(&stereo_wav, spec).unwrap();
        for _ in 0..1000 {
            writer.write_sample(16384i16).unwrap();  // +0.5
            writer.write_sample(-16384i16).unwrap(); // -0.5
        }
        writer.finalize().unwrap();
    }

    let (mono_samples, info) = micstudio::renderer::read_wav_mono(&stereo_wav).unwrap();
    assert_eq!(info.channels, 2);
    assert_eq!(info.sample_rate, 44100);
    assert_eq!(mono_samples.len(), 1000);
    for &s in &mono_samples {
        assert!(s.abs() < 1e-4, "Stereo cancellation downmix failed: got {}", s);
    }

    let _ = fs::remove_file(stereo_wav);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn test_12_dc_offset_and_buffer_boundary_handling() {
    let sample_rate = 48000.0;
    // Input with heavy DC offset (+0.5) and low rumble
    let mut dc_rumble_signal = Vec::new();
    for i in 0..4800 {
        let t = i as f32 / sample_rate;
        let s = 0.5 + 0.3 * (2.0 * std::f32::consts::PI * 20.0 * t).sin(); // 20 Hz rumble + 0.5 DC
        dc_rumble_signal.push(s);
    }

    let mut pipeline = DspPipeline::new(sample_rate);
    let clean = PresetManager::new().unwrap().find_preset("Natural Clean").unwrap();
    pipeline.apply_preset(&clean);

    let mut out_samples = Vec::new();
    for &s in &dc_rumble_signal {
        out_samples.push(pipeline.process_sample(s));
    }

    // After 70 Hz HPF settles (> 1000 samples), DC offset should be stripped to near zero
    let settled_dc: f32 = out_samples[1500..].iter().sum::<f32>() / out_samples[1500..].len() as f32;
    assert!(
        settled_dc.abs() < 0.05,
        "70 Hz HPF failed to remove DC offset! Settled DC was {}",
        settled_dc
    );
}

