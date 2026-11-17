//! Stacks layers into one RGB frame, reusing every buffer.
use super::blend::{blend, Blend};
use super::layer::{Ctx, Inputs, Layer};
use super::px::Px;

struct Slot {
    layer: Box<dyn Layer>,
    mode: Blend,
    opacity: f32,
}

pub struct Compositor {
    len: usize,
    base: Vec<Slot>,
    oneshots: Vec<Slot>,
    max_oneshots: usize,
    scratch: Vec<Px>,
    acc: Vec<[f32; 3]>,
    out: Vec<u8>,
    t: f32,
    master: f32,
}

impl Compositor {
    /// `len` LEDs; at most `max_oneshots` one-shot layers alive at once.
    pub fn new(len: usize, max_oneshots: usize) -> Self {
        Compositor {
            len,
            base: Vec::new(),
            oneshots: Vec::new(),
            max_oneshots,
            scratch: vec![Px::CLEAR; len],
            acc: vec![[0.0; 3]; len],
            out: vec![0; len * 3],
            t: 0.0,
            master: 1.0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Persistent layer, drawn in the order pushed (first = bottom).
    pub fn push(&mut self, layer: Box<dyn Layer>, mode: Blend, opacity: f32) {
        self.base.push(Slot { layer, mode, opacity });
    }

    /// One-shot layer, drawn above the persistent ones and dropped when `done()`. When the cap
    /// is reached the oldest one-shot is dropped to make room.
    pub fn spawn(&mut self, layer: Box<dyn Layer>, mode: Blend, opacity: f32) {
        if self.max_oneshots == 0 {
            return;
        }
        while self.oneshots.len() >= self.max_oneshots {
            self.oneshots.remove(0);
        }
        self.oneshots.push(Slot { layer, mode, opacity });
    }

    pub fn active_oneshots(&self) -> usize {
        self.oneshots.len()
    }

    /// Master brightness 0..1 applied last.
    pub fn set_master(&mut self, k: f32) {
        self.master = k.clamp(0.0, 1.0);
    }

    /// Render one frame; returns `len * 3` RGB bytes valid until the next call.
    pub fn render(&mut self, dt: f32, inputs: &Inputs) -> &[u8] {
        self.t += dt;
        self.acc.iter_mut().for_each(|p| *p = [0.0; 3]);
        let ctx = Ctx { dt, t: self.t, inputs };
        for slot in self.base.iter_mut().chain(self.oneshots.iter_mut()) {
            self.scratch.iter_mut().for_each(|p| *p = Px::CLEAR);
            slot.layer.render(&ctx, &mut self.scratch);
            for (acc, px) in self.acc.iter_mut().zip(&self.scratch) {
                *acc = blend(*acc, *px, slot.mode, slot.opacity);
            }
        }
        self.oneshots.retain(|s| !s.layer.done());
        for (i, c) in self.acc.iter().enumerate() {
            for (k, v) in c.iter().enumerate() {
                self.out[i * 3 + k] = ((v * self.master).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        &self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fills every pixel with a colour; `life` frames for one-shots (None = persistent).
    struct Fill {
        c: [f32; 3],
        alpha: f32,
        life: Option<u32>,
    }

    impl Layer for Fill {
        fn name(&self) -> &'static str {
            "fill"
        }
        fn render(&mut self, _ctx: &Ctx, out: &mut [Px]) {
            out.iter_mut().for_each(|p| *p = Px::rgba(self.c[0], self.c[1], self.c[2], self.alpha));
            if let Some(l) = self.life.as_mut() {
                *l = l.saturating_sub(1);
            }
        }
        fn done(&self) -> bool {
            self.life == Some(0)
        }
    }

    fn fill(c: [f32; 3]) -> Box<dyn Layer> {
        Box::new(Fill { c, alpha: 1.0, life: None })
    }

    fn pixel(frame: &[u8], i: usize) -> [u8; 3] {
        [frame[i * 3], frame[i * 3 + 1], frame[i * 3 + 2]]
    }

    #[test]
    fn layers_stack_bottom_to_top() {
        let mut c = Compositor::new(4, 8);
        c.push(fill([0.0, 0.0, 1.0]), Blend::Normal, 1.0);
        c.push(fill([1.0, 0.0, 0.0]), Blend::Normal, 0.5);
        assert_eq!(pixel(c.render(0.033, &Inputs::default()), 0), [128, 0, 128]);
        assert_eq!(c.render(0.033, &Inputs::default()).len(), 12);
    }

    #[test]
    fn output_is_clamped_and_master_scales() {
        let mut c = Compositor::new(2, 8);
        c.push(fill([0.8, 0.8, 0.8]), Blend::Add, 1.0);
        c.push(fill([0.8, 0.8, 0.8]), Blend::Add, 1.0);
        assert_eq!(pixel(c.render(0.033, &Inputs::default()), 0), [255, 255, 255]);
        // master scales the summed light (1.6 * 0.5 = 0.8) before the final clamp
        c.set_master(0.5);
        assert_eq!(pixel(c.render(0.033, &Inputs::default()), 1), [204, 204, 204]);
    }

    #[test]
    fn oneshots_draw_on_top_then_vanish() {
        let mut c = Compositor::new(3, 8);
        c.push(fill([0.0, 0.0, 0.0]), Blend::Normal, 1.0);
        c.spawn(Box::new(Fill { c: [1.0, 1.0, 1.0], alpha: 1.0, life: Some(2) }), Blend::Normal, 1.0);
        assert_eq!(c.active_oneshots(), 1);
        assert_eq!(pixel(c.render(0.033, &Inputs::default()), 0), [255, 255, 255]);
        assert_eq!(pixel(c.render(0.033, &Inputs::default()), 0), [255, 255, 255], "draws its last frame");
        assert_eq!(c.active_oneshots(), 0, "removed once done");
        assert_eq!(pixel(c.render(0.033, &Inputs::default()), 0), [0, 0, 0]);
    }

    #[test]
    fn oneshot_cap_drops_the_oldest() {
        let mut c = Compositor::new(1, 2);
        for v in [0.2, 0.4, 0.6] {
            c.spawn(Box::new(Fill { c: [v, 0.0, 0.0], alpha: 1.0, life: None }), Blend::Max, 1.0);
        }
        assert_eq!(c.active_oneshots(), 2);
        // 0.2 was dropped; the brightest remaining under Max is 0.6
        assert_eq!(pixel(c.render(0.033, &Inputs::default()), 0)[0], 153);
        let mut none = Compositor::new(1, 0);
        none.spawn(fill([1.0; 3]), Blend::Normal, 1.0);
        assert_eq!(none.active_oneshots(), 0);
    }

    #[test]
    fn rendering_reuses_its_buffers() {
        let mut c = Compositor::new(180, 4);
        c.push(fill([0.3; 3]), Blend::Normal, 1.0);
        let (a, s, o) = (c.acc.capacity(), c.scratch.capacity(), c.out.capacity());
        for _ in 0..500 {
            c.spawn(Box::new(Fill { c: [1.0; 3], alpha: 1.0, life: Some(3) }), Blend::Add, 1.0);
            c.render(0.033, &Inputs::default());
        }
        assert_eq!((c.acc.capacity(), c.scratch.capacity(), c.out.capacity()), (a, s, o));
        assert!(c.active_oneshots() <= 4);
    }
}
