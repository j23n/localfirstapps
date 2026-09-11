//! On-disk pack format (`LCG1`). Written by `scripts/pack_geo.py`.

use std::str;

pub(crate) const MAGIC: &[u8; 4] = b"LCG1";
pub(crate) const VERSION: u16 = 1;
pub(crate) const NO_ADMIN: u16 = 0xFFFF;

#[derive(Debug)]
pub(crate) struct PackError(&'static str);

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "geo pack: {}", self.0)
    }
}

pub(crate) struct City {
    pub lat: f64,
    pub lon: f64,
    pub locality: String,
    pub admin: Option<String>,
}

pub(crate) struct Country {
    pub name: String,
    pub code: [u8; 2],
    pub rings: Vec<Vec<(f64, f64)>>,
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

pub(crate) struct Pack {
    pub cities: Vec<City>,
    pub countries: Vec<Country>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn rest(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], PackError> {
        if self.rest() < n {
            return Err(PackError("truncated"));
        }
        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, PackError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, PackError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Result<i32, PackError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Pack, PackError> {
    let mut c = Cursor::new(bytes);
    if c.take(4)? != MAGIC {
        return Err(PackError("bad magic"));
    }
    if c.u16()? != VERSION {
        return Err(PackError("unsupported version"));
    }
    let _flags = c.u16()?;
    let n_strings = c.u32()? as usize;
    let n_cities = c.u32()? as usize;
    let n_countries = c.u32()? as usize;

    let mut strings = Vec::with_capacity(n_strings);
    for _ in 0..n_strings {
        let n = c.u16()? as usize;
        let raw = c.take(n)?;
        let s = str::from_utf8(raw).map_err(|_| PackError("string is not utf-8"))?;
        strings.push(s.to_owned());
    }

    let intern = |idx: u16| -> Result<String, PackError> {
        strings
            .get(idx as usize)
            .cloned()
            .ok_or(PackError("string index out of range"))
    };

    let mut cities = Vec::with_capacity(n_cities);
    for _ in 0..n_cities {
        let lat = c.i32()? as f64 / 1e6;
        let lon = c.i32()? as f64 / 1e6;
        let name_i = c.u16()?;
        let admin_i = c.u16()?;
        let admin = if admin_i == NO_ADMIN {
            None
        } else {
            Some(intern(admin_i)?)
        };
        cities.push(City {
            lat,
            lon,
            locality: intern(name_i)?,
            admin,
        });
    }

    let mut countries = Vec::with_capacity(n_countries);
    for _ in 0..n_countries {
        let name = intern(c.u16()?)?;
        let code = {
            let b = c.take(2)?;
            [b[0], b[1]]
        };
        if !code[0].is_ascii_uppercase() || !code[1].is_ascii_uppercase() {
            return Err(PackError("country code is not A-Z"));
        }
        let n_rings = c.u16()? as usize;
        let mut rings = Vec::with_capacity(n_rings);
        let mut min_lon = f64::INFINITY;
        let mut min_lat = f64::INFINITY;
        let mut max_lon = f64::NEG_INFINITY;
        let mut max_lat = f64::NEG_INFINITY;
        for _ in 0..n_rings {
            let n_pts = c.u16()? as usize;
            let mut ring = Vec::with_capacity(n_pts);
            for _ in 0..n_pts {
                let lon = c.i32()? as f64 / 1e6;
                let lat = c.i32()? as f64 / 1e6;
                min_lon = min_lon.min(lon);
                min_lat = min_lat.min(lat);
                max_lon = max_lon.max(lon);
                max_lat = max_lat.max(lat);
                ring.push((lon, lat));
            }
            if ring.len() < 3 {
                return Err(PackError("ring has fewer than 3 points"));
            }
            rings.push(ring);
        }
        countries.push(Country {
            name,
            code,
            rings,
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        });
    }

    Ok(Pack { cities, countries })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_pack_decodes() {
        let pack = decode(include_bytes!("../data/places.bin")).expect("committed pack");
        assert!(!pack.cities.is_empty());
        assert!(!pack.countries.is_empty());
        assert!(pack.cities.iter().any(|c| c.locality == "Paris"));
        let codes: Vec<String> = pack
            .countries
            .iter()
            .map(|c| std::str::from_utf8(&c.code).unwrap().to_string())
            .collect();
        assert!(codes.contains(&"FR".to_string()), "{codes:?}");
        assert!(codes.contains(&"US".to_string()), "{codes:?}");
        assert!(codes.contains(&"CA".to_string()), "{codes:?}");
    }
}
