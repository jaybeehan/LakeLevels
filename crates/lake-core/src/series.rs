//! A station's daily water levels, and how they are put together from the
//! approved (HYDAT) record and the provisional real-time record.

use crate::csv::Csv;
use crate::dates::{parse_datetime, parse_iso};
use std::collections::BTreeMap;

/// Daily levels: `t` day numbers (sorted), `v` levels in metres, `p` provisional flags.
#[derive(Clone, Debug, Default)]
pub struct Series {
    pub t: Vec<i64>,
    pub v: Vec<f64>,
    pub p: Vec<bool>,
}

/// The level on one day.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub v: f64,
    /// a reading that day (rather than interpolated)
    pub measured: bool,
    /// the reading is provisional real-time data
    pub prov: bool,
}

impl Level {
    pub fn source(&self) -> Source {
        match (self.measured, self.prov) {
            (false, _) => Source::Interpolated,
            (true, false) => Source::Measured,
            (true, true) => Source::MeasuredProvisional,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Measured,
    MeasuredProvisional,
    Interpolated,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Measured => "measured",
            Source::MeasuredProvisional => "measured (provisional)",
            Source::Interpolated => "interpolated",
        }
    }
}

impl Series {
    pub fn len(&self) -> usize {
        self.t.len()
    }
    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }
    pub fn first(&self) -> i64 {
        self.t[0]
    }
    pub fn last(&self) -> i64 {
        self.t[self.t.len() - 1]
    }
    pub fn latest(&self) -> f64 {
        self.v[self.v.len() - 1]
    }

    /// The level on day `t`: the reading if there is one, otherwise a straight
    /// line between the readings either side. `None` outside the record.
    pub fn level_on(&self, t: i64) -> Option<Level> {
        if self.is_empty() || t < self.first() || t > self.last() {
            return None;
        }
        match self.t.binary_search(&t) {
            Ok(i) => Some(Level { v: self.v[i], measured: true, prov: self.p[i] }),
            Err(i) => {
                let (t0, t1) = (self.t[i - 1], self.t[i]);
                let (v0, v1) = (self.v[i - 1], self.v[i]);
                let f = (t - t0) as f64 / (t1 - t0) as f64;
                Some(Level { v: v0 + (v1 - v0) * f, measured: false, prov: false })
            }
        }
    }
}

/// Collects approved daily means and real-time readings, then merges them.
/// Approved data always wins; real-time data only fills days it doesn't cover.
#[derive(Default)]
pub struct Loader {
    pub approved: BTreeMap<i64, f64>,
    realtime: BTreeMap<i64, (f64, u32)>,
}

impl Loader {
    pub fn new() -> Loader {
        Loader::default()
    }

    /// A page of daily means as CSV with DATE and LEVEL columns
    /// (the GeoMet `hydrometric-daily-mean` collection). Returns the number of rows on the page.
    pub fn add_daily_mean_csv(&mut self, text: &str) -> usize {
        let csv = Csv::parse(text);
        let (dc, lc) = (csv.col("DATE"), csv.col("LEVEL"));
        for row in &csv.rows {
            let (d, l) = (Csv::get(row, dc), Csv::get(row, lc));
            if let (Some(t), Ok(v)) = (parse_iso(d), l.parse::<f64>()) {
                if v.is_finite() {
                    self.approved.insert(t, v);
                }
            }
        }
        csv.rows.len()
    }

    /// One real-time reading taken at `secs` (UTC). It is counted on the local
    /// standard-time day, `offset_days` behind UTC (7/24 for Alberta).
    /// Readings on or after day `before` are skipped.
    pub fn add_reading(&mut self, secs: f64, level: f64, offset_days: f64, before: Option<i64>) -> bool {
        if !level.is_finite() {
            return false;
        }
        let day = (secs / 86_400.0 - offset_days).floor() as i64;
        if before.is_some_and(|b| day >= b) {
            return false;
        }
        let e = self.realtime.entry(day).or_insert((0.0, 0));
        e.0 += level;
        e.1 += 1;
        true
    }

    /// Real-time CSV from the GeoMet `hydrometric-realtime` collection
    /// (DATETIME and LEVEL columns). Returns the number of rows.
    pub fn add_realtime_csv(&mut self, text: &str, offset_days: f64) -> usize {
        let csv = Csv::parse(text);
        let (tc, lc) = (csv.col("DATETIME"), csv.col("LEVEL"));
        for row in &csv.rows {
            if let (Some(s), Ok(v)) = (parse_datetime(Csv::get(row, tc)), Csv::get(row, lc).parse::<f64>()) {
                self.add_reading(s, v, offset_days, None);
            }
        }
        csv.rows.len()
    }

    /// Real-time CSV from WaterOffice (`real_time_data/csv/inline`: the time
    /// is the 2nd column, the value the 4th). Returns the number of readings used.
    pub fn add_wateroffice_csv(&mut self, text: &str, offset_days: f64, before: i64) -> usize {
        let csv = Csv::parse(text);
        let mut n = 0;
        for row in &csv.rows {
            if row.len() < 4 {
                continue;
            }
            if let (Some(s), Ok(v)) = (parse_datetime(row[1].trim()), row[3].trim().parse::<f64>()) {
                if self.add_reading(s, v, offset_days, Some(before)) {
                    n += 1;
                }
            }
        }
        n
    }

    pub fn approved_until(&self) -> Option<i64> {
        self.approved.keys().next_back().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.approved.is_empty() && self.realtime.is_empty()
    }

    /// The merged series, and the first day filled from real-time data (if any).
    pub fn finish(&self) -> (Series, Option<i64>) {
        let mut all: BTreeMap<i64, (f64, bool)> = self.approved.iter().map(|(&t, &v)| (t, (v, false))).collect();
        let mut rt_from = None;
        for (&t, &(sum, n)) in &self.realtime {
            if !all.contains_key(&t) {
                all.insert(t, (sum / n as f64, true));
                rt_from = Some(rt_from.map_or(t, |r: i64| r.min(t)));
            }
        }
        let mut s = Series::default();
        for (t, (v, p)) in all {
            s.t.push(t);
            s.v.push(v);
            s.p.push(p);
        }
        (s, rt_from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dates::day;

    #[test]
    fn merge_and_interpolate() {
        let mut l = Loader::new();
        assert_eq!(l.add_daily_mean_csv("x,y,DATE,LEVEL\n1,2,2020-01-01,10\n1,2,2020-01-05,14\n1,2,2020-01-06,\n"), 3);
        // Alberta: 7 h behind UTC. 06:00Z on Jan 6 is still Jan 5 locally (approved wins);
        // 08:00Z and 10:00Z on Jan 6 average to 20 on Jan 6.
        let off = 7.0 / 24.0;
        let at = |d: i64, h: f64| d as f64 * 86_400.0 + h * 3600.0;
        l.add_reading(at(day(2020, 1, 6), 6.0), 99.0, off, None);
        l.add_reading(at(day(2020, 1, 6), 8.0), 19.0, off, None);
        l.add_reading(at(day(2020, 1, 6), 10.0), 21.0, off, None);
        let (s, rt_from) = l.finish();
        assert_eq!(s.t, vec![day(2020, 1, 1), day(2020, 1, 5), day(2020, 1, 6)]);
        assert_eq!(s.v, vec![10.0, 14.0, 20.0]);
        assert_eq!(s.p, vec![false, false, true]);
        assert_eq!(rt_from, Some(day(2020, 1, 6)));
        assert_eq!(l.approved_until(), Some(day(2020, 1, 5)));
        let m = s.level_on(day(2020, 1, 2)).unwrap();
        assert_eq!((m.v, m.measured), (11.0, false));
        assert_eq!(s.level_on(day(2020, 1, 6)).unwrap().source(), Source::MeasuredProvisional);
        assert!(s.level_on(day(2019, 12, 31)).is_none());
    }

    #[test]
    fn wateroffice() {
        let mut l = Loader::new();
        let text = "ID,Date,Parameter/Paramètre,Value/Valeur\n06AC007,2020-01-31T08:00:00Z,46,5\n06AC007,2020-02-01T07:00:00Z,46,6\n";
        assert_eq!(l.add_wateroffice_csv(text, 7.0 / 24.0, day(2020, 2, 1)), 1);
    }
}
