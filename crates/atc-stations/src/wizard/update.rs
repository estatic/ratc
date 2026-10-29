//! Update: download changed countries and regenerate their stations.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::Path;

use atc_navdata::apt::parse_apt_dat;
use atc_navdata::model::Airport;
use atc_navdata::procedures::{PositionDef, Procedures};
use atc_navdata::xplane::resolve_airport;

use super::{
    APT_DAT, META, Meta, PROCEDURES, Prompt, STATION, StationDir, WizardEnv, combined,
    download_country, fetched_on, mhz, stations, write_meta,
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
    let (mut changed, mut updated, mut errored) = (false, false, false);
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
                errored = true;
                break;
            }
        }
    }
    if updated {
        regenerate_all(env, p);
    } else if !changed && !errored {
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
    match download_country(env, api, cc, p) {
        // The store keeps its previous copy when every list is empty.
        Ok(d) if d.airports.is_empty() && d.airspaces.is_empty() && d.points.is_empty() => {
            p.say(format!(
                "OpenAIP returned no data for {cc}; keeping the stored copy."
            ));
            false
        }
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
    let m = match_positions(&old.positions, &offered);
    let doc = generate(
        ap,
        all,
        &layout,
        &Choice {
            positions: m.kept.clone(),
        },
        fetched_on,
    );

    let gone_prefixes: Vec<String> = m
        .gone
        .iter()
        .map(|g| format!("positions: {} (", mhz(g.khz)))
        .collect();
    let mut lines: BTreeSet<String> = orphans(&old, &doc.procs, &ov)
        .into_iter()
        .filter(|l| !gone_prefixes.iter().any(|g| l.starts_with(g.as_str())))
        .collect();
    for gone in &m.gone {
        lines.insert(format!(
            "positions: {} ({}) is not in OpenAIP any more",
            mhz(gone.khz),
            gone.callsign
        ));
    }
    for (was, now) in &m.retuned {
        lines.insert(format!(
            "positions: {:?} {} -> {} (frequency changed)",
            was.role,
            mhz(was.khz),
            mhz(now.khz)
        ));
    }
    for l in &lines {
        p.say(format!("{id}: {l}"));
    }
    for n in m.unticked.iter().filter(|o| {
        meta.offered_khz
            .as_ref()
            .is_none_or(|before| !before.contains(&o.khz))
    }) {
        p.say(format!(
            "{id}: New in OpenAIP: {} {} (use Create to add it)",
            n.callsign,
            mhz(n.khz)
        ));
    }
    let meta = Meta {
        openaip_id: meta.openaip_id.clone(),
        country: meta.country.clone(),
        positions_khz: doc.procs.positions.iter().map(|x| x.khz).collect(),
        offered_khz: Some(offered.iter().map(|x| x.khz).collect()),
    };
    let written = write_station(&doc, &st.dir.join(STATION))
        .map_err(|e| e.to_string())
        .and_then(|_| write_meta(&st.dir, &meta));
    match written {
        Ok(()) => p.say(format!("Updated {id}.")),
        Err(e) => p.say(format!("{id}: could not write the station: {e}")),
    }
}

/// The old station's positions matched to the positions OpenAIP offers now.
#[derive(Debug, Default, PartialEq)]
struct Matched {
    /// Offered positions that continue a ticked one, in offered order.
    kept: Vec<PositionDef>,
    /// Old position and the offered one that continues it on another frequency.
    retuned: Vec<(PositionDef, PositionDef)>,
    /// Old positions with no counterpart.
    gone: Vec<PositionDef>,
    /// Offered positions that continue nothing.
    unticked: Vec<PositionDef>,
}

/// Matches by frequency first; else by role when the old station and the offer each have
/// exactly one position of that role; else by role and callsign.
fn match_positions(old: &[PositionDef], offered: &[PositionDef]) -> Matched {
    let mut used = vec![false; offered.len()];
    let mut pairs: Vec<Option<usize>> = old
        .iter()
        .map(|o| {
            let i = offered
                .iter()
                .enumerate()
                .position(|(i, x)| !used[i] && x.khz == o.khz)?;
            used[i] = true;
            Some(i)
        })
        .collect();
    for (k, o) in old.iter().enumerate() {
        if pairs[k].is_some() {
            continue;
        }
        let of_role = |list: &[PositionDef]| list.iter().filter(|x| x.role == o.role).count();
        let by_role = (of_role(old) == 1 && of_role(offered) == 1)
            .then(|| offered.iter().position(|x| x.role == o.role))
            .flatten()
            .filter(|&i| !used[i]);
        let by_callsign = || {
            let mut same = offered.iter().enumerate().filter(|(i, x)| {
                !used[*i] && x.role == o.role && x.callsign.eq_ignore_ascii_case(&o.callsign)
            });
            match (same.next(), same.next()) {
                (Some((i, _)), None) => Some(i),
                _ => None,
            }
        };
        if let Some(i) = by_role.or_else(by_callsign) {
            used[i] = true;
            pairs[k] = Some(i);
        }
    }
    let mut m = Matched::default();
    for (o, pair) in old.iter().zip(&pairs) {
        match pair {
            Some(i) if offered[*i].khz != o.khz => m.retuned.push((o.clone(), offered[*i].clone())),
            Some(_) => {}
            None => m.gone.push(o.clone()),
        }
    }
    for (i, x) in offered.iter().enumerate() {
        if used[i] {
            m.kept.push(x.clone());
        } else {
            m.unticked.push(x.clone());
        }
    }
    m
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

#[cfg(test)]
mod tests {
    use super::*;
    use atc_navdata::model::FreqKind;

    fn pos(role: FreqKind, callsign: &str, khz: u32) -> PositionDef {
        PositionDef {
            role,
            callsign: callsign.into(),
            khz,
        }
    }

    #[test]
    fn retuned_single_role_is_kept() {
        let old = [
            pos(FreqKind::Tower, "Zeta Tower", 118_300),
            pos(FreqKind::Delivery, "Zeta Delivery", 121_900),
        ];
        let offered = [
            pos(FreqKind::Ground, "Zeta Ground", 121_700),
            pos(FreqKind::Tower, "Zeta Tower", 118_350),
        ];
        let m = match_positions(&old, &offered);
        assert_eq!(m.kept, [offered[1].clone()]);
        assert_eq!(m.retuned, [(old[0].clone(), offered[1].clone())]);
        assert_eq!(m.gone, [old[1].clone()]);
        assert_eq!(m.unticked, [offered[0].clone()]);
    }

    #[test]
    fn several_of_a_role_match_by_callsign() {
        let old = [
            pos(FreqKind::Ground, "Zeta Ground", 121_700),
            pos(FreqKind::Ground, "Zeta Apron", 121_750),
        ];
        let offered = [
            pos(FreqKind::Ground, "Zeta Apron", 121_850),
            pos(FreqKind::Ground, "Zeta Ground", 121_700),
            pos(FreqKind::Ground, "Zeta East", 121_600),
        ];
        let m = match_positions(&old, &offered);
        assert_eq!(m.kept, [offered[0].clone(), offered[1].clone()]);
        assert_eq!(m.retuned, [(old[1].clone(), offered[0].clone())]);
        assert!(m.gone.is_empty());
        assert_eq!(m.unticked, [offered[2].clone()]);
    }
}
