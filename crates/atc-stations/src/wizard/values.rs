//! The value screen (create and edit), edit and remove.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use atc_navdata::procedures::Procedures;
use atc_navdata::station::merge_overrides;

use super::{META, OVERRIDES, Prompt, STATION, StationDir, WizardEnv, stations};
use crate::overrides::Overrides;

/// Top-level station fields in `Procedures` order, as numbered on the value screen.
const FIELDS: [&str; 17] = [
    "icao",
    "source",
    "airac",
    "transition_altitude_ft",
    "circuits",
    "preferred_runway",
    "positions",
    "vfr_points",
    "vfr_holds",
    "vfr_hold_max_ft",
    "adjacent",
    "tma",
    "delivery_hours",
    "ifr",
    "approach_alt_khz",
    "magnetic_variation_deg",
    "airspaces",
];

/// Shows every value with its origin until the user keeps them. `false` = cancelled.
pub(super) fn value_screen<R: BufRead, W: Write>(
    p: &mut Prompt<R, W>,
    id: &str,
    base: &toml::Table,
    provenance: &BTreeMap<String, String>,
    ov: &mut Overrides,
) -> bool {
    loop {
        let merged = merge_overrides(base.clone(), &ov.table);
        p.say(format!("Values of {id}:"));
        for (i, key) in FIELDS.iter().enumerate() {
            let value = merged
                .get(*key)
                .map_or_else(|| "(none)".to_string(), compact);
            let src = if ov.table.contains_key(*key) {
                let from = ov.sources.get(*key).map_or("user", String::as_str);
                format!("override: {from}")
            } else {
                provenance.get(*key).map_or("?", String::as_str).to_string()
            };
            p.say(format!("  {}. {key} = {value}   [{src}]", i + 1));
        }
        match p.menu(
            &format!("Station {id}"),
            &[
                "Keep",
                "Change a value",
                "Revert a value to generated",
                "Back without saving",
            ],
        ) {
            1 => return true,
            2 => change_value(p, base, ov),
            3 => revert_value(p, ov),
            _ => return false,
        }
    }
}

fn change_value<R: BufRead, W: Write>(
    p: &mut Prompt<R, W>,
    base: &toml::Table,
    ov: &mut Overrides,
) {
    let n = loop {
        let a = p.ask(
            &format!(
                "Number of the value to change (1-{}, Enter = back)",
                FIELDS.len()
            ),
            None,
        );
        if a.is_empty() {
            return;
        }
        match a.parse::<usize>() {
            Ok(n) if (1..=FIELDS.len()).contains(&n) => break n,
            _ => p.say(format!("Please enter a number from 1 to {}.", FIELDS.len())),
        }
    };
    let key = FIELDS[n - 1];
    loop {
        let lit = p.ask(
            &format!("New value of {key} in TOML, e.g. \"25\", 1800 or [...] (Enter = back)"),
            None,
        );
        if lit.is_empty() {
            return;
        }
        let value = match format!("{key} = {lit}").parse::<toml::Table>() {
            Ok(mut t) if t.len() == 1 => t.remove(key),
            Ok(_) => None,
            Err(e) => {
                p.say(format!("Not a valid TOML value: {}", e.message().trim()));
                continue;
            }
        };
        let Some(value) = value else {
            p.say("Not a valid TOML value: enter one value only.");
            continue;
        };
        let mut trial = ov.table.clone();
        trial.insert(key.to_string(), value.clone());
        if let Err(e) = merge_overrides(base.clone(), &trial).try_into::<Procedures>() {
            p.say(format!(
                "That value does not fit {key}: {}",
                e.message().trim()
            ));
            continue;
        }
        ov.set(key, value, "user");
        return;
    }
}

/// Drops one override, so the generated value applies again.
fn revert_value<R: BufRead, W: Write>(p: &mut Prompt<R, W>, ov: &mut Overrides) {
    let keys: Vec<String> = ov.table.keys().cloned().collect();
    if keys.is_empty() {
        p.say("No value is overridden.");
        return;
    }
    let mut items: Vec<&str> = keys.iter().map(String::as_str).collect();
    items.push("Back");
    let n = p.menu("Revert which value to generated?", &items);
    if (1..=keys.len()).contains(&n) {
        ov.remove(&keys[n - 1]);
    }
}

/// One-line rendering of a value; long lists and tables are summarised.
fn compact(v: &toml::Value) -> String {
    const MAX: usize = 60;
    let s = v.to_string();
    if s.chars().count() <= MAX {
        return s;
    }
    match v {
        toml::Value::Array(a) => format!("[{} items]", a.len()),
        toml::Value::Table(t) => format!("{{{} fields}}", t.len()),
        _ => format!("{}...", s.chars().take(MAX - 3).collect::<String>()),
    }
}

/// `# source: ...` comments of a written `station.toml`, by top-level key.
fn provenance_of(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return out;
    };
    let table = doc.as_table();
    for (key, item) in table.iter() {
        let prefix = match item {
            toml_edit::Item::Table(t) => t.decor().prefix(),
            toml_edit::Item::ArrayOfTables(a) => a.iter().next().and_then(|t| t.decor().prefix()),
            _ => table
                .get_key_value(key)
                .and_then(|(k, _)| k.leaf_decor().prefix()),
        };
        let src = prefix
            .and_then(|r| r.as_str())
            .and_then(|s| s.lines().find_map(|l| l.trim().strip_prefix("# source: ")));
        if let Some(src) = src {
            out.insert(key.to_string(), src.trim().to_string());
        }
    }
    out
}

// ---------------------------------------------------------------- edit, remove

pub(super) fn edit<R: BufRead, W: Write>(env: &WizardEnv, p: &mut Prompt<R, W>) {
    let Some(st) = pick_station(env, p, "Edit which station?") else {
        return;
    };
    let id = &st.id;
    let text = match std::fs::read_to_string(st.dir.join(STATION)) {
        Ok(t) => t,
        Err(e) => {
            p.say(format!("{id}: cannot read {STATION}: {e}"));
            return;
        }
    };
    let base: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            p.say(format!("{id}: {STATION} is not valid TOML: {e}"));
            return;
        }
    };
    let loaded = match Overrides::load(&st.dir) {
        Ok(o) => o,
        Err(e) => {
            p.say(format!("{id}: {e}"));
            return;
        }
    };
    let mut ov = loaded.clone();
    if !value_screen(p, id, &base, &provenance_of(&text), &mut ov) {
        return;
    }
    // A hand-edited overrides.toml (with its comments) is rewritten only when changed.
    if ov == loaded {
        p.say(format!("No changes to {id}."));
    } else {
        match ov.save(&st.dir) {
            Ok(()) => p.say(format!("Saved the overrides of {id}.")),
            Err(e) => p.say(format!("{id}: {e}")),
        }
    }
}

pub(super) fn remove<R: BufRead, W: Write>(env: &WizardEnv, p: &mut Prompt<R, W>) {
    let Some(st) = pick_station(env, p, "Remove which station?") else {
        return;
    };
    let id = &st.id;
    if !p.confirm(&format!("Delete {STATION} of {id}?"), false) {
        return;
    }
    for f in [STATION, META] {
        let path = st.dir.join(f);
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            p.say(format!("Could not delete {}: {e}", path.display()));
            return;
        }
    }
    p.say(format!("Deleted {STATION} of {id}."));
    let over = st.dir.join(OVERRIDES);
    if over.is_file()
        && p.confirm(
            &format!("Also delete {OVERRIDES} of {id} (your changes to the station)?"),
            false,
        )
    {
        match std::fs::remove_file(&over) {
            Ok(()) => p.say(format!("Deleted {OVERRIDES} of {id}.")),
            Err(e) => p.say(format!("Could not delete {}: {e}", over.display())),
        }
    }
}

/// Menu of the stations that have a `station.toml`; `None` when cancelled or there are none.
fn pick_station<R: BufRead, W: Write>(
    env: &WizardEnv,
    p: &mut Prompt<R, W>,
    title: &str,
) -> Option<StationDir> {
    let mut list: Vec<StationDir> = stations(&env.data_dir)
        .into_iter()
        .filter(|s| s.dir.join(STATION).is_file())
        .collect();
    if list.is_empty() {
        p.say(format!(
            "No generated stations in {}.",
            env.data_dir.display()
        ));
        return None;
    }
    let mut items: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
    items.push("Cancel");
    let n = p.menu(title, &items);
    (n >= 1 && n <= list.len()).then(|| list.swap_remove(n - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_summarises_long_lists() {
        assert_eq!(compact(&toml::Value::Integer(5)), "5");
        let long = toml::Value::Array((0..40).map(toml::Value::Integer).collect());
        assert_eq!(compact(&long), "[40 items]");
    }
}
