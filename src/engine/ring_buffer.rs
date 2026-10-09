//! Lock-free Single-Producer Single-Consumer (SPSC) Audio Ring Buffer.
//! Safe for real-time audio threads: zero allocations, zero mutexes, zero syscalls.

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscRingBuffer {
    buffer: Box<[f32]>,
    capacity: usize,
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
}

impl SpscRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let size = capacity.next_power_of_two().max(1024);
        Self {
            buffer: vec![0.0; size].into_boxed_slice(),
            capacity: size,
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    pub fn push(&self, sample: f32) -> bool {
        let w = self.write_idx.load(Ordering::Relaxed);
        let r = self.read_idx.load(Ordering::Acquire);

        let next_w = (w + 1) & (self.capacity - 1);
        if next_w == r {
            // Buffer full (overflow)
            return false;
        }

        // Unsafe cast to mutate buffer slot in single-producer context
        unsafe {
            let ptr = self.buffer.as_ptr() as *mut f32;
            *ptr.add(w) = sample;
        }

        self.write_idx.store(next_w, Ordering::Release);
        true
    }

    #[inline(always)]
    pub fn pop(&self) -> Option<f32> {
        let r = self.read_idx.load(Ordering::Relaxed);
        let w = self.write_idx.load(Ordering::Acquire);

        if r == w {
            // Buffer empty (underflow)
            return None;
        }

        let sample = unsafe {
            let ptr = self.buffer.as_ptr();
            *ptr.add(r)
        };

        let next_r = (r + 1) & (self.capacity - 1);
        self.read_idx.store(next_r, Ordering::Release);
        Some(sample)
    }

    pub fn available_read(&self) -> usize {
        let w = self.write_idx.load(Ordering::Acquire);
        let r = self.read_idx.load(Ordering::Relaxed);
        (w + self.capacity - r) & (self.capacity - 1)
    }

    pub fn clear(&self) {
        self.write_idx.store(0, Ordering::Relaxed);
        self.read_idx.store(0, Ordering::Release);
    }
}

// Safety: Single-Producer Single-Consumer guarantee across audio threads
unsafe impl Send for SpscRingBuffer {}
unsafe impl Sync for SpscRingBuffer {}
