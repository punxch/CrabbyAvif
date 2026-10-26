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

// The dav1d-style API here is implemented by rav1d in Rust. Its types must never
// be mixed with dav1d-sys, despite the identical exported function names.
use crate::codecs::{Decoder, DecoderConfig};
use crate::decoder::{item::Item, GridImageHelper};
use crate::image::{Image, YuvRange};
use crate::internal_utils::u32_from_usize;
use crate::utils::pixels::Pixels;
use crate::*;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::*;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

const EAGAIN: i32 = -libc::EAGAIN;

#[derive(Default)]
pub struct Rav1d {
    context: Option<Dav1dContext>,
    // Image planes borrow this picture until the next successful decode or Drop.
    picture: Option<Picture>,
}

#[derive(Default)]
struct Picture(Dav1dPicture);

impl Picture {
    fn use_layer(&self, spatial_id: u8) -> AvifResult<bool> {
        let header = self.0.frame_hdr.ok_or(AvifError::NoContent)?;
        // SAFETY: A successful get_picture owns the frame header until picture_unref.
        Ok(spatial_id == 0xff || spatial_id == unsafe { header.as_ref().spatial_id })
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: The picture is initialized, uniquely owned, and unreferenced exactly once.
        unsafe { dav1d_picture_unref(Some(NonNull::from(&mut self.0))) };
    }
}

#[derive(Default)]
struct Data(Dav1dData);

impl Data {
    fn new(payload: &[u8]) -> AvifResult<Self> {
        if payload.is_empty() {
            return AvifError::unknown_error("rav1d received an empty AV1 sample");
        }
        let mut data = Self::default();
        // SAFETY: data is writable; rav1d allocates payload.len() bytes on success.
        let ptr = unsafe { dav1d_data_create(Some(NonNull::from(&mut data.0)), payload.len()) };
        if ptr.is_null() {
            return Err(AvifError::OutOfMemory);
        }
        // SAFETY: The new allocation has the requested length and does not alias payload.
        unsafe { std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, payload.len()) };
        Ok(data)
    }

    fn has_data(&self) -> bool {
        self.0.sz > 0 && self.0.data.is_some()
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        // SAFETY: data is initialized. send_data clears consumed references; unref also
        // handles the empty value, and releases any input left over on an error path.
        unsafe { dav1d_data_unref(Some(NonNull::from(&mut self.0))) };
    }
}

impl Rav1d {
    fn decode_picture(&mut self, payload: &[u8], spatial_id: u8) -> AvifResult<Picture> {
        if self.context.is_none() {
            return AvifError::unknown_error("rav1d decoder is not initialized");
        }
        let mut data = Data::new(payload)?;
        let mut selected = None;
        loop {
            let mut sent = false;
            if data.has_data() {
                // SAFETY: context came from open and remains live; data is uniquely owned.
                let res =
                    unsafe { dav1d_send_data(self.context, Some(NonNull::from(&mut data.0))) }.0;
                if res != 0 && res != EAGAIN {
                    return AvifError::unknown_error(format!("rav1d send_data returned {res}"));
                }
                sent = res == 0;
            }
            let mut picture = Picture::default();
            // SAFETY: context is live and picture is a new, writable, empty output.
            let res =
                unsafe { dav1d_get_picture(self.context, Some(NonNull::from(&mut picture.0))) }.0;
            if res == EAGAIN {
                if !data.has_data() {
                    break;
                }
                if !sent {
                    return AvifError::unknown_error("rav1d made no progress consuming AV1 sample");
                }
            } else if res != 0 {
                return AvifError::unknown_error(format!("rav1d get_picture returned {res}"));
            } else if selected.is_none() && picture.use_layer(spatial_id)? {
                selected = Some(picture);
            }
            // Drain the remaining spatial layers without flushing prediction state.
            // max_frame_delay=1 ensures a sample's output is ready before the next input.
        }
        selected.ok_or_else(|| AvifError::UnknownError("rav1d returned no matching frame".into()))
    }

    fn decode(&mut self, payload: &[u8], spatial_id: u8) -> AvifResult<()> {
        match self.decode_picture(payload, spatial_id) {
            Ok(picture) => {
                self.picture = Some(picture);
                Ok(())
            }
            Err(error) => {
                // Do not reuse potentially corrupted prediction state after a decode error.
                // Keep the previous output picture alive for existing image plane pointers.
                self.close();
                Err(error)
            }
        }
    }

    fn close(&mut self) {
        if self.context.is_some() {
            // SAFETY: close consumes the one context reference returned by open and clears it.
            unsafe { dav1d_close(Some(NonNull::from(&mut self.context))) };
        }
    }

    pub(crate) fn version() -> String {
        format!("rav1d: {}", rav1d_version())
    }

    fn picture_to_image(
        &self,
        picture: &Dav1dPicture,
        image: &mut Image,
        category: Category,
        overwrite_alpha_plane_yuv_range: bool,
    ) -> AvifResult<()> {
        match category {
            Category::Alpha => {
                if image.width > 0
                    && image.height > 0
                    && (image.width != (picture.p.w as u32)
                        || image.height != (picture.p.h as u32)
                        || image.depth != (picture.p.bpc as u8))
                {
                    // Alpha plane does not match the previous alpha plane.
                    return AvifError::unknown_error("");
                }
                image.width = picture.p.w as u32;
                image.height = picture.p.h as u32;
                image.depth = picture.p.bpc as u8;
                image.row_bytes[3] =
                    u32::try_from(picture.stride[0]).map_err(|_| AvifError::NoContent)?;
                image.planes[3] = Some(Pixels::from_raw_pointer(
                    picture.data[0].ok_or(AvifError::NoContent)?.as_ptr().cast(),
                    image.depth as u32,
                    image.height,
                    image.row_bytes[3],
                )?);
                if overwrite_alpha_plane_yuv_range {
                    // # Safety: seq_hdr is populated by rav1d and is guaranteed to be valid.
                    let seq_hdr = unsafe { picture.seq_hdr.ok_or(AvifError::NoContent)?.as_ref() };
                    image.yuv_range =
                        if seq_hdr.color_range == 0 { YuvRange::Limited } else { YuvRange::Full };
                }
            }
            _ => {
                image.width = picture.p.w as u32;
                image.height = picture.p.h as u32;
                image.depth = picture.p.bpc as u8;

                image.yuv_format = match picture.p.layout {
                    0 => PixelFormat::Yuv400,
                    1 => PixelFormat::Yuv420,
                    2 => PixelFormat::Yuv422,
                    3 => PixelFormat::Yuv444,
                    _ => return AvifError::unknown_error(""), // not reached.
                };
                // # Safety: seq_hdr is populated by rav1d and is guaranteed to be valid.
                let seq_hdr = unsafe { picture.seq_hdr.ok_or(AvifError::NoContent)?.as_ref() };
                image.yuv_range =
                    if seq_hdr.color_range == 0 { YuvRange::Limited } else { YuvRange::Full };
                image.chroma_sample_position = seq_hdr.chr.into();

                image.color_primaries = (seq_hdr.pri as u16).into();
                image.transfer_characteristics = (seq_hdr.trc as u16).into();
                image.matrix_coefficients = (seq_hdr.mtrx as u16).into();

                for plane in 0usize..image.yuv_format.plane_count() {
                    let stride_index = if plane == 0 { 0 } else { 1 };
                    image.row_bytes[plane] = u32::try_from(picture.stride[stride_index])
                        .map_err(|_| AvifError::NoContent)?;
                    image.planes[plane] = Some(Pixels::from_raw_pointer(
                        picture.data[plane]
                            .ok_or(AvifError::NoContent)?
                            .as_ptr()
                            .cast(),
                        image.depth as u32,
                        u32_from_usize(image.height(plane.into()))?,
                        image.row_bytes[plane],
                    )?);
                }
                if image.yuv_format == PixelFormat::Yuv400 {
                    // Clear left over chroma planes from previous frames.
                    image.clear_chroma_planes();
                }
            }
        }
        Ok(())
    }
}

impl Decoder for Rav1d {
    fn codec(&self) -> CodecChoice {
        CodecChoice::Rav1d
    }

    fn initialize(&mut self, config: &DecoderConfig) -> AvifResult<()> {
        self.close();
        let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: settings points to writable storage; default_settings initializes all fields.
        unsafe { dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap()) };
        // SAFETY: The preceding call initialized settings.
        let mut settings = unsafe { settings.assume_init() };
        settings.n_threads = config.max_threads.clamp(1, 256) as i32;
        settings.max_frame_delay = 1;
        settings.operating_point = i32::from(config.operating_point);
        settings.all_layers = i32::from(config.all_layers);
        settings.frame_size_limit = config.image_size_limit.map_or(0, |limit| limit.get());
        if cfg!(target_pointer_width = "32") {
            // Zero means unlimited, so enforce the 32-bit cap in that case as well.
            settings.frame_size_limit = if settings.frame_size_limit == 0 {
                8192 * 8192
            } else {
                settings.frame_size_limit.min(8192 * 8192)
            };
        }
        // SAFETY: context is an empty writable output and settings was fully initialized.
        let res = unsafe {
            dav1d_open(
                Some(NonNull::from(&mut self.context)),
                Some(NonNull::from(&mut settings)),
            )
        }
        .0;
        if res != 0 {
            return AvifError::unknown_error(format!("rav1d open returned {res}"));
        }
        Ok(())
    }

    fn get_next_image(
        &mut self,
        av1_payload: &[u8],
        spatial_id: u8,
        image: &mut Image,
        category: Category,
        _item: Option<&Item>,
        _signal_eos: bool,
    ) -> AvifResult<()> {
        self.decode(av1_payload, spatial_id)?;
        self.picture_to_image(&self.picture.as_ref().unwrap().0, image, category, true)
    }

    fn get_next_image_grid(
        &mut self,
        payloads: &[Vec<u8>],
        spatial_id: u8,
        grid_image_helper: &mut GridImageHelper,
    ) -> AvifResult<()> {
        for payload in payloads {
            self.decode(payload, spatial_id)?;
            let mut cell = Image::default();
            self.picture_to_image(
                &self.picture.as_ref().unwrap().0,
                &mut cell,
                grid_image_helper.category,
                true,
            )?;
            // Copy while the picture is still alive, before decoding the next cell.
            grid_image_helper.copy_from_cell_image(&mut cell)?;
        }
        Ok(())
    }

    fn get_last_image(
        &mut self,
        payloads: &[Vec<u8>],
        spatial_id: u8,
        image: &mut Image,
        category: Category,
    ) -> AvifResult<()> {
        if payloads.is_empty() {
            return AvifError::no_content();
        }
        for payload in payloads {
            self.decode(payload, spatial_id)?;
        }
        // The caller shares this image between color and alpha; preserve the color range.
        self.picture_to_image(&self.picture.as_ref().unwrap().0, image, category, false)
    }
}

impl Drop for Rav1d {
    fn drop(&mut self) {
        self.picture = None;
        self.close();
    }
}
