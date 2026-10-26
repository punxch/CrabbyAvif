// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Planar fallback for builds without libyuv. Use bilinear interpolation with
// pixel-center alignment and clamped edges for both 8-bit and high-bit-depth AV1.
use crate::image::*;
use crate::*;

fn position(index: u32, source: usize, destination: usize) -> (usize, usize, u64) {
    let position = (((2 * u64::from(index) + 1) * source as u64 * 32768) / destination as u64)
        .saturating_sub(32768)
        .min((source as u64 - 1) * 65536);
    let first = (position >> 16) as usize;
    (first, (first + 1).min(source - 1), position & 65535)
}

fn interpolate(a: u16, b: u16, c: u16, d: u16, x: u64, y: u64) -> u16 {
    let top = u64::from(a) * (65536 - x) + u64::from(b) * x;
    let bottom = u64::from(c) * (65536 - x) + u64::from(d) * x;
    ((top * (65536 - y) + bottom * y + (1 << 31)) >> 32) as u16
}

impl Image {
    pub(crate) fn scale(&mut self, width: u32, height: u32, category: Category) -> AvifResult<()> {
        if self.width == width && self.height == height {
            return Ok(());
        }
        if width == 0 || height == 0 || self.width == 0 || self.height == 0 {
            return AvifError::invalid_argument();
        }
        // Match the native scaler's source limit; also bound destination coordinate math.
        if [self.width, self.height, width, height]
            .into_iter()
            .any(|v| v > 65536)
        {
            return AvifError::not_implemented();
        }
        if !matches!(
            self.yuv_format,
            PixelFormat::Yuv400 | PixelFormat::Yuv420 | PixelFormat::Yuv422 | PixelFormat::Yuv444
        ) {
            return AvifError::not_implemented();
        }
        let mut destination = self.shallow_clone();
        destination.width = width;
        destination.height = height;
        destination.allocate_planes(category)?;
        for &plane in category.planes() {
            if !self.has_plane(plane) {
                continue;
            }
            let dst_width = destination.width(plane);
            let dst_height = destination.height(plane);
            for y in 0..dst_height as u32 {
                let (y0, y1, fraction_y) = position(y, self.height(plane), dst_height);
                if self.depth > 8 {
                    let top = self.row16(plane, y0 as u32)?;
                    let bottom = self.row16(plane, y1 as u32)?;
                    let row = destination.row16_mut(plane, y)?;
                    for (x, pixel) in row[..dst_width].iter_mut().enumerate() {
                        let (x0, x1, fraction_x) = position(x as u32, self.width(plane), dst_width);
                        *pixel = interpolate(
                            top[x0], top[x1], bottom[x0], bottom[x1], fraction_x, fraction_y,
                        );
                    }
                } else {
                    let top = self.row(plane, y0 as u32)?;
                    let bottom = self.row(plane, y1 as u32)?;
                    let row = destination.row_mut(plane, y)?;
                    for (x, pixel) in row[..dst_width].iter_mut().enumerate() {
                        let (x0, x1, fraction_x) = position(x as u32, self.width(plane), dst_width);
                        *pixel = interpolate(
                            top[x0].into(),
                            top[x1].into(),
                            bottom[x0].into(),
                            bottom[x1].into(),
                            fraction_x,
                            fraction_y,
                        ) as u8;
                    }
                }
            }
        }
        // Commit only after all allocations and row accesses succeeded. The source may
        // borrow decoder-owned planes; it must stay valid throughout interpolation.
        for &plane in category.planes() {
            if self.has_plane(plane) {
                self.planes[plane.as_usize()] = destination.planes[plane.as_usize()].take();
                self.row_bytes[plane.as_usize()] = destination.row_bytes[plane.as_usize()];
            }
        }
        self.width = width;
        self.height = height;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilinear_centers_and_clamped_edges() -> AvifResult<()> {
        for depth in [8, 10, 12, 16] {
            for category in [Category::Color, Category::Alpha] {
                let mut image = Image {
                    width: 2,
                    height: 2,
                    depth,
                    yuv_format: PixelFormat::Yuv400,
                    ..Default::default()
                };
                image.allocate_planes(category)?;
                let plane = if category == Category::Alpha { Plane::A } else { Plane::Y };
                if depth > 8 {
                    image.row16_mut(plane, 0)?[..2].copy_from_slice(&[0, 100]);
                    image.row16_mut(plane, 1)?[..2].copy_from_slice(&[200, 300]);
                } else {
                    image.row_mut(plane, 0)?[..2].copy_from_slice(&[0, 10]);
                    image.row_mut(plane, 1)?[..2].copy_from_slice(&[20, 30]);
                }
                image.scale(3, 3, category)?;
                if depth > 8 {
                    assert_eq!(&image.row16(plane, 0)?[..3], &[0, 50, 100]);
                    assert_eq!(&image.row16(plane, 1)?[..3], &[100, 150, 200]);
                    assert_eq!(&image.row16(plane, 2)?[..3], &[200, 250, 300]);
                } else {
                    assert_eq!(&image.row(plane, 0)?[..3], &[0, 5, 10]);
                    assert_eq!(&image.row(plane, 1)?[..3], &[10, 15, 20]);
                    assert_eq!(&image.row(plane, 2)?[..3], &[20, 25, 30]);
                }
                image.scale(1, 1, category)?;
                if depth > 8 {
                    assert_eq!(image.row16(plane, 0)?[0], 150);
                } else {
                    assert_eq!(image.row(plane, 0)?[0], 15);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn odd_subsampled_planes_and_invalid_dimensions() -> AvifResult<()> {
        let mut image = Image {
            width: 5,
            height: 3,
            depth: 8,
            yuv_format: PixelFormat::Yuv420,
            ..Default::default()
        };
        image.allocate_planes(Category::Color)?;
        for plane in [Plane::Y, Plane::U, Plane::V] {
            for y in 0..image.height(plane) as u32 {
                image.row_mut(plane, y)?.fill(123);
            }
        }
        assert!(image.scale(0, 1, Category::Color).is_err());
        assert_eq!((image.width, image.height), (5, 3));
        image.scale(3, 5, Category::Color)?;
        assert_eq!((image.width(Plane::U), image.height(Plane::U)), (2, 3));
        for plane in [Plane::Y, Plane::U, Plane::V] {
            for y in 0..image.height(plane) as u32 {
                assert!(image.row(plane, y)?[..image.width(plane)]
                    .iter()
                    .all(|&v| v == 123));
            }
        }
        Ok(())
    }
}
