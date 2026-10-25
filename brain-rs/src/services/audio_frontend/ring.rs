//! Fixed-capacity ring buffer used for audio and feature history.
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct RingBuffer<T> {
    buf: VecDeque<T>,
    cap: usize,
}

impl<T: Clone> RingBuffer<T> {
    pub fn new(cap: usize) -> Self {
        Self { buf: VecDeque::with_capacity(cap), cap }
    }
    pub fn push_slice(&mut self, items: &[T]) {
        for it in items {
            self.push(it.clone());
        }
    }
    pub fn push(&mut self, item: T) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(item);
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.cap
    }
    /// Most recent `n` items (or fewer), oldest first.
    pub fn last_n(&self, n: usize) -> Vec<T> {
        self.buf.iter().skip(self.buf.len().saturating_sub(n)).cloned().collect()
    }
    #[cfg(test)]
    pub fn snapshot(&self) -> Vec<T> {
        self.buf.iter().cloned().collect()
    }
}
