// Checks the built site before it's published: loads the WebAssembly module in
// Node, feeds it today's lake.json and runs every call the page makes.
// Usage: node scripts/smoke-test.mjs [site folder]
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";

const site = path.resolve(process.argv[2] || "site");
const pkg = path.join(site, "pkg"), data = path.join(site, "data");
const DAY = 864e5;

// lake_web.js is an ES module; give it an .mjs name so any Node version loads it as one
const tmp = path.join(pkg, "lake_web.smoke-test.mjs");
fs.copyFileSync(path.join(pkg, "lake_web.js"), tmp);
try {
  const { default: init, Lake, StationLoader } = await import(tmp);
  await init({ module_or_path: fs.readFileSync(path.join(pkg, "lake_web_bg.wasm")) });

  // ---- the data files
  const j = JSON.parse(fs.readFileSync(path.join(data, "lake.json"), "utf8"));
  assert.equal(j.station, "06AC007");
  assert.equal(j.d.length, j.v.length);
  assert.equal(j.p.length, j.v.length);
  const stations = JSON.parse(fs.readFileSync(path.join(data, "stations.json"), "utf8"));
  assert.ok(stations.length > 1000, "station list");
  assert.ok(stations.some((s) => s[0] === "06AC007"), "Muriel Lake in the station list");
  const xlsx = fs.statSync(path.join(data, "muriel-lake-water-levels.xlsx")).size;
  assert.ok(xlsx > 50_000, "Excel file");

  // ---- Muriel Lake through the WebAssembly module
  const lake = Lake.from_compact(j.t0, Float64Array.from(j.d), Float64Array.from(j.v), j.p);
  assert.equal(lake.len(), j.d.length);
  const last = lake.last(), lastDate = new Date(last * DAY);
  const ageDays = (Date.now() - last * DAY) / DAY;
  assert.ok(ageDays < 120, `latest reading is ${ageDays.toFixed(0)} days old`);

  const M = JSON.parse(lake.monthly_json());
  assert.ok(M.rows.length >= 30 && M.rows.every((r) => r.level.length === 12));
  const P = JSON.parse(lake.projection_json());
  assert.ok(P && P.scenarios.length === 7 && P.longTerm.length === 50, "projection");
  assert.ok(P.scenarios[0].level <= P.scenarios[3].level && P.scenarios[3].level <= P.scenarios[6].level);

  const T = JSON.parse(lake.tiles_json(lastDate.getUTCMonth() + 1, lastDate.getUTCDate(), 12));
  assert.ok(T.isLatest && T.now && T.now.v === j.v[j.v.length - 1], "latest-reading tile");
  assert.ok(T.yoy != null && T.total != null && T.outlook, "tiles");
  const C = JSON.parse(lake.compare_json(last - 365, last));
  assert.ok(C.a && C.b && Math.abs(C.diff - (C.b.v - C.a.v)) < 1e-9, "two-date comparison");
  assert.equal(lake.level_on(last + 1).length, 0);
  assert.equal(lake.csv().trim().split("\n").length, j.d.length + 1);

  // ---- another station, merged from CSV pages like the page does
  const ld = new StationLoader(-110.74);
  ld.add_daily_mean_csv("x,y,DATE,LEVEL\n0,0,2020-01-01,1\n0,0,2020-01-03,3\n");
  ld.add_realtime_csv("x,y,DATETIME,LEVEL\n0,0,2020-01-04T12:00:00Z,5\n0,0,2020-01-04T14:00:00Z,7\n");
  const L = ld.finish();
  assert.deepEqual(L.level_on(L.first() + 1), [2, 0, 0]);
  assert.deepEqual(L.level_on(L.last()), [6, 1, 1]);
  assert.equal(ld.rt_from(), L.last());
  assert.equal(ld.approved_until(), L.first() + 2);

  // "::notice::" also shows the summary on the run's page in the Actions tab
  console.log(`::notice title=Site check::OK: ${j.d.length} days to ${lastDate.toISOString().slice(0, 10)} (latest ${j.v[j.v.length - 1]} m), ` +
              `${M.rows.length} years, July 1 ${P.targetYear} typical ${P.scenarios[3].level.toFixed(3)} m, ` +
              `${stations.length} stations, Excel ${(xlsx / 1024).toFixed(0)} KB`);
} finally {
  fs.rmSync(tmp, { force: true });
}
