//! Create: search an airfield, pick airfields around it and their positions, write stations.

use std::io::{BufRead, Write};
use std::path::Path;

use atc_geo::{M_PER_NM, distance_m};
use atc_navdata::procedures::{PositionDef, Procedures};
use atc_navdata::xplane::{ResolvedAirport, load_destinations, resolve_airport};

use super::values::value_screen;
use super::{
    APT_DAT, Meta, OVERRIDES, PROCEDURES, Prompt, STATION, WizardEnv, combined, download_country,
    fetched_on, lat_lon, mhz, write_meta,
};
use crate::generate::{
    Choice, StationDoc, generate, offered_positions, write_atomic, write_station,
};
use crate::openaip::{OaAirport, OpenAipApi, khz};
use crate::overrides::{Overrides, orphans};
use crate::store::CountryData;

const AREA_M: f64 = 100_000.0;
const MATCH_M: f64 = 300.0;
const MAX_SEARCH_RESULTS: usize = 20;
/// OpenAIP airport type of a closed aerodrome.
const CLOSED: u8 = 8;

pub(super) fn create<R: BufRead, W: Write>(
    env: &WizardEnv,
    api: &dyn OpenAipApi,
    p: &mut Prompt<R, W>,
) {
    let query = p.ask("ICAO code or name of the airfield", None);
    if query.is_empty() {
        return;
    }
    let found = match api.search_airports(&query) {
        Ok(f) => f,
        Err(e) => {
            p.say(format!("Search failed: {e}"));
            return;
        }
    };
    if found.is_empty() {
        p.say(format!("No airfield matches {query}."));
        return;
    }
    let found: Vec<OaAirport> = found.into_iter().take(MAX_SEARCH_RESULTS).collect();
    let labels: Vec<String> = found
        .iter()
        .map(|a| format!("{}  {} ({})", code(a), a.name, a.country))
        .collect();
    let mut items: Vec<&str> = labels.iter().map(String::as_str).collect();
    items.push("Cancel");
    let n = p.menu("Pick the airfield", &items);
    if n == 0 || n > found.len() {
        return;
    }
    let home = &found[n - 1];
    if home.country.trim().is_empty() {
        p.say(format!(
            "{} has no country in OpenAIP; it cannot be downloaded.",
            home.name
        ));
        return;
    }

    let mut countries = vec![home.country.to_uppercase()];
    let extra = p.ask(
        "Also load neighbouring countries (ISO codes, e.g. SK CZ; Enter = none)",
        None,
    );
    for word in extra.split(|c: char| c == ',' || c.is_whitespace()) {
        let cc = word.to_uppercase();
        if cc.is_empty() || countries.contains(&cc) {
            continue;
        }
        if cc.len() != 2 || !cc.chars().all(|c| c.is_ascii_alphabetic()) {
            p.say(format!(
                "Skipping {word}: not a two-letter ISO country code."
            ));
            continue;
        }
        countries.push(cc);
    }
    for (i, cc) in countries.iter().enumerate() {
        if let Err(e) = ensure_country(env, api, cc, p) {
            p.say(format!("Download of {cc} failed: {e}"));
            if i == 0 {
                return;
            }
        }
    }
    if p.eof() {
        return;
    }

    let all = match combined(&env.store) {
        Ok(a) => a,
        Err(e) => {
            p.say(format!("Nothing created: {e}."));
            return;
        }
    };
    let centre = lat_lon(home);
    let mut near: Vec<(f64, &OaAirport)> = all
        .airports
        .iter()
        .map(|a| (distance_m(centre, lat_lon(a)), a))
        .filter(|(d, _)| *d <= AREA_M)
        .collect();
    if !near.iter().any(|(_, a)| a.id == home.id) {
        near.push((0.0, home));
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    let items: Vec<String> = near.iter().map(|(d, a)| area_line(*d, a)).collect();
    let selectable: Vec<bool> = near.iter().map(|(_, a)| a.kind != CLOSED).collect();
    let pre: Vec<usize> = near
        .iter()
        .position(|(_, a)| a.id == home.id && a.kind != CLOSED)
        .into_iter()
        .collect();
    let picked = p.pick_some("Airfields within 100 km:", &items, &pre, &selectable);
    if p.eof() {
        return;
    }
    if picked.is_empty() {
        p.say("No airfield picked.");
    }
    for i in picked {
        create_one(env, &all, near[i].1, p);
        if p.eof() {
            return;
        }
    }
}

/// Uses the stored country, downloading it when it was never fetched.
fn ensure_country<R: BufRead, W: Write>(
    env: &WizardEnv,
    api: &dyn OpenAipApi,
    cc: &str,
    p: &mut Prompt<R, W>,
) -> Result<(), String> {
    if env
        .store
        .load_country(cc)
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }
    p.say(format!("Downloading OpenAIP data for {cc}..."));
    let d = download_country(env, api, cc, p).map_err(|e| e.to_string())?;
    if d.airports.is_empty() && d.airspaces.is_empty() && d.points.is_empty() {
        p.say(format!("OpenAIP has no data for {cc}."));
    }
    Ok(())
}

/// A station id is a directory name: 1-8 ASCII letters or digits.
fn valid_id(id: &str) -> bool {
    (1..=8).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn create_one<R: BufRead, W: Write>(
    env: &WizardEnv,
    all: &CountryData,
    ap: &OaAirport,
    p: &mut Prompt<R, W>,
) {
    let name = &ap.name;
    if ap.country.trim().is_empty() {
        p.say(format!(
            "Skipping {name}: no country in OpenAIP; nothing created."
        ));
        return;
    }
    let no_layout = format!(
        "No X-Plane layout for {name}: install its scenery or check xplane_root; nothing created."
    );
    let Some((id, resolved)) = env
        .xplane_root
        .as_deref()
        .and_then(|xp| match_layout(xp, ap))
    else {
        p.say(no_layout);
        return;
    };
    let layout = match resolved.parse(&id) {
        Ok(l) => l,
        Err(e) => {
            p.say(format!(
                "Cannot read the X-Plane layout of {name} in {}: {e}; nothing created.",
                resolved.source.display()
            ));
            return;
        }
    };
    if !valid_id(&id) {
        p.say(format!("Skipping {name}: unusable station id \"{id}\"."));
        return;
    }
    let dir = env.data_dir.join(&id);
    if dir.join(STATION).exists() {
        if !p.confirm(
            &format!("Station {id} exists. Replace its {STATION} (your overrides are kept)?"),
            false,
        ) {
            return;
        }
    } else if dir.join(PROCEDURES).is_file()
        && !p.confirm(
            &format!(
                "{id} has a hand-made {PROCEDURES}; a generated {STATION} will be used instead \
                 of it at runtime (Remove restores it). Continue?"
            ),
            false,
        )
    {
        return;
    }

    let offered = offered_positions(ap);
    let positions = if offered.is_empty() {
        p.say(format!(
            "{name} has no frequencies in OpenAIP; the station will be silent."
        ));
        Vec::new()
    } else {
        let items: Vec<String> = offered.iter().map(position_line).collect();
        let every: Vec<usize> = (0..offered.len()).collect();
        let picked = p.pick_many(
            &format!("Positions of {id} (none = silent station):"),
            &items,
            &every,
        );
        if p.eof() {
            return;
        }
        picked.into_iter().map(|i| offered[i].clone()).collect()
    };
    let doc = generate(
        ap,
        all,
        &layout,
        &Choice { positions },
        &fetched_on(&env.store, &ap.country, &env.now()),
    );
    let loaded = match Overrides::load(&dir) {
        Ok(o) => o,
        Err(e) => {
            p.say(format!("{id}: {e}; nothing created."));
            return;
        }
    };
    let base = match toml::Table::try_from(&doc.procs) {
        Ok(t) => t,
        Err(e) => {
            p.say(format!("{id}: {e}; nothing created."));
            return;
        }
    };
    let mut ov = loaded.clone();
    if !value_screen(p, &id, &base, &doc.provenance, &mut ov) {
        return;
    }
    let meta = Meta {
        openaip_id: ap.id.clone(),
        country: ap.country.to_uppercase(),
        positions_khz: doc.procs.positions.iter().map(|x| x.khz).collect(),
        offered_khz: Some(offered.iter().map(|x| x.khz).collect()),
    };
    // A hand-edited overrides.toml is rewritten only when the user changed something.
    let save_ov = (ov != loaded || !dir.join(OVERRIDES).is_file()).then_some(&ov);
    // The station being replaced, without overrides, for the orphan check.
    let previous: Option<Procedures> = std::fs::read_to_string(dir.join(STATION))
        .ok()
        .and_then(|t| toml::from_str(&t).ok());
    match write_new_station(&dir, &id, &doc, save_ov, &resolved, &meta) {
        Ok(()) => p.say(format!("Created {id} in {}.", dir.display())),
        Err(e) => {
            p.say(format!("{id}: could not write the station: {e}"));
            return;
        }
    }
    if let Some(old) = previous {
        for l in orphans(&old, &doc.procs, &ov) {
            p.say(format!("{id}: {l}"));
        }
    }
}

fn write_new_station(
    dir: &Path,
    id: &str,
    doc: &StationDoc,
    ov: Option<&Overrides>,
    resolved: &ResolvedAirport,
    meta: &Meta,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write_station(doc, &dir.join(STATION)).map_err(|e| e.to_string())?;
    if let Some(ov) = ov {
        ov.save(dir).map_err(|e| e.to_string())?;
    }
    let apt = dir.join(APT_DAT);
    if !apt.exists() {
        write_atomic(&apt, apt_dat_text(resolved, id).as_bytes()).map_err(|e| e.to_string())?;
    }
    write_meta(dir, meta)
}

/// The apt.dat block as a file `parse_apt_dat(text, id)` reads: header ident set to `id`.
fn apt_dat_text(resolved: &ResolvedAirport, id: &str) -> String {
    let mut lines = resolved.text.lines();
    let header = lines.next().unwrap_or_default();
    let mut tok: Vec<&str> = header.split_whitespace().collect();
    let header = if tok.get(4).is_some_and(|t| *t != id) {
        tok[4] = id;
        tok.join(" ")
    } else {
        header.to_string()
    };
    let rest: Vec<&str> = lines.collect();
    let mut out = format!("I\n1000 Version\n{header}\n");
    for l in rest {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str("99\n");
    out
}

/// Station id and X-Plane block: by ICAO, else the nearest airport within 300 m.
fn match_layout(xp: &Path, ap: &OaAirport) -> Option<(String, ResolvedAirport)> {
    if let Some(icao) = ap.icao_code.as_deref().filter(|s| !s.trim().is_empty())
        && let Ok(r) = resolve_airport(xp, icao)
    {
        return Some((icao.to_string(), r));
    }
    let at = lat_lon(ap);
    let near = load_destinations(xp, at, MATCH_M / M_PER_NM, &[]).ok()?;
    let nearest = near
        .iter()
        .min_by(|a, b| distance_m(at, a.pos).total_cmp(&distance_m(at, b.pos)))?;
    let r = resolve_airport(xp, &nearest.icao).ok()?;
    Some((r.ident.clone(), r))
}

fn kind_word(kind: u8) -> &'static str {
    match kind {
        0 => "airport",
        1 => "glider site",
        2 => "airfield",
        3 => "international airport",
        4 => "military heliport",
        5 => "military aerodrome",
        6 => "ultralight site",
        7 => "heliport",
        CLOSED => "aerodrome (closed)",
        9 => "IFR airfield",
        10 => "water aerodrome",
        11 => "landing strip",
        12 => "agricultural strip",
        13 => "altiport",
        _ => "aerodrome",
    }
}

fn freq_word(kind: u8) -> &'static str {
    match kind {
        0 => "APP",
        5 | 26 => "DEL",
        7 => "FIS",
        9 => "GND",
        10 => "INFO",
        12 => "UNICOM",
        13 => "RADAR",
        14 => "TWR",
        15 => "ATIS",
        16 => "RADIO",
        22 => "AFIS",
        _ => "FREQ",
    }
}

/// `  12 km  EPKC  KRAKÓW-CZYŻYNY  airfield  TWR 122.330`
fn area_line(dist_m: f64, ap: &OaAirport) -> String {
    let main = ap
        .frequencies
        .iter()
        .find(|f| f.kind == Some(14))
        .or_else(|| ap.frequencies.iter().find(|f| f.primary))
        .or_else(|| ap.frequencies.first());
    let freq = main.map_or_else(String::new, |f| {
        let v = khz(&f.value).map_or_else(|| f.value.clone(), mhz);
        format!("  {} {v}", f.kind.map_or("", freq_word))
    });
    format!(
        "{:>3} km  {}  {}  {}{freq}",
        (dist_m / 1000.0).round() as i64,
        code(ap),
        ap.name,
        kind_word(ap.kind)
    )
}

fn position_line(p: &PositionDef) -> String {
    format!(
        "{:<9} {}  {}",
        format!("{:?}", p.role),
        mhz(p.khz),
        p.callsign
    )
}

fn code(ap: &OaAirport) -> &str {
    ap.icao_code
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("—")
}

#[cfg(test)]
mod tests {
    use super::*;

    use atc_navdata::apt::parse_apt_dat;
    use std::path::PathBuf;

    #[test]
    fn station_ids_are_safe_directory_names() {
        for ok in ["EPKK", "XZ01", "K1", "ABCDEFGH"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "..", "A/B", "A\\B", "ABCDEFGHI", "EP KK", "ÉPKK", "C:"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn apt_dat_header_takes_the_station_id() {
        let r = ResolvedAirport {
            source: PathBuf::new(),
            ident: "XAAA".into(),
            text: "1 10 0 0 XAAA Alpha\n1302 icao_code AAAA\n100 30.00 1 0 0.25 1 0 1 09 10.0 \
                   10.0 0.00 0.00 3 0 0 0 27 10.0 10.02 0.00 0.00 3 0 0 0"
                .into(),
        };
        let text = apt_dat_text(&r, "AAAA");
        assert!(
            text.starts_with("I\n1000 Version\n1 10 0 0 AAAA Alpha\n"),
            "{text}"
        );
        assert!(text.ends_with("\n99\n"));
        assert_eq!(parse_apt_dat(&text, "AAAA").unwrap().runways.len(), 1);
    }
}
