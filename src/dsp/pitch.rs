//! High-Fidelity Granular Overlap-Add Pitch Shifter.
//! Smooth, duration-preserving pitch transposition (-24 to +24 semitones)
//! with 4-point Hermite cubic interpolation and Hann crossfading.

pub const PITCH_BUFFER_SIZE: usize = 16384; // ~341 ms at 48 kHz
pub const GRAIN_SIZE: usize = 2048;         // ~42.7 ms at 48 kHz

pub struct PitchShifter {
    buffer: [f32; PITCH_BUFFER_SIZE],
    write_idx: usize,
    phase1: f32,
    semitones: f32,
    ratio: f32,
    hann_window: [f32; GRAIN_SIZE],
    enabled: bool,
}

impl PitchShifter {
    pub fn new() -> Self {
        let mut hann = [0.0; GRAIN_SIZE];
        for i in 0..GRAIN_SIZE {
            hann[i] = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / GRAIN_SIZE as f32).cos());
        }

        Self {
            buffer: [0.0; PITCH_BUFFER_SIZE],
            write_idx: 0,
            phase1: 0.0,
            semitones: 0.0,
            ratio: 1.0,
            hann_window: hann,
            enabled: false,
        }
    }

    pub fn set_semitones(&mut self, semitones: f32) {
        self.semitones = semitones.clamp(-24.0, 24.0);
        self.ratio = 2.0f32.powf(self.semitones / 12.0);
        self.enabled = self.semitones.abs() > 0.05;
    }

    pub fn semitones(&self) -> f32 {
        self.semitones
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[inline(always)]
    fn hermite_interpolate(y0: f32, y1: f32, y2: f32, y3: f32, x: f32) -> f32 {
        let c0 = y1;
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * x + c2) * x + c1) * x + c0
    }

    #[inline(always)]
    fn read_interpolated(&self, read_pos: f32) -> f32 {
        let buf_len = PITCH_BUFFER_SIZE as f32;
        let pos_norm = ((read_pos % buf_len) + buf_len) % buf_len;
        let i1 = pos_norm as usize;
        let frac = pos_norm - (i1 as f32);

        let i0 = (i1 + PITCH_BUFFER_SIZE - 1) % PITCH_BUFFER_SIZE;
        let i2 = (i1 + 1) % PITCH_BUFFER_SIZE;
        let i3 = (i1 + 2) % PITCH_BUFFER_SIZE;

        Self::hermite_interpolate(
            self.buffer[i0],
            self.buffer[i1],
            self.buffer[i2],
            self.buffer[i3],
            frac,
        )
    }

    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        if !self.enabled {
            return input;
        }

        // Store input sample into ring buffer
        let w = self.write_idx;
        self.buffer[w] = input;
        self.write_idx = (w + 1) % PITCH_BUFFER_SIZE;

        let g = GRAIN_SIZE as f32;
        let half_g = g * 0.5;

        // Phase 1 and Phase 2 (Tap 2 is 180° out of phase)
        let p1 = self.phase1;
        let p2 = if p1 + half_g >= g { p1 + half_g - g } else { p1 + half_g };

        let idx1 = (p1 as usize).min(GRAIN_SIZE - 1);
        let idx2 = (p2 as usize).min(GRAIN_SIZE - 1);
        let w1 = self.hann_window[idx1];
        let w2 = self.hann_window[idx2];

        // Base delay safety margin to avoid reading ahead of the write head
        let base_delay = 64.0f32;

        // Delay values for the two grains:
        // When delay slope is (1 - ratio), the read pointer advances through the buffer at rate `ratio`.
        let delay1 = base_delay + p1;
        let delay2 = base_delay + p2;

        let read_pos1 = (w as f32) - delay1;
        let read_pos2 = (w as f32) - delay2;

        let s1 = self.read_interpolated(read_pos1) * w1;
        let s2 = self.read_interpolated(read_pos2) * w2;

        // Advance phase slope only when signal is present to synchronize grains to voice onset
        if input.abs() > 1e-4 || (s1.abs() + s2.abs()) > 1e-4 {
            let slope = 1.0 - self.ratio;
            self.phase1 += slope;
            while self.phase1 >= g {
                self.phase1 -= g;
            }
            while self.phase1 < 0.0 {
                self.phase1 += g;
            }
        }

        s1 + s2
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_idx = 0;
        self.phase1 = 0.0;
    }
}
