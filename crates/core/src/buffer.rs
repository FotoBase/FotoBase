//! Linear-light image buffer for the edit-recipe engine
//! (docs/edit-recipe-spec-v1.md §2: ops are pure functions over a
//! linear-light buffer; all geometry is normalized to fractions of
//! width/height).

/// Pixel data in linear light, interleaved RGB, one `f32` per channel.
/// Nominal range is [0, 1]; ops clamp on output.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 3` samples, row-major.
    pub data: Vec<f32>,
}

impl LinearImage {
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width > 0 && height > 0, "empty images are not renderable");
        Self { width, height, data: vec![0.0; width as usize * height as usize * 3] }
    }

    pub fn from_fn(width: u32, height: u32, f: impl Fn(u32, u32) -> [f32; 3]) -> Self {
        let mut img = Self::new(width, height);
        for y in 0..height {
            for x in 0..width {
                img.set(x, y, f(x, y));
            }
        }
        img
    }

    pub fn get(&self, x: u32, y: u32) -> [f32; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    pub fn set(&mut self, x: u32, y: u32, px: [f32; 3]) {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        self.data[i] = px[0];
        self.data[i + 1] = px[1];
        self.data[i + 2] = px[2];
    }
}
