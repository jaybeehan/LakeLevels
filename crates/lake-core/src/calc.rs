//! The calculations behind the page and the Excel file: the 1st-of-month table,
//! the July 1 projection, the summary tiles and the two-date comparison.

use crate::dates::{add_months, day, year_of, ymd};
use crate::json::{self, array, num, object, opt, string};
use crate::series::{Level, Series, Source};

// ---------------------------------------------------------------- statistics

/// Inclusive linear percentile (same as Google Sheets PERCENTILE), `p` in 0..=1.
pub fn percentile(values: &[f64], p: f64) -> f64 {
    let mut s = values.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let pos = (s.len() - 1) as f64 * p;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(s.len() - 1);
    s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
}

pub fn median(values: &[f64]) -> f64 {
    percentile(values, 0.5)
}

/// Least-squares line: (slope, intercept).
pub fn linear_fit(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (x, y) in xs.iter().zip(ys) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
    }
    let slope = sxy / sxx;
    (slope, my - slope * mx)
}

// ---------------------------------------------------------------- 1st-of-month table

#[derive(Clone, Debug)]
pub struct MonthRow {
    pub year: i64,
    /// level on the 1st of each month (rounded to the millimetre)
    pub level: Vec<Option<f64>>,
    pub src: Vec<Option<Source>>,
    /// change since the first value in the column (m)
    pub total: Vec<Option<f64>>,
    /// change from the same day the year before (m)
    pub yoy: Vec<Option<f64>>,
}

#[derive(Clone, Debug)]
pub struct MonthTable {
    pub months: Vec<u32>,
    pub rows: Vec<MonthRow>,
    /// first value in each column
    pub first: Vec<Option<f64>>,
}

/// The level on the 1st of the given months for every year of the record.
pub fn month_table(s: &Series, months: &[u32]) -> MonthTable {
    let mut rows = Vec::new();
    for year in year_of(s.first())..=year_of(s.last()) {
        let (mut level, mut src) = (Vec::new(), Vec::new());
        for &m in months {
            let l = s.level_on(day(year, m, 1));
            level.push(l.map(|l| json::round(l.v, 3)));
            src.push(l.map(|l| l.source()));
        }
        rows.push(MonthRow { year, level, src, total: vec![], yoy: vec![] });
    }
    let n = months.len();
    let first: Vec<Option<f64>> = (0..n).map(|c| rows.iter().find_map(|r| r.level[c])).collect();
    for i in 0..rows.len() {
        let prev = if i > 0 { rows[i - 1].level.clone() } else { vec![None; n] };
        let r = &mut rows[i];
        r.total = (0..n).map(|c| Some(r.level[c]? - first[c]?)).collect();
        r.yoy = (0..n).map(|c| Some(r.level[c]? - prev[c]?)).collect();
    }
    MonthTable { months: months.to_vec(), rows, first }
}

impl MonthTable {
    /// Column for a month (1-12), if the table has it.
    pub fn col(&self, month: u32) -> Option<usize> {
        self.months.iter().position(|&m| m == month)
    }

    pub fn to_json(&self) -> String {
        let rows = self.rows.iter().map(|r| {
            object(&[
                ("year", r.year.to_string()),
                ("level", array(r.level.iter().map(|&v| opt(v)))),
                ("src", array(r.src.iter().map(|s| s.map_or("null".into(), |s| string(s.as_str()))))),
                ("total", array(r.total.iter().map(|&v| opt(v)))),
                ("yoy", array(r.yoy.iter().map(|&v| opt(v)))),
            ])
        });
        object(&[("rows", array(rows)), ("first", array(self.first.iter().map(|&v| opt(v))))])
    }
}

// ---------------------------------------------------------------- July 1 projection

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    /// percentile, 0-100
    pub p: u32,
    pub level: f64,
    /// vs the anchor reading (m)
    pub change: f64,
    /// vs the last July 1 (m)
    pub vs_last_jul: f64,
}

#[derive(Clone, Debug)]
pub struct LongTerm {
    pub year: i64,
    pub trend_all: f64,
    pub trend_10: f64,
    pub typical: f64,
}

#[derive(Clone, Debug)]
pub struct Projection {
    /// the reading the projection starts from (latest 1st of Jan/Apr/Jul/Oct)
    pub anchor_t: i64,
    pub anchor: f64,
    pub target_year: i64,
    /// years of history used
    pub n: usize,
    pub last_jul_year: i64,
    pub last_jul: f64,
    pub first_jul: (i64, f64),
    pub scenarios: Vec<Scenario>,
    pub med_change: f64,
    pub long_term: Vec<LongTerm>,
}

pub const SCENARIOS: [(&str, f64); 7] = [
    ("Driest on record", 0.0),
    ("Dry year", 0.10),
    ("Below normal", 0.25),
    ("Typical (median)", 0.50),
    ("Above normal", 0.75),
    ("Wet year", 0.90),
    ("Wettest on record", 1.0),
];
pub const PROJECTION_YEARS: i64 = 50;

/// July 1 projection (same method as the original Google Sheet): start from the
/// latest Jan/Apr/Jul/Oct 1st reading and add how much the lake changed between
/// that date and the next July 1 in each past year. `table` must have all 12 months.
pub fn projection(table: &MonthTable) -> Option<Projection> {
    let qc: Vec<usize> = [1, 4, 7, 10].iter().map(|&m| table.col(m)).collect::<Option<_>>()?;
    let q: Vec<(i64, [Option<f64>; 4])> =
        table.rows.iter().map(|r| (r.year, [r.level[qc[0]], r.level[qc[1]], r.level[qc[2]], r.level[qc[3]]])).collect();
    let (last_year, last) = *q.last()?;
    let ac = (0..4).rev().find(|&c| last[c].is_some())?;
    let anchor = last[ac]?;
    let shift = if ac >= 2 { 1 } else { 0 }; // anchored on Jul/Oct -> next year's July
    let target_year = last_year + shift as i64;
    let mut deltas = Vec::new();
    for i in 0..q.len() {
        if let Some((_, later)) = q.get(i + shift) {
            if let (Some(a), Some(b)) = (q[i].1[ac], later[2]) {
                deltas.push(b - a);
            }
        }
    }
    let jul: Vec<(i64, f64)> = q.iter().filter_map(|(y, l)| l[2].map(|v| (*y, v))).collect();
    if deltas.len() < 3 || jul.len() < 3 {
        return None;
    }
    let (last_jul_year, last_jul) = *jul.last()?;
    let scenarios = SCENARIOS
        .iter()
        .map(|&(name, p)| {
            let level = anchor + percentile(&deltas, p);
            Scenario { name, p: (p * 100.0).round() as u32, level, change: level - anchor, vs_last_jul: level - last_jul }
        })
        .collect();
    let ys: Vec<f64> = jul.iter().map(|r| r.0 as f64).collect();
    let vs: Vec<f64> = jul.iter().map(|r| r.1).collect();
    let (sa, ia) = linear_fit(&ys, &vs);
    let k = ys.len().saturating_sub(10);
    let (s10, i10) = linear_fit(&ys[k..], &vs[k..]);
    let changes: Vec<f64> = vs.windows(2).map(|w| w[1] - w[0]).collect();
    let med_change = median(&changes);
    let long_term = (target_year..target_year + PROJECTION_YEARS)
        .map(|y| LongTerm {
            year: y,
            trend_all: sa * y as f64 + ia,
            trend_10: s10 * y as f64 + i10,
            typical: last_jul + med_change * (y - last_jul_year) as f64,
        })
        .collect();
    Some(Projection {
        anchor_t: day(last_year, [1, 4, 7, 10][ac], 1),
        anchor,
        target_year,
        n: deltas.len(),
        last_jul_year,
        last_jul,
        first_jul: jul[0],
        scenarios,
        med_change,
        long_term,
    })
}

impl Projection {
    /// Field names match what the page expects.
    pub fn to_json(&self) -> String {
        let scen = self.scenarios.iter().map(|s| {
            object(&[
                ("name", string(s.name)),
                ("p", s.p.to_string()),
                ("level", num(s.level)),
                ("change", num(s.change)),
                ("vsLastJul", num(s.vs_last_jul)),
            ])
        });
        let lt = self.long_term.iter().map(|r| {
            object(&[
                ("year", r.year.to_string()),
                ("trendAll", num(r.trend_all)),
                ("trend10", num(r.trend_10)),
                ("typical", num(r.typical)),
            ])
        });
        object(&[
            ("anchorT", self.anchor_t.to_string()),
            ("anchor", num(self.anchor)),
            ("targetYear", self.target_year.to_string()),
            ("n", self.n.to_string()),
            ("lastJulYear", self.last_jul_year.to_string()),
            ("lastJul", num(self.last_jul)),
            ("firstJul", array(vec![self.first_jul.0.to_string(), num(self.first_jul.1)])),
            ("scenarios", array(scen)),
            ("medChange", num(self.med_change)),
            ("longTerm", array(lt)),
        ])
    }
}

// ---------------------------------------------------------------- summary tiles

pub fn level_json(l: Option<Level>) -> String {
    match l {
        None => "null".into(),
        Some(l) => object(&[
            ("v", num(l.v)),
            ("measured", l.measured.to_string()),
            ("prov", l.prov.to_string()),
        ]),
    }
}

/// Everything the summary tiles show for month `m`, day `d` (the latest reading
/// if that is the latest reading's date), looking back `lookback` months.
pub fn tiles_json(s: &Series, m: u32, d: u32, lookback: i64) -> String {
    let (last, first) = (s.last(), s.first());
    let (ly, lm, ld) = ymd(last);
    let year = if day(ly, m, d) <= last { ly } else { ly - 1 };
    let now = s.level_on(day(year, m, d));
    let prev = s.level_on(day(year - 1, m, d));
    let mut y0 = year_of(first);
    if s.level_on(day(y0, m, d)).is_none() {
        y0 += 1;
    }
    let first_l = s.level_on(day(y0, m, d));
    let back_t = add_months(year, m, d, -lookback);
    let back = s.level_on(back_t);
    let diff = |a: Option<Level>, b: Option<Level>| Some(a?.v - b?.v);

    // outlook: next July 1 from the latest reading, using how the lake moved over
    // the same calendar stretch in each past year
    let jul_this = day(ly, 7, 1);
    let target_year = if last >= jul_this { ly + 1 } else { ly };
    let k = target_year - ly;
    let mut deltas = Vec::new();
    let mut y = year_of(first);
    while day(y + k, 7, 1) <= last {
        if let (Some(a), Some(b)) = (s.level_on(day(y, lm, ld)), s.level_on(day(y + k, 7, 1))) {
            deltas.push(b.v - a.v);
        }
        y += 1;
    }
    let jul_year = if last >= jul_this { ly } else { ly - 1 };
    let last_jul = s.level_on(day(jul_year, 7, 1));
    let outlook = match last_jul {
        Some(lj) if deltas.len() >= 3 => {
            let at = |p: f64| s.latest() + percentile(&deltas, p) - lj.v;
            object(&[
                ("median", num(at(0.5))),
                ("p10", num(at(0.1))),
                ("p90", num(at(0.9))),
                ("n", deltas.len().to_string()),
            ])
        }
        _ => "null".into(),
    };
    object(&[
        ("m", m.to_string()),
        ("d", d.to_string()),
        ("year", year.to_string()),
        ("isLatest", (m == lm && d == ld).to_string()),
        ("now", level_json(now)),
        ("yoy", opt(diff(now, prev))),
        ("firstYear", y0.to_string()),
        ("total", opt(diff(now, first_l))),
        ("backT", back_t.to_string()),
        ("back", opt(diff(now, back))),
        ("hasBack", back.is_some().to_string()),
        ("outlookYear", target_year.to_string()),
        ("julYear", jul_year.to_string()),
        ("outlook", outlook),
    ])
}

/// Levels on two days and the change between them.
pub fn compare_json(s: &Series, a: i64, b: i64) -> String {
    let (la, lb) = (s.level_on(a), s.level_on(b));
    let (diff, per_year) = match (la, lb) {
        (Some(x), Some(y)) => {
            let days = b - a;
            let d = y.v - x.v;
            (Some(d), if days != 0 { Some(d / ((days.abs() as f64) / 365.25)) } else { None })
        }
        _ => (None, None),
    };
    object(&[
        ("a", level_json(la)),
        ("b", level_json(lb)),
        ("diff", opt(diff)),
        ("days", (b - a).to_string()),
        ("perYear", opt(per_year)),
    ])
}

/// Every daily level as CSV, with change since the first reading.
pub fn daily_csv(s: &Series) -> String {
    let mut out = String::from("Date,Level (m),vs first reading (m),vs first reading (ft),Source\n");
    let base = s.v[0];
    for i in 0..s.len() {
        let d = s.v[i] - base;
        out.push_str(&format!(
            "{},{:.3},{:.3},{:.3},{}\n",
            crate::dates::iso(s.t[i]),
            s.v[i],
            d,
            d / 0.3048,
            if s.p[i] { "Provisional (real-time)" } else { "Approved (HYDAT)" }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats() {
        let v = [5.0, 1.0, 3.0, 2.0];
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&v, 1.0), 5.0);
        assert_eq!(median(&v), 2.5);
        assert!((percentile(&v, 0.1) - 1.3).abs() < 1e-12);
        let (s, i) = linear_fit(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]);
        assert!((s - 2.0).abs() < 1e-12 && i.abs() < 1e-12);
    }
}
