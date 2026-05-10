use std::ops::{Deref, DerefMut};

/// Wrapper to mark a type as Send (use carefully!)
pub struct SendWrapper<T>(T);

impl<T> SendWrapper<T> {
    pub fn new(inner: T) -> Self {
        Self(inner)
    }
}

unsafe impl<T> Send for SendWrapper<T> {}

impl<T> Deref for SendWrapper<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for SendWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
