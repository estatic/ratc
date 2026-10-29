//! Update: download changed countries and regenerate their stations.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::Path;

use atc_navdata::apt::parse_apt_dat;
use atc_navdata::model::Airport;
use atc_navdata::procedures::{PositionDef, Procedures};
use atc_navdata::xplane::resolve_airport;

use super::{
    APT_DAT, META, Meta, PROCEDURES, Prompt, STATION, StationDir, WizardEnv, combined, fetched_on,
    mhz, stations, write_meta,
};
use crate::generate::{Choice, generate, offered_positions, write_station};
use crate::openaip::OpenAipApi;
use crate::overrides::{Overrides, orphans};
use crate::store::CountryData;

pub(super) fn startup_check<R: BufRead, W: Write>(
    env: &WizardEnv,
    api: &dyn OpenAipApi,
    p: &mut Prompt<R, W>,
) {
    let mut updated = false;
    for cc in env.store.countries() {
        match env.store.changes(api, &cc) {
            Ok(Some(c)) if c.airports + c.airspaces + c.points > 0 => {
                let q = format!(
                    "Updates available for {cc}: {} airports, {} airspaces, {} reporting points. \
                     Update now?",
                    c.airports, c.airspaces, c.points
                );
                if p.confirm(&q, true) {
                    updated |= download(env, api, &cc, p);
                }
                if p.eof() {
                    break;
                }
            }
            Ok(_) => {}
            Err(e) => {
                p.say(format!("Could not check OpenAIP for updates: {e}"));
                break;
            }
        }
    }
    if updated {
        regenerate_all(env, p);
    }
}

pub(super) fn update_all<R: BufRead, W: Write>(
    env: &WizardEnv,
    api: &dyn OpenAipApi,
    p: &mut Prompt<R, W>,
) {
    let countries = env.store.countries();
    if countries.is_empty() {
        p.say("No OpenAIP data downloaded yet; create a station first (menu 1).");
        return;
    }
    let (mut changed, mut updated) = (false, false);
    for cc in countries {
        match env.store.changes(api, &cc) {
            Ok(Some(c)) if c.airports + c.airspaces + c.points > 0 => {
                changed = true;
                p.say(format!(
                    "Updating {cc}: {} airports, {} airspaces, {} reporting points changed.",
                    c.airports, c.airspaces, c.points
                ));
                updated |= download(env, api, &cc, p);
            }
            Ok(_) => {}
            Err(e) => {
                p.say(format!("Could not check OpenAIP for updates: {e}"));
                break;
            }
        }
    }
    if updated {
        regenerate_all(env, p);
    } else if !changed {
        p.say("All OpenAIP data is up to date.");
    }
}

/// Downloads `cc` into the store; `false` when it failed (the previous data is kept).
fn download<R: BufRead, W: Write>(
    env: &WizardEnv,
    api: &dyn OpenAipApi,
    cc: &str,
    p: &mut Prompt<R, W>,
) -> bool {
    match env.store.fetch_country(api, cc, &env.now()) {
        Ok(_) => true,
        Err(e) => {
            p.say(format!(
                "Download of {cc} failed: {e}; the previous data is kept."
            ));
            false
        }
    }
}

/// Regenerates every wizard station: generation uses all stored countries, so a change in
/// one country can touch stations of another (adjacent towers, airspaces).
fn regenerate_all<R: BufRead, W: Write>(env: &WizardEnv, p: &mut Prompt<R, W>) {
    let all = combined(&env.store);
    for st in stations(&env.data_dir) {
        match &st.meta {
            None if st.dir.join(STATION).is_file() || st.dir.join(PROCEDURES).is_file() => {
                p.say(format!(
                    "Skipping {}: not made by the wizard (no {META}).",
                    st.id
                ));
            }
            Some(m) => {
                let on = fetched_on(&env.store, &m.country, &env.now());
                regenerate(env, &all, &st, m, &on, p);
            }
            None => {}
        }
    }
}

/// Regenerates one station from fresh data, keeping its position choice and overrides.
fn regenerate<R: BufRead, W: Write>(
    env: &WizardEnv,
    all: &CountryData,
    st: &StationDir,
    meta: &Meta,
    fetched_on: &str,
    p: &mut Prompt<R, W>,
) {
    let id = &st.id;
    let Some(ap) = all.airports.iter().find(|a| a.id == meta.openaip_id) else {
        p.say(format!(
            "{id}: its airfield is not in OpenAIP any more; station kept as it is."
        ));
        return;
    };
    let layout = match layout_for_update(env, id, &st.dir) {
        Ok(l) => l,
        Err(e) => {
            p.say(format!("{id}: {e}; station kept as it is."));
            return;
        }
    };
    let old: Procedures = match std::fs::read_to_string(st.dir.join(STATION))
        .map_err(|e| e.to_string())
        .and_then(|t| toml::from_str(&t).map_err(|e: toml::de::Error| e.to_string()))
    {
        Ok(o) => o,
        Err(e) => {
            p.say(format!(
                "{id}: cannot read its {STATION} ({e}); station kept as it is."
            ));
            return;
        }
    };
    let ov = match Overrides::load(&st.dir) {
        Ok(o) => o,
        Err(e) => {
            p.say(format!("{id}: {e}; station kept as it is."));
            return;
        }
    };
    let offered = offered_positions(ap);
    let kept: Vec<PositionDef> = offered
        .iter()
        .filter(|o| old.positions.iter().any(|x| x.khz == o.khz))
        .cloned()
        .collect();
    let doc = generate(ap, all, &layout, &Choice { positions: kept }, fetched_on);

    let mut lines: BTreeSet<String> = orphans(&doc.procs, &ov).into_iter().collect();
    for gone in old
        .positions
        .iter()
        .filter(|x| !offered.iter().any(|o| o.khz == x.khz))
    {
        lines.insert(format!(
            "positions: {} ({}) is not in OpenAIP any more",
            mhz(gone.khz),
            gone.callsign
        ));
    }
    for l in &lines {
        p.say(format!("{id}: {l}"));
    }
    let meta = Meta {
        openaip_id: meta.openaip_id.clone(),
        country: meta.country.clone(),
        positions_khz: doc.procs.positions.iter().map(|x| x.khz).collect(),
    };
    let written = write_station(&doc, &st.dir.join(STATION))
        .map_err(|e| e.to_string())
        .and_then(|_| write_meta(&st.dir, &meta));
    match written {
        Ok(()) => p.say(format!("Updated {id}.")),
        Err(e) => p.say(format!("{id}: could not write the station: {e}")),
    }
}

/// The X-Plane layout of an existing station: from the install, else its own `apt.dat`.
fn layout_for_update(env: &WizardEnv, id: &str, dir: &Path) -> Result<Airport, String> {
    if let Some(xp) = &env.xplane_root
        && let Ok(r) = resolve_airport(xp, id)
    {
        return r.parse(id).map_err(|e| e.to_string());
    }
    match std::fs::read_to_string(dir.join(APT_DAT)) {
        Ok(text) => parse_apt_dat(&text, id).map_err(|e| e.to_string()),
        Err(_) => Err("no X-Plane layout (install its scenery or check xplane_root)".into()),
    }
}
