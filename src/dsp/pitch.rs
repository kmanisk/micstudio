//! Real-Time Granular Overlap-Add Pitch Shifter.
//! Smooth pitch transposition (-24 to +24 semitones) without FFT latency.

pub const PITCH_BUFFER_SIZE: usize = 8192;
pub const GRAIN_SIZE: usize = 2048; // ~42 ms at 48 kHz

pub struct PitchShifter {
    buffer: [f32; PITCH_BUFFER_SIZE],
    write_idx: usize,
    phase1: f32,
    phase2: f32,
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
            phase2: (GRAIN_SIZE / 2) as f32,
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
    pub fn process(&mut self, input: f32) -> f32 {
        if !self.enabled {
            return input;
        }

        // Store input in circular ring buffer
        self.buffer[self.write_idx] = input;
        self.write_idx = (self.write_idx + 1) % PITCH_BUFFER_SIZE;

        let grain_len = GRAIN_SIZE as f32;
        let p1_idx = self.phase1 as usize;
        let p2_idx = self.phase2 as usize;

        let w1 = self.hann_window[p1_idx.min(GRAIN_SIZE - 1)];
        let w2 = self.hann_window[p2_idx.min(GRAIN_SIZE - 1)];

        // Compute read positions relative to write head
        let offset1 = (self.write_idx as f32 - p1_idx as f32 + PITCH_BUFFER_SIZE as f32) as usize % PITCH_BUFFER_SIZE;
        let offset2 = (self.write_idx as f32 - p2_idx as f32 + PITCH_BUFFER_SIZE as f32) as usize % PITCH_BUFFER_SIZE;

        let s1 = self.buffer[offset1] * w1;
        let s2 = self.buffer[offset2] * w2;

        // Advance phases by pitch ratio
        self.phase1 += self.ratio;
        if self.phase1 >= grain_len {
            self.phase1 -= grain_len;
        }

        self.phase2 += self.ratio;
        if self.phase2 >= grain_len {
            self.phase2 -= grain_len;
        }

        s1 + s2
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_idx = 0;
        self.phase1 = 0.0;
        self.phase2 = (GRAIN_SIZE / 2) as f32;
    }
}
