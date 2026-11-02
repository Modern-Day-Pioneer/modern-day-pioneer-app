//! The nearest town to a point, from a list of towns built into the app
//! (`data/towns.zip`, from GeoNames, CC BY 4.0). Nothing is looked up
//! online: Google Takeout's locations are named here, on the computer.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::OnceLock;

static DATA: &[u8] = include_bytes!("../data/towns.zip");

/// One town.
#[derive(Debug, Clone, PartialEq)]
pub struct Town {
    pub name: String,
    pub region: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Town {
    /// "Boise, Idaho, United States".
    pub fn label(&self) -> String {
        [self.name.as_str(), self.region.as_str(), self.country.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .fold(Vec::<&str>::new(), |mut v, s| {
                if v.last() != Some(&s) {
                    v.push(s);
                }
                v
            })
            .join(", ")
    }
}

struct Index {
    towns: Vec<Town>,
    /// Towns by one-degree square.
    cells: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(lat: f64, lon: f64) -> (i32, i32) {
    (lat.floor() as i32, lon.floor() as i32)
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut text = String::new();
        if let Ok(mut z) = zip::ZipArchive::new(Cursor::new(DATA)) {
            if let Ok(mut f) = z.by_index(0) {
                let _ = f.read_to_string(&mut text);
            }
        }
        let mut towns = Vec::new();
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for line in text.lines() {
            let p: Vec<&str> = line.split('\t').collect();
            let (Some(lat), Some(lon)) = (p.get(3).and_then(|s| s.parse().ok()), p.get(4).and_then(|s| s.parse().ok())) else { continue };
            cells.entry(cell(lat, lon)).or_default().push(towns.len() as u32);
            towns.push(Town { name: p[0].into(), region: p[1].into(), country: p[2].into(), latitude: lat, longitude: lon });
        }
        Index { towns, cells }
    })
}

/// The town nearest to a point, within `max_km`, and how far it is.
pub fn nearest(lat: f64, lon: f64, max_km: f64) -> Option<(Town, f64)> {
    let ix = index();
    let (cy, cx) = cell(lat, lon);
    // A degree of latitude is about 111 km; look far enough east and west
    // for the latitude (less near the poles), across the date line too.
    let ry = (max_km / 111.0).ceil() as i32;
    let rx = ((max_km / (111.0 * lat.to_radians().cos().abs().max(0.05))).ceil() as i32).min(180);
    let mut best: Option<(u32, f64)> = None;
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            let x = (cx + dx + 180).rem_euclid(360) - 180;
            for &i in ix.cells.get(&(cy + dy, x)).into_iter().flatten() {
                let t = &ix.towns[i as usize];
                let d = crate::takeout::km((lat, lon), (t.latitude, t.longitude));
                if d <= max_km && best.is_none_or(|(_, b)| d < b) {
                    best = Some((i, d));
                }
            }
        }
    }
    best.map(|(i, d)| (ix.towns[i as usize].clone(), d))
}

#[cfg(test)]
mod tests {
    #[test]
    fn nearest_towns() {
        let (t, d) = super::nearest(43.6150, -116.2023, 50.0).unwrap();
        assert_eq!(t.label(), "Boise, Idaho, United States");
        assert!(d < 2.0);
        assert!(super::nearest(0.0, -150.0, 50.0).is_none(), "the middle of the Pacific");
        let (t, _) = super::nearest(51.5072, -0.1276, 20.0).unwrap();
        assert!(t.label().ends_with("United Kingdom"), "{}", t.label());
    }
}
