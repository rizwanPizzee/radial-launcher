//! RGBA colour that is stored in JSON as `"#RRGGBB"` or `"#RRGGBBAA"`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color([r, g, b, 255])
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color([r, g, b, a])
    }
    pub fn rgb_arr(self) -> [u8; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }
    pub fn a(self) -> u8 {
        self.0[3]
    }
    pub fn with_alpha(self, a: u8) -> Self {
        Color([self.0[0], self.0[1], self.0[2], a])
    }
    /// Multiply the alpha by `f` (0.0..=1.0).
    pub fn faded(self, f: f32) -> Self {
        self.with_alpha((self.0[3] as f32 * f.clamp(0.0, 1.0)).round() as u8)
    }
    /// Move each colour channel towards white (`t > 0`) or black (`t < 0`).
    pub fn shade(self, t: f32) -> Self {
        let f = |c: u8| -> u8 {
            let c = c as f32;
            let v = if t >= 0.0 { c + (255.0 - c) * t } else { c * (1.0 + t) };
            v.round().clamp(0.0, 255.0) as u8
        };
        Color([f(self.0[0]), f(self.0[1]), f(self.0[2]), self.0[3]])
    }
    pub fn mix(self, other: Color, t: f32) -> Self {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Color([m(self.0[0], other.0[0]), m(self.0[1], other.0[1]), m(self.0[2], other.0[2]), m(self.0[3], other.0[3])])
    }
    /// Perceived brightness 0..=255 (used to pick readable text colours).
    pub fn luma(self) -> f32 {
        0.299 * self.0[0] as f32 + 0.587 * self.0[1] as f32 + 0.114 * self.0[2] as f32
    }

    pub fn to_hex(self) -> String {
        if self.0[3] == 255 {
            format!("#{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2], self.0[3])
        }
    }

    pub fn from_hex(s: &str) -> Option<Color> {
        let h = s.trim().trim_start_matches('#');
        if !h.is_ascii() || (h.len() != 6 && h.len() != 8) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Color([byte(0)?, byte(2)?, byte(4)?, if h.len() == 8 { byte(6)? } else { 255 }]))
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour \"{s}\" (use #RRGGBB or #RRGGBBAA)")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(Color::from_hex("#FF8000"), Some(Color([255, 128, 0, 255])));
        assert_eq!(Color::from_hex("ff800080"), Some(Color([255, 128, 0, 128])));
        assert_eq!(Color::from_hex("#12345"), None);
        assert_eq!(Color::from_hex("#GG0000"), None);
        assert_eq!(Color::from_hex("#é12345"), None);
        let c = Color([1, 2, 3, 4]);
        assert_eq!(Color::from_hex(&c.to_hex()), Some(c));
        assert_eq!(Color::rgb(9, 8, 7).to_hex(), "#090807");
    }

    #[test]
    fn shading_and_mixing() {
        assert_eq!(Color::rgb(100, 100, 100).shade(1.0), Color::rgb(255, 255, 255));
        assert_eq!(Color::rgb(100, 100, 100).shade(-1.0), Color::rgb(0, 0, 0));
        assert_eq!(Color::rgb(0, 0, 0).mix(Color::rgb(200, 100, 50), 0.5), Color::rgb(100, 50, 25));
        assert!(Color::rgb(250, 250, 250).luma() > Color::rgb(10, 10, 10).luma());
    }
}
