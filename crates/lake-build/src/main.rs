//! Builds the data files for the Muriel Lake water level page.
//!
//! Pulls water levels for Water Survey of Canada station 06AC007 (Muriel Lake
//! near Gurneyville, AB), merges the approved historical record with the
//! provisional real-time record, and writes (into `site/data/`, or the folder
//! given as the first argument):
//!
//!   lake.json                       the merged daily series (the page does all the maths)
//!   stations.json                   every hydrometric station, for the page's station search
//!   muriel-lake-water-levels.xlsx   full tables, projections and charts for download
//!
//! Data sources
//!   * Approved historical daily means (HYDAT), via the GeoMet OGC API:
//!       https://api.weather.gc.ca/collections/hydrometric-daily-mean
//!   * Provisional real-time readings (about the last 18 months), via WaterOffice:
//!       https://wateroffice.ec.gc.ca/services/real_time_data/csv/inline
//!
//! Approved data always wins. Real-time data only fills dates the approved
//! record doesn't have yet, so when Environment Canada publishes a new year of
//! approved data the page switches to it automatically on the next run.

mod xlsx;

use lake_core::csv::Csv;
use lake_core::dates::{day, iso, ymd};
use lake_core::json::{self, array, num, object, string};
use lake_core::{month_table, projection, Loader};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const STATION: &str = "06AC007";
pub const STATION_NAME: &str = "Muriel Lake near Gurneyville";
const HYDAT_URL: &str = "https://api.weather.gc.ca/collections/hydrometric-daily-mean/items";
const STATIONS_URL: &str = "https://api.weather.gc.ca/collections/hydrometric-stations/items";
const REALTIME_URL: &str = "https://wateroffice.ec.gc.ca/services/real_time_data/csv/inline";
const PAGE_SIZE: usize = 10_000;
const REALTIME_MONTHS: i64 = 19; // WaterOffice keeps roughly 18 months
const LST_OFFSET_DAYS: f64 = 7.0 / 24.0; // Alberta local standard time = UTC-7 (what HYDAT uses)
const LAT: f64 = 54.1446;
const LON: f64 = -110.7440;
pub const XLSX_NAME: &str = "muriel-lake-water-levels.xlsx";
pub const PAGE_URL: &str = "https://jaybeehan.github.io/LakeLevels/";

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "site/data".into()));
    if let Err(e) = build(&out) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------- fetching

/// Percent-encode a query value, leaving `[]:,` alone like the Python version did.
fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'[' | b']' | b':' | b',' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

struct Fetcher {
    agent: ureq::Agent,
}

impl Fetcher {
    fn new() -> Fetcher {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(120))
            .user_agent("lake-levels (github.com/jaybeehan/LakeLevels)")
            .build();
        Fetcher { agent }
    }

    /// GET `url?params` as text, retrying a few times on network or server errors.
    fn get(&self, url: &str, params: &[(&str, String)]) -> Res<String> {
        let q: Vec<String> = params.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect();
        let full = format!("{url}?{}", q.join("&"));
        let tries = 4;
        for attempt in 0..tries {
            match self.agent.get(&full).call() {
                Ok(resp) => {
                    let mut bytes = Vec::new();
                    resp.into_reader().read_to_end(&mut bytes)?;
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    return Ok(text.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(text));
                }
                Err(e) => {
                    if attempt == tries - 1 {
                        return Err(Box::new(e));
                    }
                    eprintln!("  retrying after error: {e}");
                    std::thread::sleep(Duration::from_secs(10 * (attempt as u64 + 1)));
                }
            }
        }
        unreachable!()
    }
}

/// Approved daily means.
fn fetch_hydat(f: &Fetcher, loader: &mut Loader) -> Res<()> {
    let mut offset = 0;
    loop {
        let text = f.get(HYDAT_URL, &[
            ("STATION_NUMBER", STATION.into()),
            ("f", "csv".into()),
            ("limit", PAGE_SIZE.to_string()),
            ("offset", offset.to_string()),
            ("sortby", "DATE".into()),
            ("properties", "DATE,LEVEL".into()),
        ])?;
        let rows = loader.add_daily_mean_csv(&text);
        println!("  HYDAT offset {offset}: {rows} rows");
        if rows < PAGE_SIZE {
            return Ok(());
        }
        offset += PAGE_SIZE;
    }
}

/// Provisional readings, one month at a time, averaged to local-standard-time days.
fn fetch_realtime(f: &Fetcher, loader: &mut Loader, today: i64) {
    let (ty, tm, _) = ymd(today);
    for k in 0..REALTIME_MONTHS {
        let mm = tm as i64 - 1 - k;
        let (y, m) = (ty + mm.div_euclid(12), (mm.rem_euclid(12) + 1) as u32);
        let start = day(y, m, 1);
        let end = day(y + (m == 12) as i64, m % 12 + 1, 1);
        let text = match f.get(REALTIME_URL, &[
            ("stations[]", STATION.into()),
            ("parameters[]", "46".into()),
            ("start_date", format!("{} 07:00:00", iso(start))),
            ("end_date", format!("{} 07:00:00", iso(end))),
        ]) {
            Ok(t) => t,
            Err(e) => {
                // a missing month shouldn't sink the whole run
                eprintln!("  real-time {y}-{m:02}: failed ({e})");
                continue;
            }
        };
        // the closing reading belongs to next month's chunk
        let n = loader.add_wateroffice_csv(&text, LST_OFFSET_DAYS, end);
        println!("  real-time {y}-{m:02}: {n} readings");
    }
}

/// All hydrometric stations as JSON: [number, name, province, lat, lon, active, real_time].
fn fetch_stations(f: &Fetcher) -> Res<(String, usize)> {
    let text = f.get(STATIONS_URL, &[
        ("f", "csv".into()),
        ("limit", "20000".into()),
        ("properties", "STATION_NUMBER,STATION_NAME,PROV_TERR_STATE_LOC,STATUS_EN,REAL_TIME".into()),
    ])?;
    let csv = Csv::parse(&text);
    let c = |n: &str| csv.col(n);
    let (cn, cname, cprov, cst, crt, cx, cy) =
        (c("STATION_NUMBER"), c("STATION_NAME"), c("PROV_TERR_STATE_LOC"), c("STATUS_EN"), c("REAL_TIME"), c("x"), c("y"));
    let mut out: Vec<(String, String)> = Vec::new();
    for row in &csv.rows {
        let code = Csv::get(row, cn);
        let (Ok(lat), Ok(lon)) = (Csv::get(row, cy).parse::<f64>(), Csv::get(row, cx).parse::<f64>()) else { continue };
        if code.is_empty() || cname.is_none() || cprov.is_none() {
            continue;
        }
        let item = array(vec![
            string(code),
            string(Csv::get(row, cname)),
            string(Csv::get(row, cprov)),
            num(json::round(lat, 4)),
            num(json::round(lon, 4)),
            if Csv::get(row, cst) == "Active" { "1" } else { "0" }.into(),
            if Csv::get(row, crt) == "1" { "1" } else { "0" }.into(),
        ]);
        out.push((code.to_string(), item));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    let n = out.len();
    Ok((array(out.into_iter().map(|x| x.1)), n))
}

// ---------------------------------------------------------------- build

/// Now as (day number, "YYYY-MM-DDTHH:MM:SSZ"), in UTC.
fn now_utc() -> (f64, String) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let t = secs.div_euclid(86_400);
    let s = secs.rem_euclid(86_400);
    let stamp = format!("{}T{:02}:{:02}:{:02}Z", iso(t), s / 3600, s % 3600 / 60, s % 60);
    (secs as f64 / 86_400.0, stamp)
}

fn kb(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len() / 1024).unwrap_or(0)
}

fn build(out: &Path) -> Res<()> {
    let (now_days, updated) = now_utc();
    let today = (now_days - LST_OFFSET_DAYS).floor() as i64;
    let f = Fetcher::new();
    let mut loader = Loader::new();
    println!("Fetching approved historical data…");
    fetch_hydat(&f, &mut loader)?;
    let Some(approved_until) = loader.approved_until() else {
        return Err("No historical data came back; leaving the old data in place.".into());
    };
    println!("Fetching real-time data…");
    fetch_realtime(&f, &mut loader, today);

    let (s, _) = loader.finish();
    println!("Merged series: {} days, {} to {}", s.len(), iso(s.first()), iso(s.last()));

    // compact daily series for the page: day offsets from t0, levels, provisional flags
    std::fs::create_dir_all(out)?;
    let t0 = s.first();
    let page = object(&[
        ("station", string(STATION)),
        ("station_name", string(STATION_NAME)),
        ("lat", num(LAT)),
        ("lon", num(LON)),
        ("updated", string(&updated)),
        ("approved_until", string(&iso(approved_until))),
        ("realtime_note", string("Provisional real-time data covers about the last 18 months.")),
        ("t0", string(&iso(t0))),
        ("d", array(s.t.iter().map(|t| (t - t0).to_string()))),
        ("v", array(s.v.iter().map(|&v| num(json::round(v, 3))))),
        ("p", string(&s.p.iter().map(|&p| if p { '1' } else { '0' }).collect::<String>())),
    ]);
    let lake = out.join("lake.json");
    std::fs::write(&lake, page)?;
    println!("Wrote {} ({} KB)", lake.display(), kb(&lake));

    match fetch_stations(&f) {
        Ok((js, n)) if n > 1000 => {
            let p = out.join("stations.json");
            std::fs::write(&p, js)?;
            println!("Wrote {} ({n} stations, {} KB)", p.display(), kb(&p));
        }
        Ok((_, n)) => eprintln!("Station list looked incomplete ({n} rows); skipped"),
        // the page falls back to searching the API directly
        Err(e) => eprintln!("Station list failed: {e}"),
    }

    let months: Vec<u32> = (1..=12).collect();
    let table = month_table(&s, &months);
    let proj = projection(&table);
    let xp = out.join(XLSX_NAME);
    xlsx::write(&xp, &s, &table, proj.as_ref(), &updated, approved_until)?;
    println!("Wrote {} ({} KB)", xp.display(), kb(&xp));
    Ok(())
}
