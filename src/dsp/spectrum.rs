//! High-performance real-time audio spectrum analyzer and spectrograph engine.
//! Computes 1024-point Radix-2 FFT with Hann windowing and logarithmic psychoacoustic
//! frequency bands (50 Hz to 16 kHz) with peak-hold ballistics and 2D waterfall history.

use std::collections::VecDeque;

pub const FFT_SIZE: usize = 1024;
pub const NUM_BANDS: usize = 32;
pub const WATERFALL_HISTORY: usize = 48;

#[derive(Debug, Clone)]
pub struct LiveSpectrumData {
    /// Normalized levels [0.0, 1.0] for each frequency band (-80 dBFS to 0 dBFS)
    pub bands: Vec<f32>,
    /// Raw dBFS levels (-80.0 to 0.0)
    pub band_db: Vec<f32>,
    /// Decaying peak hold indicators [0.0, 1.0]
    pub peaks: Vec<f32>,
    /// Center frequencies (Hz) for each band
    pub freqs: Vec<f32>,
    /// Rolling waterfall history (oldest to newest)
    pub waterfall: VecDeque<Vec<f32>>,
    /// Overall peak amplitude (0.0 to 1.0)
    pub peak_level: f32,
    /// Overall RMS in dBFS
    pub rms_db: f32,
}

impl Default for LiveSpectrumData {
    fn default() -> Self {
        let freqs = compute_band_centers(NUM_BANDS);
        Self {
            bands: vec![0.0; NUM_BANDS],
            band_db: vec![-80.0; NUM_BANDS],
            peaks: vec![0.0; NUM_BANDS],
            freqs,
            waterfall: VecDeque::with_capacity(WATERFALL_HISTORY),
            peak_level: 0.0,
            rms_db: -80.0,
        }
    }
}

pub fn compute_band_centers(num_bands: usize) -> Vec<f32> {
    let min_f = 50.0f32;
    let max_f = 16000.0f32;
    let ratio = (max_f / min_f).powf(1.0 / num_bands as f32);
    let mut freqs = Vec::with_capacity(num_bands);
    let mut cur_low = min_f;
    for _ in 0..num_bands {
        let cur_high = cur_low * ratio;
        freqs.push((cur_low * cur_high).sqrt());
        cur_low = cur_high;
    }
    freqs
}

pub struct SpectrumAnalyzer {
    sample_rate: f32,
    num_bands: usize,
    window: [f32; FFT_SIZE],
    peak_holds: Vec<f32>,
    peak_decay_timers: Vec<u8>,
    waterfall: VecDeque<Vec<f32>>,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: f32, num_bands: usize) -> Self {
        let mut window = [0.0f32; FFT_SIZE];
        for i in 0..FFT_SIZE {
            // Hann window: 0.5 * (1 - cos(2*pi*i / (N-1)))
            window[i] =
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos());
        }

        Self {
            sample_rate,
            num_bands,
            window,
            peak_holds: vec![0.0; num_bands],
            peak_decay_timers: vec![0; num_bands],
            waterfall: VecDeque::with_capacity(WATERFALL_HISTORY),
        }
    }

    /// Process a block of samples and compute live spectrum
    pub fn process(&mut self, samples: &[f32]) -> LiveSpectrumData {
        let mut re = [0.0f32; FFT_SIZE];
        let mut im = [0.0f32; FFT_SIZE];

        let offset = if samples.len() >= FFT_SIZE {
            samples.len() - FFT_SIZE
        } else {
            0
        };

        let mut sum_sq = 0.0f32;
        let mut peak_val = 0.0f32;

        for i in 0..FFT_SIZE {
            let s = if offset + i < samples.len() {
                samples[offset + i]
            } else {
                0.0
            };
            let abs_s = s.abs();
            if abs_s > peak_val {
                peak_val = abs_s;
            }
            sum_sq += s * s;
            re[i] = s * self.window[i];
            im[i] = 0.0;
        }

        let rms = (sum_sq / FFT_SIZE as f32).sqrt();
        let rms_db = if rms > 1e-5 {
            20.0 * rms.log10()
        } else {
            -96.0
        };

        // Perform in-place Radix-2 FFT
        fft_radix2(&mut re, &mut im);

        // Group into logarithmic frequency bands
        let bin_hz = self.sample_rate / FFT_SIZE as f32; // ~46.875 Hz
        let min_f = 50.0f32;
        let max_f = 16000.0f32;
        let ratio = (max_f / min_f).powf(1.0 / self.num_bands as f32);

        let mut bands = Vec::with_capacity(self.num_bands);
        let mut band_db = Vec::with_capacity(self.num_bands);
        let mut freqs = Vec::with_capacity(self.num_bands);

        let mut cur_low = min_f;
        for b in 0..self.num_bands {
            let cur_high = cur_low * ratio;
            let center_f = (cur_low * cur_high).sqrt();
            freqs.push(center_f);

            let k_start = ((cur_low / bin_hz).floor() as usize).clamp(1, FFT_SIZE / 2 - 1);
            let k_end = ((cur_high / bin_hz).ceil() as usize).clamp(k_start, FFT_SIZE / 2 - 1);

            let mut max_mag = 0.0f32;
            for k in k_start..=k_end {
                let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() / (FFT_SIZE as f32 / 4.0);
                if mag > max_mag {
                    max_mag = mag;
                }
            }

            let db = if max_mag > 1e-5 {
                20.0 * max_mag.log10()
            } else {
                -80.0
            }
            .clamp(-80.0, 0.0);

            let norm = ((db + 80.0) / 80.0).clamp(0.0, 1.0);
            bands.push(norm);
            band_db.push(db);

            // Peak hold ballistics
            if norm >= self.peak_holds[b] {
                self.peak_holds[b] = norm;
                self.peak_decay_timers[b] = 8; // hold for ~250ms at 30 fps
            } else if self.peak_decay_timers[b] > 0 {
                self.peak_decay_timers[b] -= 1;
            } else {
                self.peak_holds[b] = (self.peak_holds[b] - 0.035).max(0.0);
            }

            cur_low = cur_high;
        }

        // Add to waterfall
        if self.waterfall.len() >= WATERFALL_HISTORY {
            self.waterfall.pop_front();
        }
        self.waterfall.push_back(bands.clone());

        LiveSpectrumData {
            bands,
            band_db,
            peaks: self.peak_holds.clone(),
            freqs,
            waterfall: self.waterfall.clone(),
            peak_level: peak_val,
            rms_db,
        }
    }
}

/// In-place Cooley-Tukey Radix-2 DIT FFT
fn fft_radix2(re: &mut [f32; FFT_SIZE], im: &mut [f32; FFT_SIZE]) {
    let n = FFT_SIZE;
    // 1. Bit-reversal permutation
    let mut j = 0;
    for i in 0..n {
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
        let mut k = n >> 1;
        while k >= 1 && k <= j {
            j -= k;
            k >>= 1;
        }
        j += k;
    }

    // 2. Butterfly computation
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let w_step_re = angle.cos();
        let w_step_im = angle.sin();

        let mut k = 0;
        while k < n {
            let mut w_re = 1.0f32;
            let mut w_im = 0.0f32;
            for j in 0..half {
                let u_re = re[k + j];
                let u_im = im[k + j];
                let v_re = w_re * re[k + j + half] - w_im * im[k + j + half];
                let v_im = w_re * im[k + j + half] + w_im * re[k + j + half];

                re[k + j] = u_re + v_re;
                im[k + j] = u_im + v_im;
                re[k + j + half] = u_re - v_re;
                im[k + j + half] = u_im - v_im;

                let next_w_re = w_re * w_step_re - w_im * w_step_im;
                let next_w_im = w_re * w_step_im + w_im * w_step_re;
                w_re = next_w_re;
                w_im = next_w_im;
            }
            k += len;
        }
        len <<= 1;
    }
}
