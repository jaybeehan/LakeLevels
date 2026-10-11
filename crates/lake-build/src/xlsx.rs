//! The Excel download: every daily reading, the 1st-of-month tables, the July 1
//! projection and a sheet of charts (site/data/muriel-lake-water-levels.xlsx).

use crate::{PAGE_URL, STATION, STATION_NAME};
use lake_core::calc::{MonthRow, MonthTable, Projection};
use lake_core::dates::{excel_serial, iso};
use lake_core::json::round;
use lake_core::{Series, Source};
use rust_xlsxwriter::{
    Chart, ChartAxisDateUnitType, ChartAxisLabelPosition, ChartEmptyCells, ChartFormat, ChartLegendPosition, ChartLine,
    ChartLineDashType, ChartMarker, ChartType, Format, FormatAlign, Workbook, Worksheet, XlsxError,
};
use std::path::Path;

const MONTH_NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const IN_PER_M: f64 = 100.0 / 2.54;
const BLUE: &str = "#2F6FB3";
const ORANGE: &str = "#D9822B";
const GREEN: &str = "#2E9A5B";
const PURPLE: &str = "#8B5CC4";
const GREY: &str = "#8A94A3";
// chart size, about 24 cm x 10 cm
const CHART_W: u32 = 907;
const CHART_H: u32 = 378;

fn head_format() -> Format {
    Format::new().set_bold().set_background_color("#DCE8F5").set_align(FormatAlign::Center).set_text_wrap()
}

/// Bold header row with column widths (`widths` gives exceptions to 12), panes frozen at B2.
fn header(ws: &mut Worksheet, cols: &[&str], widths: &[(u16, f64)]) -> Result<(), XlsxError> {
    let head = head_format();
    for (c, name) in cols.iter().enumerate() {
        let c = c as u16;
        ws.write_string_with_format(0, c, *name, &head)?;
        let w = widths.iter().find(|(k, _)| *k == c).map_or(12.0, |(_, w)| *w);
        ws.set_column_width(c, w)?;
    }
    ws.set_freeze_panes(1, 1)?;
    Ok(())
}

fn line(colour: &str, width_pt: f64, dash: bool) -> ChartFormat {
    let mut l = ChartLine::new();
    l.set_color(colour).set_width(width_pt);
    if dash {
        l.set_dash_type(ChartLineDashType::Dash);
    }
    let mut f = ChartFormat::new();
    f.set_line(&l);
    f
}

fn tidy(ch: &mut Chart, title: &str, ytitle: &str, xtitle: &str) {
    ch.title().set_name(title);
    ch.y_axis().set_name(ytitle).set_num_format("0.0");
    ch.x_axis().set_name(xtitle).set_label_position(ChartAxisLabelPosition::Low);
    ch.set_width(CHART_W).set_height(CHART_H);
    ch.legend().set_position(ChartLegendPosition::Bottom);
}

/// One table of the 1st of each month (a row per year), from `value(row, month index)`.
/// `rises`: shade rises green; otherwise interpolated values are grey italic.
fn month_sheet(
    rows: &[MonthRow],
    title: &str,
    value: impl Fn(&MonthRow, usize) -> Option<f64>,
    fmt: &str,
    rises: bool,
) -> Result<Worksheet, XlsxError> {
    let mut ws = Worksheet::new();
    ws.set_name(title)?;
    let heads: Vec<String> = MONTH_NAMES.iter().map(|m| format!("{m} 1")).collect();
    let mut cols = vec!["Year"];
    cols.extend(heads.iter().map(|s| s.as_str()));
    header(&mut ws, &cols, &[(0, 8.0)])?;
    let plain = Format::new().set_num_format(fmt);
    let rise = Format::new().set_num_format(fmt).set_background_color("#DCF1E2").set_bold().set_font_color("#1E7B3C");
    let interp = Format::new().set_num_format(fmt).set_italic().set_font_color(GREY);
    for (i, q) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        ws.write_number(r, 0, q.year as f64)?;
        for c in 0..12 {
            let Some(v) = value(q, c) else { continue };
            let f = if rises && v > 0.0 {
                &rise
            } else if !rises && q.src[c] == Some(Source::Interpolated) {
                &interp
            } else {
                &plain
            };
            ws.write_number_with_format(r, c as u16 + 1, v, f)?;
        }
    }
    Ok(ws)
}

pub fn write(
    path: &Path,
    s: &Series,
    table: &MonthTable,
    proj: Option<&Projection>,
    updated: &str,
    approved_until: i64,
) -> Result<(), XlsxError> {
    let bold = Format::new().set_bold();
    let head = head_format();
    let f3 = Format::new().set_num_format("0.000");
    let f1 = Format::new().set_num_format("0.0");
    let rows = &table.rows;
    let n_rows = rows.len() as u32;

    // ---- Daily
    let mut daily = Worksheet::new();
    daily.set_name("Daily")?;
    header(
        &mut daily,
        &["Date", "Level (m above sea level)", "vs first reading (m)", "vs first reading (ft)", "vs first reading (in)", "Source"],
        &[(1, 16.0), (2, 14.0), (3, 14.0), (4, 14.0), (5, 26.0)],
    )?;
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd");
    let base = s.v[0];
    for i in 0..s.len() {
        let r = i as u32 + 1;
        let (v, d) = (s.v[i], s.v[i] - base);
        daily.write_number_with_format(r, 0, excel_serial(s.t[i]), &date_fmt)?;
        daily.write_number_with_format(r, 1, round(v, 3), &f3)?;
        daily.write_number_with_format(r, 2, round(d, 3), &f3)?;
        daily.write_number_with_format(r, 3, round(d / 0.3048, 3), &f3)?;
        daily.write_number_with_format(r, 4, round(d * IN_PER_M, 1), &f1)?;
        daily.write_string(r, 5, if s.p[i] { "Provisional (real-time)" } else { "Approved (HYDAT)" })?;
    }
    let n_daily = s.len() as u32;

    // ---- 1st-of-month sheets
    let cm = |x: Option<f64>| x.map(|v| round(v * 100.0, 1));
    let inch = |x: Option<f64>| cm(x).map(|v| round(v / 2.54, 1));
    let signed = "+0.0;-0.0;0.0";
    let levels = month_sheet(rows, "1st of month (m)", |q: &MonthRow, c: usize| q.level[c], "0.000", false)?;
    let yoy_cm = month_sheet(rows, "Year-to-year (cm)", |q: &MonthRow, c: usize| cm(q.yoy[c]), signed, true)?;
    let yoy_in = month_sheet(rows, "Year-to-year (in)", |q: &MonthRow, c: usize| inch(q.yoy[c]), signed, true)?;
    let tot_cm = month_sheet(rows, "Total change (cm)", |q: &MonthRow, c: usize| cm(q.total[c]), signed, false)?;
    let tot_in = month_sheet(rows, "Total change (in)", |q: &MonthRow, c: usize| inch(q.total[c]), signed, false)?;

    // ---- July 1 projection
    const PROJ: &str = "July 1 projection";
    let mut pw = Worksheet::new();
    pw.set_name(PROJ)?;
    let mut july_rows = None; // (header row, last row) of the July 1 chart data
    if let Some(p) = proj {
        pw.write_string_with_format(
            0,
            0,
            format!(
                "Projection for {}, starting from the {} reading ({:.3} m), using {} years of history",
                iso(lake_core::dates::day(p.target_year, 7, 1)),
                iso(p.anchor_t),
                p.anchor,
                p.n
            ),
            &bold,
        )?;
        let cols = [
            "Scenario".to_string(),
            "Percentile".into(),
            "Projected level (m)".into(),
            "vs latest reading (cm)".into(),
            "vs latest reading (in)".into(),
            format!("vs July 1, {} (cm)", p.last_jul_year),
            format!("vs July 1, {} (in)", p.last_jul_year),
        ];
        for (c, name) in cols.iter().enumerate() {
            pw.write_string_with_format(2, c as u16, name.as_str(), &head)?;
            pw.set_column_width(c as u16, 18)?;
        }
        let pct = Format::new().set_num_format("0%");
        let mut r = 3u32;
        for sc in &p.scenarios {
            let change_cm = round(sc.change * 100.0, 1);
            let vs_cm = round(sc.vs_last_jul * 100.0, 1);
            pw.write_string(r, 0, sc.name)?;
            pw.write_number_with_format(r, 1, sc.p as f64 / 100.0, &pct)?;
            pw.write_number_with_format(r, 2, round(sc.level, 3), &f3)?;
            pw.write_number(r, 3, change_cm)?;
            pw.write_number(r, 4, round(change_cm / 2.54, 1))?;
            pw.write_number(r, 5, vs_cm)?;
            pw.write_number(r, 6, round(vs_cm / 2.54, 1))?;
            r += 1;
        }
        r += 1;
        for (c, name) in ["July 1", "Trend, all years (m)", "Trend, last 10 years (m)", "Typical yearly change (m)"].iter().enumerate() {
            pw.write_string_with_format(r, c as u16, *name, &head)?;
        }
        for lt in &p.long_term {
            r += 1;
            pw.write_number(r, 0, lt.year as f64)?;
            pw.write_number_with_format(r, 1, round(lt.trend_all, 3), &f3)?;
            pw.write_number_with_format(r, 2, round(lt.trend_10, 3), &f3)?;
            pw.write_number_with_format(r, 3, round(lt.typical, 3), &f3)?;
        }
        // data for the July 1 chart: actual July 1 levels, then the projections
        r += 2;
        let jstart = r;
        for (c, name) in ["Year", "Actual July 1 (m)", "Trend, last 10 years (m)", "Typical yearly change (m)"].iter().enumerate() {
            pw.write_string_with_format(r, c as u16, *name, &head)?;
        }
        let jc = table.col(7).unwrap_or(6);
        let last_year = rows.last().map_or(i64::MIN, |q| q.year);
        for q in rows {
            r += 1;
            pw.write_number(r, 0, q.year as f64)?;
            if let Some(v) = q.level[jc] {
                pw.write_number_with_format(r, 1, v, &f3)?;
            }
            if q.year == p.last_jul_year {
                // join the projection lines to the last actual value
                pw.write_number_with_format(r, 2, p.last_jul, &f3)?;
                pw.write_number_with_format(r, 3, p.last_jul, &f3)?;
            }
        }
        for lt in p.long_term.iter().filter(|lt| lt.year > last_year) {
            r += 1;
            pw.write_number(r, 0, lt.year as f64)?;
            pw.write_number_with_format(r, 2, round(lt.trend_10, 3), &f3)?;
            pw.write_number_with_format(r, 3, round(lt.typical, 3), &f3)?;
        }
        july_rows = Some((jstart, r));
    } else {
        pw.write_string(0, 0, "Not enough history yet for a July 1 projection.")?;
    }

    // ---- Charts
    let mut cs = Worksheet::new();
    cs.set_name("Charts")?;
    cs.write_string_with_format(
        0,
        0,
        format!("Charts — Muriel Lake ({STATION}). The tables they come from are on the other sheets."),
        &Format::new().set_bold().set_font_size(13),
    )?;
    cs.set_landscape();
    cs.set_print_fit_to_pages(1, 1); // prints/exports the charts on one page

    let daily_chart = |col: u16, title: &str, ytitle: &str, colour: &str| -> Chart {
        let mut ch = Chart::new(ChartType::Line);
        tidy(&mut ch, title, ytitle, "Date");
        ch.x_axis()
            .set_date_axis(true)
            .set_num_format("yyyy")
            .set_major_unit(5)
            .set_major_unit_date_type(ChartAxisDateUnitType::Years);
        ch.add_series()
            .set_name(("Daily", 0, col))
            .set_values(("Daily", 1, col, n_daily, col))
            .set_categories(("Daily", 1, 0, n_daily, 0))
            .set_format(&mut line(colour, 1.2, false))
            .set_marker(ChartMarker::new().set_none());
        ch.legend().set_hidden();
        ch
    };
    cs.insert_chart(2, 0, &daily_chart(2, "Daily water level vs first reading (m)", "Change (m)", BLUE))?;
    cs.insert_chart(2, 15, &daily_chart(3, "Daily water level vs first reading (ft)", "Change (ft)", ORANGE))?;

    if let Some((jstart, jend)) = july_rows {
        let mut ch = Chart::new(ChartType::Line);
        tidy(&mut ch, "July 1 water level: history and projections", "Level (m above sea level)", "Year");
        for (col, colour, dash) in [(1u16, BLUE, false), (2, PURPLE, true), (3, GREY, true)] {
            ch.add_series()
                .set_name((PROJ, jstart, col))
                .set_values((PROJ, jstart + 1, col, jend, col))
                .set_categories((PROJ, jstart + 1, 0, jend, 0))
                .set_format(&mut line(colour, 1.75, dash))
                .set_marker(ChartMarker::new().set_none());
        }
        ch.show_empty_cells_as(ChartEmptyCells::Gaps);
        ch.x_axis().set_label_interval(5);
        cs.insert_chart(23, 0, &ch)?;
    }

    const TOT: &str = "Total change (cm)";
    let mut ch = Chart::new(ChartType::Line);
    tidy(&mut ch, "Total change since first reading, 1st of Jan / Apr / Jul / Oct (cm)", "Change (cm)", "Year");
    for (col, colour) in [(1u16, BLUE), (4, ORANGE), (7, GREEN), (10, PURPLE)] {
        ch.add_series()
            .set_name((TOT, 0, col))
            .set_values((TOT, 1, col, n_rows, col))
            .set_categories((TOT, 1, 0, n_rows, 0))
            .set_format(&mut line(colour, 1.6, false))
            .set_marker(ChartMarker::new().set_none());
    }
    ch.show_empty_cells_as(ChartEmptyCells::Gaps);
    ch.x_axis().set_label_interval(5);
    cs.insert_chart(23, 15, &ch)?;

    const YOY: &str = "Year-to-year (cm)";
    for (i, (col, name)) in [(1u16, "Jan 1"), (4, "Apr 1"), (7, "Jul 1"), (10, "Oct 1")].into_iter().enumerate() {
        let mut ch = Chart::new(ChartType::Column);
        tidy(&mut ch, &format!("{name}: change from the year before (cm) — green = rose"), "Change (cm)", "Year");
        let colours: Vec<&str> =
            rows.iter().map(|q| if cm(q.yoy[col as usize - 1]).is_some_and(|v| v > 0.0) { GREEN } else { ORANGE }).collect();
        ch.add_series()
            .set_name((YOY, 0, col))
            .set_values((YOY, 1, col, n_rows, col))
            .set_categories((YOY, 1, 0, n_rows, 0))
            .set_gap(40)
            .set_point_colors(&colours);
        ch.legend().set_hidden();
        ch.x_axis().set_label_interval(5);
        cs.insert_chart(44 + 21 * (i as u32 / 2), if i % 2 == 0 { 0 } else { 15 }, &ch)?;
    }

    // ---- About
    let mut about = Worksheet::new();
    about.set_name("About")?;
    let lines = [
        format!("Muriel Lake water levels — Water Survey of Canada station {STATION} ({STATION_NAME})"),
        format!("Generated {updated} (UTC). Latest reading {}: {:.3} m.", iso(s.last()), round(s.latest(), 3)),
        format!("Approved historical data (HYDAT) through {}; later dates are provisional real-time data.", iso(approved_until)),
        "Days with no reading are filled by straight-line interpolation (grey italic in the 1st-of-month table).".into(),
        "Green cells in the year-to-year sheets mean the lake was higher than on the same date the year before.".into(),
        "Data © Environment and Climate Change Canada, Open Government Licence – Canada.".into(),
        format!("Live page: {PAGE_URL}"),
        "Built by lake-build, written in Rust: https://github.com/jaybeehan/LakeLevels".into(),
    ];
    for (r, text) in lines.iter().enumerate() {
        if r == 0 {
            about.write_string_with_format(0, 0, text.as_str(), &Format::new().set_bold().set_font_size(13))?;
        } else {
            about.write_string(r as u32, 0, text.as_str())?;
        }
    }
    about.set_column_width(0, 110)?;

    let mut wb = Workbook::new();
    for ws in [cs, daily, levels, yoy_cm, yoy_in, tot_cm, tot_in, pw, about] {
        wb.push_worksheet(ws);
    }
    wb.save(path)?;
    Ok(())
}
