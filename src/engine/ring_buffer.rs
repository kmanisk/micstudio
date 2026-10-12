//! Lock-free Single-Producer Single-Consumer (SPSC) Audio Ring Buffer.
//! Safe for real-time audio threads: zero allocations, zero mutexes, zero syscalls.
//! Uses Dusky circular overwrite model: producer never blocks or drops,
//! consumer automatically stays bounded to ultra-low latency (~10ms).

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscRingBuffer {
    buffer: Box<[f32]>,
    capacity: usize,
    mask: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl SpscRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let size = capacity.next_power_of_two().max(1024);
        Self {
            buffer: vec![0.0; size].into_boxed_slice(),
            capacity: size,
            mask: size - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    pub fn push(&self, sample: f32) -> bool {
        let h = self.head.load(Ordering::Relaxed);
        unsafe {
            let ptr = self.buffer.as_ptr() as *mut f32;
            *ptr.add(h & self.mask) = sample;
        }
        self.head.store(h + 1, Ordering::Release);
        true
    }

    #[inline(always)]
    pub fn pop(&self) -> Option<f32> {
        let h = self.head.load(Ordering::Acquire);
        let mut t = self.tail.load(Ordering::Relaxed);

        if h > t {
            if h - t > 960 {
                t = h.saturating_sub(480);
            }
        }

        if t >= h {
            return None;
        }

        let sample = unsafe {
            let ptr = self.buffer.as_ptr();
            *ptr.add(t & self.mask)
        };

        self.tail.store(t + 1, Ordering::Release);
        Some(sample)
    }

    #[inline(always)]
    pub fn pop_slice(&self, slice: &mut [f32]) {
        let h = self.head.load(Ordering::Acquire);
        let mut t = self.tail.load(Ordering::Relaxed);

        if h > t {
            if h - t > 960 + slice.len() {
                t = h.saturating_sub(480 + slice.len());
            }
        }

        for out in slice.iter_mut() {
            if t < h {
                *out = unsafe {
                    let ptr = self.buffer.as_ptr();
                    *ptr.add(t & self.mask)
                };
                t += 1;
            } else {
                *out = 0.0;
            }
        }
        self.tail.store(t, Ordering::Release);
    }

    pub fn available_read(&self) -> usize {
        let h = self.head.load(Ordering::Acquire);
        let t = self.tail.load(Ordering::Relaxed);
        h.saturating_sub(t).min(self.capacity)
    }

    pub fn clear(&self) {
        let h = self.head.load(Ordering::Relaxed);
        self.tail.store(h, Ordering::Release);
    }
}

// Safety: Single-Producer Single-Consumer guarantee across audio threads
unsafe impl Send for SpscRingBuffer {}
unsafe impl Sync for SpscRingBuffer {}
