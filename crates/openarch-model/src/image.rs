//! Images (Insert > Image): a PNG or JPEG placed in a view or on a sheet, kept in the project. Its size is on paper,
//! as an annotation's (a logo stays the same size at any view scale); it keeps its proportions.

use serde::{Deserialize, Serialize};

use crate::doc::Document;
use crate::element::{Element, Id};
use crate::geom::Point;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageItem {
    /// The view, or the sheet, it is placed in.
    pub view: Id,
    /// Its centre: view mm, or paper mm on a sheet.
    pub at: Point,
    /// Width on paper, mm; the height follows the picture's proportions.
    pub width_mm: f64,
    /// The picture's size in pixels.
    pub px: [u32; 2],
    /// "image/png" or "image/jpeg".
    pub mime: String,
    /// The file, base64.
    pub data: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pinned: bool,
}

impl ImageItem {
    pub fn height_mm(&self) -> f64 {
        if self.px[0] == 0 { self.width_mm } else { self.width_mm * self.px[1] as f64 / self.px[0] as f64 }
    }
}

/// The kind and pixel size of a PNG or JPEG file, or why it can't be used.
pub fn image_kind(bytes: &[u8]) -> Result<(&'static str, [u32; 2]), String> {
    if bytes.len() > 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        return Ok(("image/png", [be(16), be(20)]));
    }
    if bytes.len() > 4 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        // walk the markers to a start of frame
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff { i += 1; continue; }
            let m = bytes[i + 1];
            if matches!(m, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Ok(("image/jpeg", [w, h]));
            }
            if m == 0xd8 || m == 0x01 || (0xd0..=0xd7).contains(&m) { i += 2; continue; }
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            i += 2 + len;
        }
        return Err("That JPEG has no picture size in it.".into());
    }
    Err("Images are PNG or JPEG files.".into())
}

pub fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

impl Document {
    pub fn image(&self, id: Id) -> Option<&ImageItem> {
        match self.get(id) {
            Some(Element::ImageItem(i)) => Some(i),
            _ => None,
        }
    }

    pub fn images_in(&self, view: Id) -> Vec<(Id, &ImageItem)> {
        self.iter().filter_map(|(id, e)| match e { Element::ImageItem(i) if i.view == view => Some((id, i)), _ => None }).collect()
    }

    /// Place a picture (the file's bytes) in a view or on a sheet, `width_mm` wide on paper (100 mm when not given,
    /// less for a small picture), one undo step. Pictures over 10 MB are refused.
    pub fn add_image(&mut self, view: Id, bytes: &[u8], name: &str, at: Point, width_mm: Option<f64>) -> Result<Id, String> {
        self.check_detail_view(view)?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err("That picture is over 10 MB: make it smaller first.".into());
        }
        let (mime, px) = image_kind(bytes)?;
        if px[0] == 0 || px[1] == 0 {
            return Err("That picture has no size.".into());
        }
        let width_mm = width_mm.unwrap_or_else(|| (px[0] as f64 * 25.4 / 96.0).clamp(10.0, 100.0));
        check_width(width_mm)?;
        let item = ImageItem { view, at, width_mm, px, mime: mime.into(), data: base64_encode(bytes), name: name.into(), pinned: false };
        self.transact("Image", |tx| Ok(tx.add(Element::ImageItem(item))))
    }

    /// Width, position, name and Pinned; the picture itself stays.
    pub fn patch_image(&mut self, id: Id, patch: &serde_json::Value) -> Result<(), String> {
        let old = self.image(id).cloned().ok_or("not an image")?;
        let mut new: ImageItem = crate::doc::apply_patch(&old, patch)?;
        new.view = old.view;
        new.px = old.px;
        new.mime = old.mime.clone();
        new.data = old.data.clone();
        check_width(new.width_mm)?;
        if new == old {
            return Ok(());
        }
        self.transact("Change image", |tx| tx.update(id, Element::ImageItem(new)))
    }
}

fn check_width(w: f64) -> Result<(), String> {
    if w.is_finite() && (1.0..=2000.0).contains(&w) { Ok(()) } else { Err("An image is 1 to 2000 mm wide on paper.".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_read_for_their_kind_and_size() {
        // a 3 x 2 PNG header and a JPEG with a start of frame for 640 x 480
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        png.extend(3u32.to_be_bytes());
        png.extend(2u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        assert_eq!(image_kind(&png).unwrap(), ("image/png", [3, 2]));
        let jpg = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x01, 0xe0, 0x02, 0x80, 0x03, 0x00];
        assert_eq!(image_kind(&jpg).unwrap(), ("image/jpeg", [640, 480]));
        assert!(image_kind(b"GIF89a....").is_err());
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }
}
