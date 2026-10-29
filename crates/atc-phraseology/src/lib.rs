//! Normalizing and parsing pilot radio transmissions (as transcribed by
//! Whisper) into structured messages. Pure functions: no I/O, no global state.

pub mod normalize;
pub mod parse;
pub mod pl;
pub mod render;
pub mod types;

pub use normalize::{Token, normalize};
pub use parse::{Vocabulary, parse_pilot, parse_pilot_known, parse_pilot_with};
pub use render::{
    altitude_words, digits_words, frequency_decimals, frequency_words, number_words, render,
    runway_words, spell_char, station_name,
};
pub use types::{
    ApproachKind, AtcItem, AtcMessage, Callsign, CircuitLeg, ClearanceKind, Courtesy, FlightRules,
    HeardItems, Lang, Level, PilotIntent, PilotMessage, ReadbackItem, Side, TaxiTo, Turn,
    UnableReason, Wind,
};
