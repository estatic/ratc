//! Types produced by the pilot-speech parser and consumed by the ATC core.

use crate::normalize::{Token, normalize};

/// The flight rules of a clearance request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FlightRules {
    Ifr,
    Vfr,
}

/// Normalized aircraft callsign: uppercase alphanumerics, no dashes or
/// spaces (`"SP-ABC"` is stored as `"SPABC"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Callsign(pub String);

impl Callsign {
    /// Builds a callsign, normalizing `raw` (drops everything that is not
    /// alphanumeric, uppercases).
    pub fn new(raw: &str) -> Self {
        Callsign(
            raw.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_uppercase)
                .collect(),
        )
    }

    /// ICAO abbreviated form: first character plus the last two
    /// (`"SPABC"` -> `"SBC"`). Callsigns of 3 characters or fewer are
    /// returned unchanged.
    pub fn abbreviated(&self) -> String {
        let chars: Vec<char> = self.0.chars().collect();
        if chars.len() <= 3 {
            return self.0.clone();
        }
        let mut s = String::new();
        s.push(chars[0]);
        s.extend(&chars[chars.len() - 2..]);
        s
    }

    /// True when `heard` is this callsign in full or abbreviated form.
    ///
    /// Accepted input: any spelling of the callsign letters/digits, with or
    /// without spaces or dashes, in any case (`"SPABC"`, `"sp-abc"`,
    /// `"S B C"`). Phonetic words (`"Sierra Bravo Charlie"`) are also
    /// resolved here, but the text must contain only the callsign (no
    /// addressee or aircraft-type words); `PilotMessage::callsign` is
    /// already in this form.
    pub fn matches(&self, heard: &str) -> bool {
        let mut s = String::new();
        for t in normalize(heard) {
            match t {
                Token::Letter(c) => s.push(c),
                Token::Number(d) => s.push_str(&d),
                Token::Break => {}
                Token::Word(w) => s.extend(w.chars().flat_map(char::to_uppercase)),
            }
        }
        !s.is_empty() && (s == self.0 || s == self.abbreviated())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    En,
    Pl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CircuitLeg {
    Upwind,
    Crosswind,
    Downwind,
    Base,
    Final,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PilotIntent {
    RequestTaxi {
        atis: Option<char>,
        vfr_circuits: bool,
    },
    ReadyForDeparture {
        runway: Option<String>,
    },
    ReportLeg(CircuitLeg),
    RequestFullStop,
    RequestTaxiToStand {
        stand: Option<String>,
    },
    /// Lost or unsure of the way ("request progressive taxi"): the route again.
    RequestTaxiHelp,
    /// "Say again" / "repeat": the last transmission again.
    SayAgain,
    /// "Request follow-me" / "request marshaller": a car to lead the way.
    RequestFollowMe,
    /// "On stand": parked at the end of the taxi-in.
    ReportOnStand,
    /// IFR (or VFR) clearance request: "IFR to Katowice, request clearance".
    RequestClearance {
        /// ICAO code of the destination, when understood.
        destination: Option<String>,
        /// The flight rules the pilot named; `None` when he said neither IFR nor VFR.
        rules: Option<FlightRules>,
        /// Requested level in feet (a flight level counts as hundreds of feet).
        level_ft: Option<i32>,
    },
    /// "Request departure runway zero seven": another runway than the one in use.
    RequestDepartureRunway {
        runway: String,
    },
    /// "Request start-up" / "request push-back and start-up" (`push` when push-back is asked).
    RequestStartUp {
        push: bool,
    },
    /// First call to Approach after take-off ("passing two thousand five hundred climbing five thousand").
    ReportDeparture,
    /// First call inbound, IFR or VFR ("inbound, at Zulu").
    ReportInbound {
        point: Option<String>,
    },
    /// "Request vectors" / "request ILS approach" / "request full procedure".
    RequestApproach {
        kind: Option<ApproachKind>,
        full_procedure: bool,
    },
    /// "Established ILS runway two five".
    ReportEstablished,
    /// VFR: "request joining" (at a reporting point).
    RequestJoin {
        point: Option<String>,
    },
    /// VFR: "request leaving the zone via Kilo".
    RequestLeave {
        point: Option<String>,
    },
    /// The message repeats instructions (carries clearance items) and does
    /// not match any request.
    Readback,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClearanceKind {
    Taxi,
    LineUp,
    Takeoff,
    Land,
    TouchAndGo,
    Cross,
    HoldPosition,
    GoAround,
    Contact,
}

/// Items heard in a transmission. Every list keeps the order of first
/// appearance.
///
/// Counting rule: `runway X` that is part of `hold short [of] runway X` is
/// reported in `hold_short` only. `runways` additionally holds X only if
/// `runway X` is also mentioned separately in the message. `runways` and
/// `hold_short` contain each value once. Runways are two-digit strings with
/// an optional `L`/`R`/`C` suffix; taxiways are uppercase letters with an
/// optional digit (`"A"`, `"B2"`); `squawk` is the numeric value of the code
/// (leading zeros are not preserved, compare numerically; renderers pad it to 4 digits); `kinds` contains each kind once.
///
/// Procedures: `sids` and `stars` hold `(FIX, number, letter)` with the fix uppercase. A
/// designator said without "departure" or "arrival" is listed in `sids` (the words alone do not
/// tell them apart; the readback check accepts either list). `flight_levels` holds the number
/// of each "flight level N" / "FL N". `destination` is an ICAO code (a spelled code or a
/// place name resolved through [`crate::parse::Vocabulary::places`]); `points` holds VFR
/// reporting points, uppercase.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeardItems {
    pub runways: Vec<String>,
    pub hold_short: Vec<String>,
    pub taxiways: Vec<String>,
    pub altitudes_ft: Vec<i32>,
    pub headings: Vec<u16>,
    pub freqs_khz: Vec<u32>,
    pub qnh: Vec<u16>,
    pub squawk: Vec<u16>,
    pub kinds: Vec<ClearanceKind>,
    pub flight_levels: Vec<u16>,
    pub sids: Vec<(String, u8, char)>,
    pub stars: Vec<(String, u8, char)>,
    pub directs: Vec<String>,
    pub approaches: Vec<(ApproachKind, String)>,
    pub destination: Option<String>,
    pub points: Vec<String>,
}

impl HeardItems {
    pub fn is_empty(&self) -> bool {
        *self == HeardItems::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PilotMessage {
    /// `"ground"`, `"tower"`, `"information"`, or `"krakow"` when only the
    /// place name was heard.
    pub addressee: Option<String>,
    /// The message opens with the place and the station ("Krakow Approach, ..."): a call to
    /// that station, not a readback naming it ("Approach one two one decimal ...").
    pub station_called: bool,
    /// Normalized callsign as heard (`"SPABC"`, or `"SBC"` if abbreviated).
    pub callsign: Option<String>,
    pub intent: PilotIntent,
    pub items: HeardItems,
    /// Opening greeting before the callsign ("good morning", "hello").
    pub greeting: Option<Courtesy>,
    /// The reply owed to a farewell or a wish: `GoodDay` or `ThankYouYouToo`.
    pub farewell: Option<Courtesy>,
    /// "Roger", "wilco", "copy", "copied", "understand" or "understood" was said.
    pub ack: bool,
    /// Words of acknowledgement, thanks and farewell ("roger", "thanks", "have a nice day").
    pub ack_words: Vec<String>,
    /// Words of a greeting or a hesitation ("hello", "good morning", "uh"), not in `ack_words`.
    pub greeting_words: Vec<String>,
    /// Every word in neither list, station name included; letters and digits are not words.
    pub residual: Vec<String>,
    pub raw: String,
}

/// One element of a pilot readback that ATC may ask to be repeated.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReadbackItem {
    Runway(String),
    HoldShort(String),
    Route(Vec<String>),
    Kind(ClearanceKind),
    Altitude(i32),
    Heading(u16),
    Frequency(u32),
    Qnh(u16),
    Squawk(u16),
    /// A departure procedure designator (spoken fix, number, letter).
    Sid(String, u8, char),
    /// An arrival procedure designator (spoken fix, number, letter).
    Star(String, u8, char),
    /// A cleared, assigned or maintained level.
    Level(Level),
    /// A direct-to fix.
    Direct(String),
    /// An approach type and runway.
    Approach(ApproachKind, String),
    /// The destination of an IFR clearance as an ICAO code (the engine puts the code here,
    /// not the spoken name; the parser resolves place names to ICAO).
    Destination(String),
}

/// A vertical clearance limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Level {
    /// Altitude in feet (above QNH).
    Altitude(i32),
    /// Flight level (hundreds of feet on standard pressure).
    FlightLevel(u16),
}

/// Direction of a turn onto a heading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Turn {
    Left,
    Right,
}

/// Instrument approach type as spoken (phraseology's own type; the core
/// converts from the navdata type).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ApproachKind {
    Ils,
    VorDme,
    Rnav,
}

/// Surface wind. When `calm` is true the direction and speed are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Wind {
    pub dir_deg: u16,
    pub speed_kt: u16,
    pub calm: bool,
}

/// Where a taxi clearance leads.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaxiTo {
    HoldingPoint { runway: String },
    Stand { name: String },
}

/// Where something is relative to the aircraft's nose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Ahead,
    Right,
    Behind,
    Left,
}

/// A greeting, farewell or wish.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Courtesy {
    Hello,
    GoodMorning,
    GoodAfternoon,
    GoodEvening,
    GoodDay,
    HaveANiceFlight,
    ThankYouYouToo,
}

/// Why a request cannot be granted.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UnableReason {
    /// No taxi route to the target.
    NoRoute,
    /// The requested stand is not in the airport data.
    UnknownStand { stand: String },
    /// The destination of a clearance request is unknown or was not understood.
    UnknownDestination,
    /// No IFR procedure data for this airport: VFR only.
    NoIfrData,
    /// The service is closed.
    Closed,
    /// The requested approach is not published for the runway.
    ApproachNotAvailable,
    /// VFR entry from the south is only via the southern points.
    SouthernEntry,
    /// The requested departure runway cannot be given: `runway` stays in use.
    RunwayInUse { runway: String },
}

/// One instruction or piece of information in an ATC transmission.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AtcItem {
    Taxi {
        to: TaxiTo,
        via: Vec<String>,
        crossings: Vec<String>,
    },
    HoldShort(String),
    CrossRunway(String),
    /// The departure runway the pilot asked for is approved.
    RunwayApproved(String),
    /// The flight's departure runway changed after its clearance: the new runway (a SID
    /// follows for an IFR flight).
    RunwayChanged(String),
    LineUpAndWait(String),
    ClearedTakeoff {
        runway: String,
        wind: Wind,
    },
    ClearedLand {
        runway: String,
        wind: Wind,
    },
    ClearedTouchAndGo {
        runway: String,
        wind: Wind,
    },
    Report(CircuitLeg),
    Sequence(u8),
    Qnh(u16),
    /// Transponder code to set: its four octal digits read as a decimal number
    /// (`4201` is "four two zero one").
    Squawk(u16),
    /// The transponder shows another code than the one assigned.
    CheckSquawk(u16),
    /// The transponder is off or on standby: switch it to mode C.
    SquawkCharlie,
    Contact {
        station: String,
        khz: u32,
    },
    HoldPosition,
    GiveWay {
        traffic: String,
    },
    Follow {
        traffic: String,
    },
    GoAround,
    Negative,
    ReadBack(Vec<ReadbackItem>),
    SayAgainCallsign {
        station: String,
    },
    /// "Station calling <station>, say again": a transmission too unclear to read.
    SayAgainStation {
        station: String,
    },
    /// "This is <station>": the pilot called another station on this frequency.
    ThisIs {
        station: String,
    },
    AtisCurrent(char),
    Vacate {
        via: Option<String>,
    },
    Unable(UnableReason),
    Roger,
    SayAgain,
    /// "Say flight rules": the request named neither IFR nor VFR.
    SayFlightRules,
    /// "No flight plan": Approach has no plan for an airborne flight calling it.
    NoFlightPlan,
    SayIntentions,
    SayStand,
    PassYourMessage,
    /// The information needed for the reply (weather, position) is not known yet.
    Standby,
    FollowMeOnTheWay,
    FollowTheFollowMe,
    /// Where the stand is from the aircraft, `metres` rounded to 10 m.
    StandPosition {
        stand: String,
        metres: u32,
        side: Side,
    },
    TakenOffWithoutClearance,
    LandedWithoutClearance,
    /// The aircraft entered `runway` without clearance.
    RunwayIncursion {
        runway: String,
    },
    /// The aircraft left the circuit altitude.
    CheckAltitude {
        circuit_ft: u32,
    },
    /// Still taxiing when ready for departure: "taxi to holding point, report ready".
    TaxiAndReportReady {
        runway: String,
    },
    /// The pilot read back a take-off clearance it does not have.
    NotClearedForTakeoff,
    /// The pilot read back a landing (or touch and go) clearance it does not have.
    NotClearedToLand,
    Courtesy(Courtesy),
    /// IFR clearance destination: `destination` is the spoken name ("Katowice", rendered),
    /// `icao` the code the pilot's readback is checked against.
    ClearedTo {
        destination: String,
        icao: String,
    },
    /// Departure procedure: `("SUDOL", 2, 'J')` is "SUDOL two Juliett departure".
    Sid {
        fix: String,
        number: u8,
        letter: char,
    },
    /// Arrival procedure: `("MAPIK", 1, 'G')` is "MAPIK one Golf arrival".
    Star {
        fix: String,
        number: u8,
        letter: char,
    },
    ClimbTo(Level),
    DescendTo(Level),
    MaintainLevel(Level),
    /// Heading instruction, with an optional turn direction.
    Heading {
        deg: u16,
        turn: Option<Turn>,
    },
    DirectTo {
        fix: String,
    },
    ExpectApproach {
        kind: ApproachKind,
        runway: String,
    },
    ClearedApproach {
        kind: ApproachKind,
        runway: String,
    },
    ReportEstablished,
    Identified,
    RadarServiceTerminated,
    /// The aircraft is `nm` nautical miles off the track of its cleared procedure.
    OffTrack {
        nm: u16,
    },
    /// The aircraft is off its assigned heading (magnetic).
    CheckHeading(u16),
    /// The aircraft left its cleared level.
    CheckLevel(Level),
    /// Distance to touchdown in nautical miles.
    DistanceToGo(u16),
    /// Cleared for the approach but not established on its final track by the FAF.
    CheckEstablished {
        kind: ApproachKind,
        runway: String,
    },
    StartUpApproved,
    /// Push-back and start-up approved, optionally facing a direction.
    PushAndStartApproved {
        facing: Option<String>,
    },
    ReadbackCorrect,
    /// VFR: enter the control zone via a reporting point at `alt_ft`.
    EnterControlZoneVia {
        point: String,
        alt_ft: u32,
    },
    LeaveControlZoneVia {
        point: String,
    },
    /// VFR: hold over a point, not above `max_ft`.
    HoldOver {
        point: String,
        max_ft: u32,
    },
    ReportAt {
        point: String,
    },
    VisualApproachNotAvailable,
    /// VFR: join the circuit on the downwind leg of `runway`.
    JoinDownwind {
        runway: String,
    },
    /// "Frequency change approved" (a VFR flight leaving for the next unit).
    FrequencyChangeApproved,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AtcMessage {
    pub to: Option<Callsign>,
    /// Transmitting station, e.g. `"Kraków Ground"`.
    pub from: String,
    pub items: Vec<AtcItem>,
    pub lang: Lang,
}
