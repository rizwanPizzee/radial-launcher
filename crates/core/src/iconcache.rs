//! Icon cache: one small file with the 96x96 icon of every configured app.
//!
//! The settings app (or `radial-launcher --refresh-icons`) extracts the icons with the
//! Windows shell and writes this file. The always-running launcher only *reads* it while
//! the wheel is open, so none of the shell/COM/imaging machinery is ever loaded into the
//! resident process and no icon bitmaps stay in RAM while idle.

use crate::config::{cache_dir, expand_env, Config, ItemConfig};
use crate::ICON_PX;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

const MAGIC: &[u8; 4] = b"RLIC";
const VERSION: u32 = 2;
const PIXEL_BYTES: usize = (ICON_PX * ICON_PX * 4) as usize;

/// Scale any icon bitmap to exactly `ICON_PX` x `ICON_PX`, keeping its aspect ratio.
pub fn fit_icon(src: &Pixmap) -> Pixmap {
    if src.width() == ICON_PX && src.height() == ICON_PX {
        return src.clone();
    }
    let mut out = Pixmap::new(ICON_PX, ICON_PX).expect("ICON_PX is non-zero");
    let k = (ICON_PX as f32 / src.width() as f32).min(ICON_PX as f32 / src.height() as f32);
    let tx = (ICON_PX as f32 - src.width() as f32 * k) / 2.0;
    let ty = (ICON_PX as f32 - src.height() as f32 * k) / 2.0;
    out.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &PixmapPaint { quality: FilterQuality::Bicubic, ..PixmapPaint::default() },
        Transform::from_row(k, 0.0, 0.0, k, tx, ty),
        None,
    );
    out
}

pub fn cache_path() -> PathBuf {
    cache_dir().join("icons.bin")
}

/// Stable key of an item's target (case-insensitive, environment variables expanded).
pub fn key_for(path: &str) -> u64 {
    let s = expand_env(path.trim()).to_lowercase();
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Cache key of an item's icon, or `None` if it has nothing to extract: built-in actions use a
/// drawn icon (unless the user picked a picture for them) and empty entries have no target.
/// A custom picture gets its own key, so changing it never reuses the app's icon.
pub fn item_key(it: &ItemConfig) -> Option<u64> {
    let source = it.icon_source();
    if source.is_empty() || (it.builtin().is_some() && !it.has_custom_icon()) {
        None
    } else {
        Some(key_for(source))
    }
}

/// Keys the config needs icons for.
pub fn needed_keys(cfg: &Config) -> Vec<u64> {
    cfg.items.iter().filter_map(item_key).collect()
}

pub fn write(path: &Path, entries: &[(u64, &Pixmap)]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = io::BufWriter::new(std::fs::File::create(&tmp)?);
        f.write_all(MAGIC)?;
        f.write_all(&VERSION.to_le_bytes())?;
        f.write_all(&ICON_PX.to_le_bytes())?;
        f.write_all(&(entries.len() as u32).to_le_bytes())?;
        for (key, pm) in entries {
            if pm.width() != ICON_PX || pm.height() != ICON_PX {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "icon has the wrong size"));
            }
            f.write_all(&key.to_le_bytes())?;
            f.write_all(pm.data())?;
        }
        f.flush()?;
    }
    std::fs::rename(&tmp, path)
}

fn read_header(r: &mut impl Read) -> Option<u32> {
    let mut h = [0u8; 16];
    r.read_exact(&mut h).ok()?;
    let u = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    (&h[0..4] == MAGIC && u(4) == VERSION && u(8) == ICON_PX).then(|| u(12))
}

/// Load every icon in the file.
pub fn read(path: &Path) -> Option<HashMap<u64, Pixmap>> {
    read_filtered(path, |_| true)
}

/// Load only the icons whose key `wanted` accepts; the others are skipped without being read,
/// so a wheel with hundreds of apps only loads the icons of the page that is on screen.
pub fn read_filtered(path: &Path, wanted: impl Fn(u64) -> bool) -> Option<HashMap<u64, Pixmap>> {
    use std::io::{Seek, SeekFrom};
    let mut r = io::BufReader::new(std::fs::File::open(path).ok()?);
    let count = read_header(&mut r)?;
    let mut map = HashMap::new();
    for _ in 0..count {
        let mut key = [0u8; 8];
        r.read_exact(&mut key).ok()?;
        if !wanted(u64::from_le_bytes(key)) {
            r.seek(SeekFrom::Current(PIXEL_BYTES as i64)).ok()?;
            continue;
        }
        let mut data = vec![0u8; PIXEL_BYTES];
        r.read_exact(&mut data).ok()?;
        let mut pm = Pixmap::new(ICON_PX, ICON_PX)?;
        pm.data_mut().copy_from_slice(&data);
        map.insert(u64::from_le_bytes(key), pm);
    }
    Some(map)
}

/// Keys present in the file, without loading the pixels.
pub fn read_keys(path: &Path) -> Option<Vec<u64>> {
    use std::io::{Seek, SeekFrom};
    let mut r = io::BufReader::new(std::fs::File::open(path).ok()?);
    let count = read_header(&mut r)?;
    let mut keys = Vec::new();
    for _ in 0..count {
        let mut key = [0u8; 8];
        r.read_exact(&mut key).ok()?;
        keys.push(u64::from_le_bytes(key));
        r.seek(SeekFrom::Current(PIXEL_BYTES as i64)).ok()?;
    }
    Some(keys)
}

/// Fingerprint of the set of icons the config needs. The launcher uses it to start the icon
/// helper at most once per distinct set, so an icon that can never be extracted (deleted app,
/// protected Store app...) cannot make it retry forever. Never 0.
pub fn signature(cfg: &Config) -> u64 {
    let mut keys = needed_keys(cfg);
    keys.sort_unstable();
    keys.dedup();
    keys.iter().fold(0x9e37_79b9_7f4a_7c15u64, |h, k| (h ^ k).wrapping_mul(0x0100_0000_01b3).rotate_left(13)) | 1
}

/// True if the cache lacks an icon the config needs (or does not exist / is unreadable).
pub fn is_stale(cfg: &Config) -> bool {
    let needed = needed_keys(cfg);
    if needed.is_empty() {
        return false;
    }
    match read_keys(&cache_path()) {
        Some(have) => needed.iter().any(|k| !have.contains(k)),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(v: u8) -> Pixmap {
        let mut pm = Pixmap::new(ICON_PX, ICON_PX).unwrap();
        pm.data_mut().fill(v);
        pm
    }

    #[test]
    fn write_then_read_round_trip() {
        let path = std::env::temp_dir().join(format!("rl-icons-{}.bin", std::process::id()));
        let (a, b) = (solid(10), solid(200));
        write(&path, &[(1, &a), (2, &b)]).unwrap();
        let map = read(&path).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map[&1].data(), a.data());
        assert_eq!(map[&2].data(), b.data());
        assert_eq!(read_keys(&path).unwrap(), vec![1, 2]);
        // truncated / foreign files are rejected, not mis-read
        std::fs::write(&path, b"RLIC").unwrap();
        assert!(read(&path).is_none() && read_keys(&path).is_none());
        std::fs::write(&path, b"not an icon cache at all").unwrap();
        assert!(read(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fit_icon_normalises_size() {
        for (w, h) in [(256, 256), (32, 32), (128, 64), (10, 40)] {
            let out = fit_icon(&Pixmap::new(w, h).unwrap());
            assert_eq!((out.width(), out.height()), (ICON_PX, ICON_PX));
        }
    }

    #[test]
    fn wrong_icon_size_is_refused() {
        let path = std::env::temp_dir().join(format!("rl-icons-bad-{}.bin", std::process::id()));
        let small = Pixmap::new(8, 8).unwrap();
        assert!(write(&path, &[(1, &small)]).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn keys_ignore_case_and_whitespace_and_expand_variables() {
        assert_eq!(key_for(" C:\\A\\B.exe "), key_for("c:\\a\\b.EXE"));
        assert_ne!(key_for("a"), key_for("b"));
        std::env::set_var("RL_KEY_TEST", "C:\\X");
        assert_eq!(key_for("%RL_KEY_TEST%\\a.exe"), key_for("c:\\x\\a.exe"));
    }

    #[test]
    fn filtered_read_loads_only_what_is_asked_for() {
        let path = std::env::temp_dir().join(format!("rl-icons-filter-{}.bin", std::process::id()));
        let (a, b, c) = (solid(1), solid(2), solid(3));
        write(&path, &[(10, &a), (20, &b), (30, &c)]).unwrap();
        let map = read_filtered(&path, |k| k == 20 || k == 30).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map[&20].data(), b.data());
        assert_eq!(map[&30].data(), c.data());
        assert!(read_filtered(&path, |_| false).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn custom_pictures_get_their_own_key() {
        let mut it = crate::config::ItemConfig::new("A", "C:\\a.exe");
        let plain = item_key(&it).unwrap();
        it.icon = "D:\\pic.png".into();
        assert_ne!(item_key(&it).unwrap(), plain);
        assert_eq!(item_key(&it), Some(key_for("D:\\pic.png")));
        // the built-in settings slice has a drawn icon unless the user picks a picture
        let mut s = crate::config::ItemConfig::new("S", "@settings");
        assert_eq!(item_key(&s), None);
        s.icon = "D:\\gear.png".into();
        assert!(item_key(&s).is_some());
    }

    #[test]
    fn signature_changes_with_the_icon_set_only() {
        let mut cfg = Config::default();
        let a = signature(&cfg);
        assert_ne!(a, 0);
        cfg.theme.hover = crate::color::Color::rgb(1, 2, 3); // unrelated setting
        assert_eq!(signature(&cfg), a);
        cfg.items.reverse(); // order does not matter
        assert_eq!(signature(&cfg), a);
        cfg.items.push(crate::config::ItemConfig::new("New", "C:\\new.exe"));
        assert_ne!(signature(&cfg), a);
    }

    #[test]
    fn builtin_and_empty_items_need_no_icons() {
        let mut cfg = Config::default();
        cfg.items = vec![crate::config::ItemConfig::new("S", "@settings"), crate::config::ItemConfig::new("E", "")];
        assert!(needed_keys(&cfg).is_empty());
    }
}
