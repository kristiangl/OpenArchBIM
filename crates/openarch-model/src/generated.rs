//! What the material generator made for a project's parametric materials, kept with the project so a material looks
//! the same wherever and whenever the project is opened, even after the generator itself is improved. For each
//! material's values and the generator version that made them: the tile size, relief depth, colour, roughness,
//! metalness and pattern lines, and the 3D maps (PNG, made when first drawn and at the latest when the project is
//! saved). A material names the version it was made with (`Appearance::generator_version`); changing its values, or
//! Update Generated Materials, makes it again with the current generator. Not undone: what is kept here is only ever
//! added to during a session, so undo always goes back to values and a version whose output is still here. A save
//! keeps what the materials use.

use serde::{Deserialize, Serialize};

use crate::element::{Element, Id};
use crate::matgen::{self, GeneratorParams};
use crate::styles::{AppearanceKind, HatchLine, Material};
use crate::view::Color;
use crate::Document;

/// The generator's output for one material's values, as one version of the generator made it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedEntry {
    pub version: u32,
    pub uv_size_mm: [f64; 2],
    pub depth_mm: f64,
    pub color: Color,
    pub roughness: f64,
    pub metalness: f64,
    /// The pattern's lines before the material's rotation and offset.
    pub hatch: Vec<HatchLine>,
    #[serde(skip)]
    pub maps: Option<GeneratedMaps>,
}

impl GeneratedEntry {
    /// The current generator's output for `g` (its maps are made separately, when wanted).
    pub fn make(g: &GeneratorParams) -> Result<GeneratedEntry, String> {
        let (w, h) = matgen::domain(g)?;
        let metal = g.archetype == matgen::Archetype::Metal && !matches!(g.finish, matgen::Finish::PowderCoated | matgen::Finish::Patina);
        Ok(GeneratedEntry {
            version: matgen::GENERATOR_VERSION,
            uv_size_mm: [w, h],
            depth_mm: matgen::relief_mm(g),
            color: g.base_color(),
            roughness: g.finish.roughness(),
            metalness: if metal { 1.0 } else { 0.0 },
            hatch: matgen::hatch(g)?,
            maps: None,
        })
    }
}

/// The 3D view's maps of a generated material: albedo, surface (normal and height packed) and roughness, as PNG.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedMaps {
    pub res: u32,
    pub px: [u32; 2],
    pub albedo: Vec<u8>,
    pub surface: Vec<u8>,
    pub roughness: Vec<u8>,
}

const MAGIC: &[u8; 8] = b"OAMAPS1\n";

impl GeneratedMaps {
    /// Made by the current generator at `res` pixels along the longer side.
    pub fn make(g: &GeneratorParams, res: u32) -> Result<GeneratedMaps, String> {
        let gen = matgen::generate(g, res, false)?;
        Ok(GeneratedMaps { res, px: gen.px, albedo: gen.albedo_png()?, surface: gen.surface_png()?, roughness: gen.roughness_png()? })
    }

    /// As one block of bytes (the project file and the map cache keep them so): a header, the size, then each PNG
    /// after its length.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(32 + self.albedo.len() + self.surface.len() + self.roughness.len());
        o.extend_from_slice(MAGIC);
        for v in [self.res, self.px[0], self.px[1]] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        for png in [&self.albedo, &self.surface, &self.roughness] {
            o.extend_from_slice(&(png.len() as u64).to_le_bytes());
            o.extend_from_slice(png);
        }
        o
    }

    pub fn from_bytes(b: &[u8]) -> Option<GeneratedMaps> {
        if b.len() < 20 || &b[..8] != MAGIC {
            return None;
        }
        let u32_at = |i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
        let (res, w, h) = (u32_at(8)?, u32_at(12)?, u32_at(16)?);
        let mut at = 20;
        let mut take = || -> Option<Vec<u8>> {
            let n = u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?) as usize;
            let png = b.get(at + 8..at.checked_add(8)?.checked_add(n)?)?.to_vec();
            at += 8 + n;
            Some(png)
        };
        let (albedo, surface, roughness) = (take()?, take()?, take()?);
        Some(GeneratedMaps { res, px: [w, h], albedo, surface, roughness })
    }
}

/// The key a parametric material's output is kept under: its values and the version it was made with.
pub fn key_of(m: &Material) -> Option<String> {
    if m.appearance.kind != AppearanceKind::Parametric {
        return None;
    }
    Some(matgen::store_key(m.appearance.generator.as_ref()?, m.appearance.generator_version))
}

impl Document {
    /// The output kept under `key`, without its maps.
    pub fn generated_entry(&self, key: &str) -> Option<GeneratedEntry> {
        self.generated.borrow().get(key).map(|e| GeneratedEntry { maps: None, ..e.clone() })
    }

    /// The maps kept under `key`, if they have been made.
    pub fn generated_maps(&self, key: &str) -> Option<GeneratedMaps> {
        self.generated.borrow().get(key).and_then(|e| e.maps.clone())
    }

    pub fn put_generated_entry(&self, key: String, e: GeneratedEntry) {
        self.generated.borrow_mut().insert(key, e);
    }

    /// Keep maps with the output under `key` (false when there is none).
    pub fn set_generated_maps(&self, key: &str, maps: GeneratedMaps) -> bool {
        match self.generated.borrow_mut().get_mut(key) {
            Some(e) => {
                e.maps = Some(maps);
                true
            }
            None => false,
        }
    }

    /// The keys of the output the project's materials use, each once.
    pub fn generated_in_use(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.materials().into_iter().filter_map(|(_, m)| key_of(m)).collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// The parametric materials made by a generator older (or newer) than this build's.
    pub fn outdated_generated(&self) -> Vec<(Id, String)> {
        self.materials()
            .into_iter()
            .filter(|(_, m)| m.appearance.kind == AppearanceKind::Parametric && m.appearance.generator.is_some() && m.appearance.generator_version != matgen::GENERATOR_VERSION)
            .map(|(id, m)| (id, m.name.clone()))
            .collect()
    }

    /// What the project file keeps: the output the materials use, as (key, JSON, maps).
    pub(crate) fn generated_rows(&self) -> Vec<(String, String, Option<Vec<u8>>)> {
        let kept = self.generated.borrow();
        self.generated_in_use()
            .into_iter()
            .filter_map(|k| {
                let e = kept.get(&k)?;
                Some((k, serde_json::to_string(e).ok()?, e.maps.as_ref().map(GeneratedMaps::to_bytes)))
            })
            .collect()
    }

    pub(crate) fn load_generated(&mut self, key: String, json: &str, maps: Option<&[u8]>) {
        if let Ok(mut e) = serde_json::from_str::<GeneratedEntry>(json) {
            e.maps = maps.and_then(GeneratedMaps::from_bytes);
            self.generated.get_mut().insert(key, e);
        }
    }

    /// A project opened: a material made before outputs were kept (or whose output is missing) takes the current
    /// generator's, which is what made it then.
    pub(crate) fn ensure_generated(&mut self) {
        let Document { elements, generated, .. } = self;
        for e in elements.values_mut() {
            let Element::Material(m) = e else { continue };
            if m.appearance.kind != AppearanceKind::Parametric {
                continue;
            }
            let Some(g) = m.appearance.generator.clone() else { continue };
            let kept = generated.get_mut();
            if kept.contains_key(&matgen::store_key(&g, m.appearance.generator_version)) {
                continue;
            }
            m.appearance.generator_version = matgen::GENERATOR_VERSION;
            let key = matgen::store_key(&g, matgen::GENERATOR_VERSION);
            if !kept.contains_key(&key) {
                if let Ok(made) = GeneratedEntry::make(&g) {
                    kept.insert(key, made);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_round_trip_as_bytes_and_bad_bytes_are_refused() {
        let m = GeneratedMaps { res: 512, px: [512, 344], albedo: vec![1, 2, 3], surface: vec![], roughness: vec![9; 40] };
        let b = m.to_bytes();
        assert_eq!(GeneratedMaps::from_bytes(&b), Some(m));
        assert_eq!(GeneratedMaps::from_bytes(&b[..b.len() - 1]), None, "cut short");
        assert_eq!(GeneratedMaps::from_bytes(b"not maps at all"), None);
        let mut long = b.clone();
        long[20..28].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(GeneratedMaps::from_bytes(&long), None, "a length past the end");
    }
}
