//! MicStudio Offline Audio Renderer.
//! Provides high-fidelity offline processing of WAV files through the identical
//! DspPipeline used by the live PipeWire engine.

use crate::dsp::pipeline::DspPipeline;
use crate::presets::Preset;
use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub preset: Preset,
    pub cleanup_enabled: bool,
    pub fx_enabled: bool,
    pub denoise_override: Option<bool>,
    pub denoise_blend_override: Option<f32>,
    pub gate_override: Option<bool>,
    pub hpf_override: Option<bool>,
    pub limiter_override: Option<bool>,
    pub raw_passthrough: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            input_path: PathBuf::new(),
            output_path: PathBuf::new(),
            preset: Preset::default(),
            cleanup_enabled: true,
            fx_enabled: true,
            denoise_override: None,
            denoise_blend_override: None,
            gate_override: None,
            hpf_override: None,
            limiter_override: None,
            raw_passthrough: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sample_format: String,
    pub duration_seconds: f64,
    pub num_samples: usize,
    pub peak_amplitude: f32,
    pub rms_amplitude: f32,
    pub rms_dbfs: f32,
    pub clipped_samples: usize,
    pub dc_offset: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderResult {
    pub input_file: String,
    pub output_file: String,
    pub preset_name: String,
    pub input_info: WavInfo,
    pub output_info: WavInfo,
    pub latency_flushed_samples: usize,
    pub cleanup_enabled: bool,
    pub fx_enabled: bool,
    pub denoise_active: bool,
    pub denoise_blend: f32,
    pub gate_active: bool,
}

/// Reads a WAV file of any standard bit depth/format and converts to normalized f32 mono at native sample rate.
pub fn read_wav_mono(path: &Path) -> Result<(Vec<f32>, WavInfo)> {
    let reader = WavReader::open(path)
        .with_context(|| format!("Failed to open WAV file: {}", path.display()))?;
    let spec = reader.spec();

    let channels = spec.channels as usize;
    if channels == 0 {
        anyhow::bail!("WAV file has 0 channels: {}", path.display());
    }

    let mut raw_mono = Vec::new();
    let mut clipped_count = 0usize;
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut max_abs = 0.0f32;

    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => {
            let samples: Vec<i16> = reader
                .into_samples::<i16>()
                .filter_map(Result::ok)
                .collect();
            let num_frames = samples.len() / channels;
            raw_mono.reserve(num_frames);

            for f in 0..num_frames {
                let mut frame_sum = 0.0f32;
                for c in 0..channels {
                    let s = samples[f * channels + c];
                    let norm = s as f32 / 32768.0;
                    frame_sum += norm;
                    if s == i16::MAX || s == i16::MIN {
                        clipped_count += 1;
                    }
                }
                let mono = frame_sum / channels as f32;
                let abs_s = mono.abs();
                if abs_s > max_abs {
                    max_abs = abs_s;
                }
                sum += mono as f64;
                sum_sq += (mono as f64) * (mono as f64);
                raw_mono.push(mono);
            }
        }
        (SampleFormat::Int, 24) => {
            let samples: Vec<i32> = reader
                .into_samples::<i32>()
                .filter_map(Result::ok)
                .collect();
            let num_frames = samples.len() / channels;
            raw_mono.reserve(num_frames);

            for f in 0..num_frames {
                let mut frame_sum = 0.0f32;
                for c in 0..channels {
                    let s = samples[f * channels + c];
                    let norm = s as f32 / 8388608.0;
                    frame_sum += norm;
                    if s >= 8388607 || s <= -8388608 {
                        clipped_count += 1;
                    }
                }
                let mono = frame_sum / channels as f32;
                let abs_s = mono.abs();
                if abs_s > max_abs {
                    max_abs = abs_s;
                }
                sum += mono as f64;
                sum_sq += (mono as f64) * (mono as f64);
                raw_mono.push(mono);
            }
        }
        (SampleFormat::Int, 32) => {
            let samples: Vec<i32> = reader
                .into_samples::<i32>()
                .filter_map(Result::ok)
                .collect();
            let num_frames = samples.len() / channels;
            raw_mono.reserve(num_frames);

            for f in 0..num_frames {
                let mut frame_sum = 0.0f32;
                for c in 0..channels {
                    let s = samples[f * channels + c];
                    let norm = s as f32 / 2147483648.0;
                    frame_sum += norm;
                    if s == i32::MAX || s == i32::MIN {
                        clipped_count += 1;
                    }
                }
                let mono = frame_sum / channels as f32;
                let abs_s = mono.abs();
                if abs_s > max_abs {
                    max_abs = abs_s;
                }
                sum += mono as f64;
                sum_sq += (mono as f64) * (mono as f64);
                raw_mono.push(mono);
            }
        }
        (SampleFormat::Float, 32) => {
            let samples: Vec<f32> = reader
                .into_samples::<f32>()
                .filter_map(Result::ok)
                .collect();
            let num_frames = samples.len() / channels;
            raw_mono.reserve(num_frames);

            for f in 0..num_frames {
                let mut frame_sum = 0.0f32;
                for c in 0..channels {
                    let s = samples[f * channels + c];
                    frame_sum += s;
                    if s.abs() >= 1.0 {
                        clipped_count += 1;
                    }
                }
                let mono = frame_sum / channels as f32;
                let abs_s = mono.abs();
                if abs_s > max_abs {
                    max_abs = abs_s;
                }
                sum += mono as f64;
                sum_sq += (mono as f64) * (mono as f64);
                raw_mono.push(mono);
            }
        }
        (fmt, bits) => {
            anyhow::bail!("Unsupported WAV format: {:?} with {} bits", fmt, bits);
        }
    }

    let n = raw_mono.len();
    let duration = n as f64 / spec.sample_rate as f64;
    let rms = if n > 0 {
        (sum_sq / n as f64).sqrt() as f32
    } else {
        0.0
    };
    let rms_db = if rms > 1e-7 {
        20.0 * rms.log10()
    } else {
        -96.0
    };
    let dc = if n > 0 { (sum / n as f64) as f32 } else { 0.0 };

    let info = WavInfo {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        bits_per_sample: spec.bits_per_sample,
        sample_format: format!("{:?}", spec.sample_format),
        duration_seconds: duration,
        num_samples: n,
        peak_amplitude: max_abs,
        rms_amplitude: rms,
        rms_dbfs: rms_db,
        clipped_samples: clipped_count,
        dc_offset: dc,
    };

    Ok((raw_mono, info))
}

/// 4-point Hermite cubic resampling to 48 kHz.
pub fn resample_to_48k(samples: &[f32], src_rate: u32) -> Vec<f32> {
    if src_rate == 48000 {
        return samples.to_vec();
    }
    let ratio = src_rate as f64 / 48000.0;
    let out_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;

        let s0 = if idx > 0 {
            samples[idx - 1]
        } else {
            samples[0]
        };
        let s1 = samples.get(idx).copied().unwrap_or(0.0);
        let s2 = samples.get(idx + 1).copied().unwrap_or(s1);
        let s3 = samples.get(idx + 2).copied().unwrap_or(s2);

        // 4-point Hermite cubic interpolation polynomial
        let c0 = s1;
        let c1 = 0.5 * (s2 - s0);
        let c2 = s0 - 2.5 * s1 + 2.0 * s2 - 0.5 * s3;
        let c3 = 0.5 * (s3 - s0) + 1.5 * (s1 - s2);
        let val = ((c3 * frac + c2) * frac + c1) * frac + c0;
        out.push(val);
    }
    out
}

/// Writes normalized f32 mono audio to 24-bit PCM WAV at 48000 Hz.
pub fn write_wav_24bit_48k(path: &Path, samples: &[f32]) -> Result<WavInfo> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let spec = WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 24,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::create(path, spec)
        .with_context(|| format!("Failed to create output WAV file: {}", path.display()))?;

    let mut clipped_count = 0usize;
    let mut max_abs = 0.0f32;
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;

    for &s in samples {
        let abs_s = s.abs();
        if abs_s > max_abs {
            max_abs = abs_s;
        }
        if abs_s >= 0.9999 {
            clipped_count += 1;
        }

        sum += s as f64;
        sum_sq += (s as f64) * (s as f64);

        // Scale to 24-bit signed integer (-8388608 to 8388607)
        let scaled = (s * 8388607.0).clamp(-8388608.0, 8388607.0).round() as i32;
        writer.write_sample(scaled)?;
    }

    writer.finalize()?;

    let n = samples.len();
    let duration = n as f64 / 48000.0;
    let rms = if n > 0 {
        (sum_sq / n as f64).sqrt() as f32
    } else {
        0.0
    };
    let rms_db = if rms > 1e-7 {
        20.0 * rms.log10()
    } else {
        -96.0
    };
    let dc = if n > 0 { (sum / n as f64) as f32 } else { 0.0 };

    Ok(WavInfo {
        sample_rate: 48000,
        channels: 1,
        bits_per_sample: 24,
        sample_format: "Int".to_string(),
        duration_seconds: duration,
        num_samples: n,
        peak_amplitude: max_abs,
        rms_amplitude: rms,
        rms_dbfs: rms_db,
        clipped_samples: clipped_count,
        dc_offset: dc,
    })
}

/// Primary offline rendering engine executing through shared DspPipeline.
pub fn render_file(options: &RenderOptions) -> Result<RenderResult> {
    let (raw_samples, in_info) = read_wav_mono(&options.input_path)?;
    let resampled_48k = resample_to_48k(&raw_samples, in_info.sample_rate);

    let output_samples = if options.raw_passthrough {
        resampled_48k
    } else {
        let mut pipeline = DspPipeline::new(48000.0);
        pipeline.reset();
        pipeline.apply_preset(&options.preset);

        // Diagnostic bypass overrides
        pipeline.set_cleanup_enabled(options.cleanup_enabled);
        pipeline.set_fx_enabled(options.fx_enabled);

        if let Some(denoise) = options.denoise_override {
            pipeline.set_denoise_enabled(denoise);
        }
        if let Some(blend) = options.denoise_blend_override {
            pipeline.set_denoise_blend(blend);
        }
        if let Some(gate) = options.gate_override {
            pipeline.set_gate_enabled(gate);
        }
        if let Some(hpf) = options.hpf_override {
            pipeline.set_hpf_enabled(hpf);
        }
        if let Some(limiter) = options.limiter_override {
            pipeline.set_limiter_enabled(limiter);
        }

        let mut processed = Vec::with_capacity(resampled_48k.len() + 2560);
        for &s in &resampled_48k {
            processed.push(pipeline.process_sample(s));
        }

        // Flush pipeline latency and tail (480 samples RNNoise + 2048 samples granular pitch)
        let tail_samples = 2528;
        for _ in 0..tail_samples {
            processed.push(pipeline.process_sample(0.0));
        }

        processed
    };

    let out_info = write_wav_24bit_48k(&options.output_path, &output_samples)?;

    let denoise_active = options.cleanup_enabled
        && options
            .denoise_override
            .unwrap_or(options.preset.noise_suppression);
    let denoise_blend = options.denoise_blend_override.unwrap_or(1.0);
    let gate_active = options.cleanup_enabled
        && options
            .gate_override
            .unwrap_or(options.preset.noise_suppression);

    Ok(RenderResult {
        input_file: options
            .input_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        output_file: options
            .output_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        preset_name: options.preset.name.clone(),
        input_info: in_info,
        output_info: out_info,
        latency_flushed_samples: 2528,
        cleanup_enabled: options.cleanup_enabled,
        fx_enabled: options.fx_enabled,
        denoise_active,
        denoise_blend,
        gate_active,
    })
}
