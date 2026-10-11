# Lake Levels (Rust)

A web page showing water levels for Muriel Lake, Alberta (Water Survey of Canada station **06AC007**), updated automatically every day, and able to show any other Water Survey of Canada station.

This is the Rust version of [muriel-lake](https://github.com/jaybeehan/muriel-lake): the same page and features, with the data pipeline and all the calculations rewritten in Rust.

**Live page:** https://jaybeehan.github.io/LakeLevels/
**Another station:** add `?station=CODE`, e.g. https://jaybeehan.github.io/LakeLevels/?station=05FA013

## What it shows

- Summary tiles for the latest reading (or any day you pick): level, change vs a year earlier, change vs the first reading on record, gained/lost over the last 1–24 months, and the July 1 outlook
- Change between any two dates
- Every daily reading, with a From date and 1 / 5 / 10 / 20-year buttons; the change from the left edge to the latest reading is shown above the chart, and hovering shows the change from that day to the latest reading
- Change on the 1st of each month (total change and change vs the year before, rises in green)
- July 1 projection (dry / typical / wet range) and 50-year trend lines
- Weather forecast for the station's location
- Metric or feet & inches; light or dark
- Station search (all ~8,000 stations), with Muriel Lake overlaid for comparison
- Excel download for Muriel Lake (tables plus charts); CSV download for other stations

## How it's built

```
crates/
  lake-core/    the maths, plain Rust with no dependencies: merging approved and real-time
                data, interpolation, 1st-of-month tables, July 1 projection, summary tiles
  lake-build/   command-line program that downloads the data and writes site/data/
                (replaces the Python script scripts/build_data.py)
  lake-web/     lake-core compiled to WebAssembly for the page
site/
  index.html    the page: fetches data, draws charts (Plotly) and calls the Rust code for every number
```

1. Every day a GitHub Action builds the Rust code and runs `lake-build`, which downloads Muriel Lake's approved historical record (HYDAT) and its last ~18 months of provisional real-time data from Environment Canada. It writes `site/data/lake.json` (the merged daily series), `site/data/stations.json` (the station list) and the Excel workbook. Approved data always wins over real-time data.
2. The same Action compiles `lake-web` to WebAssembly (`site/pkg/`) and publishes `site/` to GitHub Pages. All calculations (interpolation, monthly tables, projections) run in the browser in that WebAssembly module, so the same code works for any station, and the page and the Excel file always agree.
3. For other stations the page loads data live from Environment Canada's API: the approved record plus the last 30 days of real-time data, merged by the same Rust code. (WaterOffice's 18-month real-time service doesn't allow other websites to read it, so dates between the end of the approved record and the last 30 days are interpolated, and the page says so.)

To update right away, go to the **Actions** tab → **Update data and publish** → **Run workflow**.

### Running it yourself

With [Rust](https://rustup.rs) installed:

```
cargo test -p lake-core                      # check the maths
cargo run -p lake-build --release            # download data into site/data/
```

To build the page's WebAssembly module locally:

```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli               # same version as the wasm-bindgen crate in Cargo.lock
cargo build -p lake-web --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir site/pkg target/wasm32-unknown-unknown/release/lake_web.wasm
```

Then serve `site/` with any local web server (WebAssembly won't load from a `file://` page).

## Data sources

- Approved daily means: https://api.weather.gc.ca/collections/hydrometric-daily-mean
- Real-time (provisional): https://wateroffice.ec.gc.ca/report/real_time_e.html?stn=06AC007 and https://api.weather.gc.ca/collections/hydrometric-realtime
- Stations: https://api.weather.gc.ca/collections/hydrometric-stations
- Weather: https://open-meteo.com/

Water data © Environment and Climate Change Canada, used under the Open Government Licence – Canada.
