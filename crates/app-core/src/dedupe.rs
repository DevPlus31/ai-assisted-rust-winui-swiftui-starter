//! Exact duplicate detection (content hash) and similar-photo grouping
//! (perceptual hash). Read-only: deciding what to delete is the user's call.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

/// Hamming distance (of 64 bits) at or below which two photos count as
/// "similar": survives re-encoding, resizing and small edits.
pub const SIMILARITY_THRESHOLD: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// Sorted paths; the first is a reasonable "keep" suggestion.
    pub paths: Vec<PathBuf>,
    /// Bytes per file (exact duplicates) or of the largest file (similar).
    pub size: u64,
    pub exact: bool,
}

impl DuplicateGroup {
    /// Bytes freed by keeping one file from the group.
    pub fn reclaimable_bytes(&self) -> u64 {
        if self.exact {
            self.size * (self.paths.len() as u64 - 1)
        } else {
            0
        }
    }
}

/// Byte-identical files. Groups by size first so only candidates are hashed.
pub fn find_exact_duplicates(paths: &[PathBuf]) -> Vec<DuplicateGroup> {
    let mut by_size: HashMap<u64, Vec<&PathBuf>> = HashMap::new();
    for path in paths {
        if let Ok(meta) = fs::metadata(path)
            && meta.is_file()
            && meta.len() > 0
        {
            by_size.entry(meta.len()).or_default().push(path);
        }
    }
    let candidates: Vec<(u64, &PathBuf)> = by_size
        .into_iter()
        .filter(|(_, group)| group.len() > 1)
        .flat_map(|(size, group)| group.into_iter().map(move |p| (size, p)))
        .collect();
    let hashed: Vec<((u64, blake3::Hash), &PathBuf)> = candidates
        .par_iter()
        .filter_map(|(size, path)| hash_file(path).map(|h| ((*size, h), *path)))
        .collect();

    let mut groups: HashMap<(u64, blake3::Hash), Vec<PathBuf>> = HashMap::new();
    for (key, path) in hashed {
        groups.entry(key).or_default().push(path.clone());
    }
    let mut result: Vec<DuplicateGroup> = groups
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|((size, _), mut paths)| {
            paths.sort();
            DuplicateGroup {
                paths,
                size,
                exact: true,
            }
        })
        .collect();
    result.sort_by(|a, b| {
        b.reclaimable_bytes()
            .cmp(&a.reclaimable_bytes())
            .then(a.paths.cmp(&b.paths))
    });
    result
}

/// `true` when both files have identical bytes (size check, then BLAKE3).
///
/// # Errors
/// Any I/O error reading either file.
pub fn same_content(a: &Path, b: &Path) -> std::io::Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let unreadable = || std::io::Error::from(std::io::ErrorKind::InvalidData);
    Ok(hash_file(a).ok_or_else(unreadable)? == hash_file(b).ok_or_else(unreadable)?)
}

fn hash_file(path: &Path) -> Option<blake3::Hash> {
    let mut hasher = blake3::Hasher::new();
    let mut file = fs::File::open(path).ok()?;
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(hasher.finalize());
        }
        hasher.update(&buffer[..read]);
    }
}

/// Visually similar images (different files that look alike: bursts,
/// re-saves, resized copies). Exact duplicates are excluded — report those
/// with [`find_exact_duplicates`].
pub fn find_similar_images(paths: &[PathBuf]) -> Vec<DuplicateGroup> {
    let hashes: Vec<(PathBuf, u64, u64)> = paths
        .par_iter()
        .filter_map(|path| {
            let size = fs::metadata(path).ok()?.len();
            Some((path.clone(), difference_hash(path)?, size))
        })
        .collect();

    let mut sets = DisjointSets::new(hashes.len());
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            if (hashes[i].1 ^ hashes[j].1).count_ones() <= SIMILARITY_THRESHOLD {
                sets.union(i, j);
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..hashes.len() {
        groups.entry(sets.find(i)).or_default().push(i);
    }
    let mut result: Vec<DuplicateGroup> = groups
        .into_values()
        .filter(|members| members.len() > 1)
        .map(|members| {
            let mut paths: Vec<PathBuf> = members.iter().map(|&i| hashes[i].0.clone()).collect();
            paths.sort();
            let size = members.iter().map(|&i| hashes[i].2).max().unwrap_or(0);
            DuplicateGroup {
                paths,
                size,
                exact: false,
            }
        })
        .collect();
    result.sort_by(|a, b| a.paths.cmp(&b.paths));
    result
}

/// 64-bit dHash: grayscale 9×8 thumbnail, one bit per horizontal gradient.
pub fn difference_hash(path: &Path) -> Option<u64> {
    crate::error::contained(|| difference_hash_unguarded(path)).flatten()
}

fn difference_hash_unguarded(path: &Path) -> Option<u64> {
    let thumbnail = crate::imaging::open_upright(path)?
        .resize_exact(9, 8, image::imageops::FilterType::Triangle)
        .to_luma8();
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let left = thumbnail.get_pixel(x, y)[0];
            let right = thumbnail.get_pixel(x + 1, y)[0];
            hash = (hash << 1) | u64::from(left > right);
        }
    }
    Some(hash)
}

struct DisjointSets {
    parent: Vec<usize>,
}

impl DisjointSets {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};

    fn gradient(width: u32, height: u32, flip: bool) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| {
            let v = u8::try_from((x * 255 / width + y * 40 / height).min(255)).unwrap();
            let v = if flip { 255 - v } else { v };
            Rgb([v, v / 2, 255 - v])
        })
    }

    #[test]
    fn finds_byte_identical_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        let c = dir.path().join("c.bin"); // same size, different bytes
        fs::write(&a, b"same-content").unwrap();
        fs::write(&b, b"same-content").unwrap();
        fs::write(&c, b"diff-content").unwrap();

        let groups = find_exact_duplicates(&[a.clone(), b.clone(), c]);

        assert_eq!(
            groups,
            vec![DuplicateGroup {
                paths: vec![a, b],
                size: 12,
                exact: true
            }]
        );
        assert_eq!(groups[0].reclaimable_bytes(), 12);
    }

    #[test]
    fn groups_resized_and_reencoded_photos() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.png");
        let smaller = dir.path().join("smaller.jpg");
        let different = dir.path().join("different.png");
        gradient(400, 300, false).save(&original).unwrap();
        image::DynamicImage::ImageRgb8(gradient(400, 300, false))
            .resize(200, 150, image::imageops::FilterType::Triangle)
            .save_with_format(&smaller, ImageFormat::Jpeg)
            .unwrap();
        gradient(400, 300, true).save(&different).unwrap();

        let groups = find_similar_images(&[original.clone(), smaller.clone(), different]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths, vec![original, smaller]);
        assert!(!groups[0].exact);
    }

    #[test]
    fn non_images_are_ignored_for_similarity() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        fs::write(&a, "x").unwrap();
        assert!(find_similar_images(&[a.clone(), a]).is_empty());
    }
}
