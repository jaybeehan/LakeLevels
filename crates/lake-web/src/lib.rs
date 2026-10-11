//! The page's calculations, compiled to WebAssembly. The page's JavaScript only
//! fetches data, draws charts and fills in the HTML; everything it shows is
//! worked out here by `lake-core`. Results that have several parts come back as
//! JSON text for the page to `JSON.parse`.

use lake_core::calc::{compare_json, daily_csv, tiles_json};
use lake_core::dates::parse_iso;
use lake_core::{month_table, projection, Loader, MonthTable, Projection, Series};
use wasm_bindgen::prelude::*;

/// One station's daily levels with its 1st-of-month table and July 1 projection.
#[wasm_bindgen]
pub struct Lake {
    s: Series,
    table: MonthTable,
    proj: Option<Projection>,
}

impl Lake {
    fn from_series(s: Series) -> Lake {
        let table = month_table(&s, &(1..=12).collect::<Vec<u32>>());
        let proj = projection(&table);
        Lake { s, table, proj }
    }
}

#[wasm_bindgen]
impl Lake {
    /// Muriel Lake's prepared file: start date, day offsets from it, levels and a
    /// string of "0"/"1" provisional flags.
    pub fn from_compact(t0: &str, d: Vec<f64>, v: Vec<f64>, p: &str) -> Result<Lake, JsError> {
        let t0 = parse_iso(t0).ok_or_else(|| JsError::new("bad start date in lake.json"))?;
        if d.len() != v.len() || d.is_empty() {
            return Err(JsError::new("lake.json has no readings"));
        }
        let flags = p.as_bytes();
        let s = Series {
            t: d.iter().map(|&x| t0 + x as i64).collect(),
            v,
            p: (0..d.len()).map(|i| flags.get(i) == Some(&b'1')).collect(),
        };
        Ok(Lake::from_series(s))
    }

    pub fn len(&self) -> usize {
        self.s.len()
    }
    /// first and last day numbers (days since 1970-01-01)
    pub fn first(&self) -> f64 {
        self.s.first() as f64
    }
    pub fn last(&self) -> f64 {
        self.s.last() as f64
    }
    pub fn days(&self) -> Vec<f64> {
        self.s.t.iter().map(|&t| t as f64).collect()
    }
    pub fn levels(&self) -> Vec<f64> {
        self.s.v.clone()
    }
    pub fn provisional(&self) -> Vec<u8> {
        self.s.p.iter().map(|&p| p as u8).collect()
    }

    /// Level on day `t`: `[level, measured (1/0), provisional (1/0)]`, or empty outside the record.
    pub fn level_on(&self, t: f64) -> Vec<f64> {
        match self.s.level_on(t.round() as i64) {
            Some(l) => vec![l.v, l.measured as u8 as f64, l.prov as u8 as f64],
            None => vec![],
        }
    }

    /// `{rows: [{year, level[12], src[12], total[12], yoy[12]}], first[12]}`
    pub fn monthly_json(&self) -> String {
        self.table.to_json()
    }

    /// The July 1 projection, or `null` if there isn't enough history.
    pub fn projection_json(&self) -> String {
        self.proj.as_ref().map_or("null".into(), |p| p.to_json())
    }

    /// The summary tiles for month `m`, day `d`, looking back `lookback` months.
    pub fn tiles_json(&self, m: u32, d: u32, lookback: i32) -> String {
        tiles_json(&self.s, m, d, lookback as i64)
    }

    /// Levels on two days and the change between them.
    pub fn compare_json(&self, a: f64, b: f64) -> String {
        compare_json(&self.s, a.round() as i64, b.round() as i64)
    }

    /// Every daily level as CSV.
    pub fn csv(&self) -> String {
        daily_csv(&self.s)
    }
}

/// Builds a [`Lake`] for any station from Environment Canada's CSV pages.
#[wasm_bindgen]
pub struct StationLoader {
    loader: Loader,
    /// local standard time, in days behind UTC
    offset_days: f64,
    rt_from: Option<i64>,
}

#[wasm_bindgen]
impl StationLoader {
    /// `lon`: the station's longitude, used to estimate its standard-time zone.
    #[wasm_bindgen(constructor)]
    pub fn new(lon: f64) -> StationLoader {
        let hours = (-lon / 15.0 + 0.5).floor();
        StationLoader { loader: Loader::new(), offset_days: hours / 24.0, rt_from: None }
    }

    /// A page of approved daily means (CSV with DATE and LEVEL). Returns its row count.
    pub fn add_daily_mean_csv(&mut self, text: &str) -> usize {
        self.loader.add_daily_mean_csv(text)
    }

    /// A page of real-time readings (CSV with DATETIME and LEVEL). Returns its row count.
    pub fn add_realtime_csv(&mut self, text: &str) -> usize {
        self.loader.add_realtime_csv(text, self.offset_days)
    }

    pub fn is_empty(&self) -> bool {
        self.loader.is_empty()
    }

    /// Last day of approved data, if any.
    pub fn approved_until(&self) -> Option<f64> {
        self.loader.approved_until().map(|t| t as f64)
    }

    /// First day filled from real-time data (after `finish`), if any.
    pub fn rt_from(&self) -> Option<f64> {
        self.rt_from.map(|t| t as f64)
    }

    /// The merged series.
    pub fn finish(&mut self) -> Result<Lake, JsError> {
        if self.loader.is_empty() {
            return Err(JsError::new("No water level records"));
        }
        let (s, rt_from) = self.loader.finish();
        self.rt_from = rt_from;
        Ok(Lake::from_series(s))
    }
}
