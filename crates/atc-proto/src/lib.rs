//! Versioned client/server protocol for the virtual ATC. No I/O.
//!
//! v1 audio is PCM s16le mono (client -> server 16 kHz, server -> client at the
//! voice sample rate, usually 22050). `sample_rate` fields and the frame magic
//! leave room for adding a codec (e.g. Opus) later.
//!
//! A message type a peer does not know fails to parse; both sides log it and go on, so
//! newer messages (such as `FollowMe`) are ignored by older clients.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

const MAGIC: &[u8; 4] = b"ATCA";
const HEADER_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProtoError {
    #[error("bad audio frame magic")]
    BadMagic,
    #[error("audio frame header too short: {0} bytes")]
    HeaderTooShort(usize),
    #[error("odd PCM byte length: {0}")]
    OddPcmLength(usize),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    pub alt_ft_msl: f64,
    pub heading_deg: f64,
    pub ias_kt: f64,
    pub gs_kt: f64,
    pub vs_fpm: f64,
    pub on_ground: bool,
    /// Frequency of the COM radio selected for transmission on the audio panel (COM1 or
    /// COM2; the name predates COM2).
    pub com1_khz: u32,
    /// Every frequency the pilot hears: the transmit radio and each other COM radio selected
    /// for listening. Empty from peers that do not send it ([`TelemetryFrame::heard_khz`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listen_khz: Vec<u32>,
    /// Transponder code as dialled (`4201`); `None` when the sim does not report it.
    #[serde(default)]
    pub xpdr_code: Option<u16>,
    /// Transponder mode as X-Plane 11 reports it: 0 off, 1 standby, 2 on, 3 alt, 4 test.
    #[serde(default)]
    pub xpdr_mode: Option<u8>,
    /// Sim UTC time of day in seconds (`sim/time/zulu_time_sec`); `None` from peers that do
    /// not send it.
    #[serde(default)]
    pub utc_s: Option<f64>,
    /// Sim day of the year, 0 = 1 January (`sim/time/local_date_days`); `None` from peers
    /// that do not send it.
    #[serde(default)]
    pub day_of_year: Option<u16>,
    pub paused: bool,
}

impl TelemetryFrame {
    /// The frequencies heard: `listen_khz` with the transmit frequency, or the transmit
    /// frequency alone when the peer sends no `listen_khz`.
    pub fn heard_khz(&self) -> Vec<u32> {
        let mut v = self.listen_khz.clone();
        if !v.contains(&self.com1_khz) {
            v.insert(0, self.com1_khz);
        }
        v
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudLayer {
    /// "few" | "sct" | "bkn" | "ovc"
    pub cover: String,
    pub base_ft_agl: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherFrame {
    pub wind_dir_deg: u16,
    pub wind_speed_kt: u16,
    pub visibility_m: u32,
    pub clouds: Vec<CloudLayer>,
    pub temp_c: i16,
    pub dewpoint_c: i16,
    pub qnh_hpa: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        protocol: u16,
        callsign: String,
        aircraft_type: String,
    },
    Telemetry(TelemetryFrame),
    Weather(WeatherFrame),
    PttStart {
        freq_khz: u32,
    },
    PttEnd,
    TextTransmission {
        freq_khz: u32,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        protocol: u16,
        session: String,
    },
    Rejected {
        reason: String,
    },
    AtcText {
        station: String,
        freq_khz: u32,
        text: String,
        utterance: u32,
    },
    AtcAudioStart {
        utterance: u32,
        station: String,
        freq_khz: u32,
        sample_rate: u32,
        looping: bool,
    },
    AtcAudioEnd {
        utterance: u32,
    },
    PilotTranscript {
        freq_khz: u32,
        callsign: Option<String>,
        text: String,
        lang: String,
    },
    FrequencyBlocked {
        freq_khz: u32,
    },
    RelayStart {
        from: String,
        freq_khz: u32,
        sample_rate: u32,
    },
    RelayEnd {
        from: String,
    },
    /// Show a follow-me car leading this client's aircraft along `path` (`[lat, lon]`
    /// degrees, the first point near the aircraft); it ends facing `end_heading_deg` (true).
    FollowMe {
        id: u32,
        path: Vec<[f64; 2]>,
        end_heading_deg: f64,
    },
    /// Remove follow-me car `id`.
    FollowMeCancel {
        id: u32,
    },
}

/// Prefix of a vehicle command on the client -> X-Plane plugin UDP channel; the rest of the
/// datagram is the `ServerMsg` as JSON. Every other datagram is an overlay text line.
pub const OVERLAY_COMMAND_PREFIX: &str = "FOLLOWME ";

/// The overlay datagram relaying `msg` to the plugin; `None` for messages it does not take.
pub fn overlay_command(msg: &ServerMsg) -> Option<String> {
    matches!(
        msg,
        ServerMsg::FollowMe { .. } | ServerMsg::FollowMeCancel { .. }
    )
    .then(|| {
        format!(
            "{OVERLAY_COMMAND_PREFIX}{}",
            serde_json::to_string(msg).expect("ServerMsg serializes")
        )
    })
}

/// The vehicle command in an overlay datagram; `None` for a text line (or a malformed
/// command).
pub fn parse_overlay_command(line: &str) -> Option<ServerMsg> {
    let json = line.strip_prefix(OVERLAY_COMMAND_PREFIX)?;
    serde_json::from_str(json).ok().filter(|m| {
        matches!(
            m,
            ServerMsg::FollowMe { .. } | ServerMsg::FollowMeCancel { .. }
        )
    })
}

/// Binary audio frame: `b"ATCA"`, stream u32 LE, seq u32 LE, then i16 LE samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    /// client→server 0 = own PTT; server→client 0 = relayed pilot audio between
    /// RelayStart/RelayEnd, otherwise the ATC utterance id.
    pub stream: u32,
    pub seq: u32,
    pub pcm: Vec<i16>,
}

impl AudioFrame {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.pcm.len() * 2);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.stream.to_le_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        for s in &self.pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<AudioFrame, ProtoError> {
        if bytes.len() < HEADER_LEN {
            return Err(ProtoError::HeaderTooShort(bytes.len()));
        }
        if &bytes[..4] != MAGIC {
            return Err(ProtoError::BadMagic);
        }
        let body = &bytes[HEADER_LEN..];
        if !body.len().is_multiple_of(2) {
            return Err(ProtoError::OddPcmLength(body.len()));
        }
        Ok(AudioFrame {
            stream: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            seq: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            pcm: body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_le_bytes(*c))
                .collect(),
        })
    }
}

/// Carrier frequency in Hz of a channel given by its name in kHz.
///
/// 8.33 kHz channel names map onto 25 kHz blocks: within a block `n`, the names
/// `n+0` (25 kHz) and `n+5` share the carrier `n`; `n+10` and `n+15` are `n + 8.33` and
/// `n + 16.67`. So "118.105" is transmitted on 118.100 MHz, which a 25 kHz radio
/// tunes as "118.100".
pub fn carrier_hz(khz: u32) -> u64 {
    let base = u64::from(khz - khz % 25) * 1000;
    base + match khz % 25 {
        10 => 8_333,
        15 => 16_667,
        _ => 0,
    }
}

/// `true` when both channel names are the same carrier, e.g. 118.100 and 118.105.
pub fn same_channel(a_khz: u32, b_khz: u32) -> bool {
    carrier_hz(a_khz) == carrier_hz(b_khz)
}

#[cfg(test)]
mod tests {
    #[test]
    fn channel_names_share_25k_carriers() {
        use super::same_channel;
        assert!(same_channel(118_105, 118_100));
        assert!(same_channel(123_255, 123_250));
        assert!(same_channel(126_130, 126_125));
        assert!(same_channel(118_105, 118_105));
        assert!(!same_channel(118_110, 118_100));
        assert!(!same_channel(118_115, 118_110));
        assert!(!same_channel(118_125, 118_100));
    }

    use super::*;

    fn tel() -> TelemetryFrame {
        TelemetryFrame {
            t: 12.5,
            lat: 55.97,
            lon: 37.41,
            alt_ft_msl: 600.0,
            heading_deg: 70.0,
            ias_kt: 0.0,
            gs_kt: 0.0,
            vs_fpm: 0.0,
            on_ground: true,
            com1_khz: 121_700,
            listen_khz: Vec::new(),
            xpdr_code: Some(4201),
            xpdr_mode: Some(2),
            utc_s: None,
            day_of_year: None,
            paused: false,
        }
    }

    /// Frames of peers that do not send the transponder still parse.
    #[test]
    fn telemetry_without_transponder() {
        let mut v = serde_json::to_value(ClientMsg::Telemetry(tel())).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("xpdr_code");
        obj.remove("xpdr_mode");
        let back: ClientMsg = serde_json::from_value(v).unwrap();
        let ClientMsg::Telemetry(f) = back else {
            panic!("{back:?}")
        };
        assert_eq!((f.xpdr_code, f.xpdr_mode), (None, None));
    }

    /// Old peers send no `listen_khz`: only the transmit frequency is heard. A listened
    /// COM2 is heard besides the transmit radio.
    #[test]
    fn heard_frequencies() {
        let v = serde_json::to_value(ClientMsg::Telemetry(tel())).unwrap();
        assert!(v.get("listen_khz").is_none(), "{v}");
        let ClientMsg::Telemetry(f) = serde_json::from_value(v).unwrap() else {
            panic!()
        };
        assert_eq!(f.heard_khz(), vec![121_700]);
        let both = TelemetryFrame {
            listen_khz: vec![126_130],
            ..tel()
        };
        assert_eq!(both.heard_khz(), vec![121_700, 126_130]);
    }

    /// Frames of peers that do not send the sim time still parse (Review Focus 4).
    #[test]
    fn telemetry_without_utc() {
        let with = TelemetryFrame {
            utc_s: Some(43_200.0),
            day_of_year: Some(200),
            ..tel()
        };
        let mut v = serde_json::to_value(ClientMsg::Telemetry(with)).unwrap();
        let back: ClientMsg = serde_json::from_value(v.clone()).unwrap();
        let ClientMsg::Telemetry(f) = back else {
            panic!("{back:?}")
        };
        assert_eq!((f.utc_s, f.day_of_year), (Some(43_200.0), Some(200)));
        let obj = v.as_object_mut().unwrap();
        obj.remove("utc_s");
        obj.remove("day_of_year");
        let back: ClientMsg = serde_json::from_value(v).unwrap();
        let ClientMsg::Telemetry(f) = back else {
            panic!("{back:?}")
        };
        assert_eq!((f.utc_s, f.day_of_year), (None, None));
    }

    #[test]
    fn client_msgs_roundtrip_json() {
        let msgs = vec![
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                callsign: "N123AB".into(),
                aircraft_type: "C172".into(),
            },
            ClientMsg::Telemetry(tel()),
            ClientMsg::Weather(WeatherFrame {
                wind_dir_deg: 270,
                wind_speed_kt: 8,
                visibility_m: 9999,
                clouds: vec![CloudLayer {
                    cover: "sct".into(),
                    base_ft_agl: 3000,
                }],
                temp_c: 12,
                dewpoint_c: -3,
                qnh_hpa: 1013,
            }),
            ClientMsg::PttStart { freq_khz: 121_700 },
            ClientMsg::PttEnd,
            ClientMsg::TextTransmission {
                freq_khz: 121_700,
                text: "ready".into(),
            },
        ];
        for m in msgs {
            let s = serde_json::to_string(&m).unwrap();
            let back: ClientMsg = serde_json::from_str(&s).unwrap();
            assert_eq!(m, back, "{s}");
        }
    }

    #[test]
    fn server_msgs_roundtrip_json() {
        let msgs = vec![
            ServerMsg::Welcome {
                protocol: 1,
                session: "s".into(),
            },
            ServerMsg::Rejected { reason: "x".into() },
            ServerMsg::AtcText {
                station: "UUEE_GND".into(),
                freq_khz: 121_700,
                text: "t".into(),
                utterance: 3,
            },
            ServerMsg::AtcAudioStart {
                utterance: 3,
                station: "g".into(),
                freq_khz: 121_700,
                sample_rate: 22050,
                looping: false,
            },
            ServerMsg::AtcAudioEnd { utterance: 3 },
            ServerMsg::PilotTranscript {
                freq_khz: 121_700,
                callsign: None,
                text: "t".into(),
                lang: "en".into(),
            },
            ServerMsg::FrequencyBlocked { freq_khz: 121_700 },
            ServerMsg::RelayStart {
                from: "N1".into(),
                freq_khz: 121_700,
                sample_rate: 16000,
            },
            ServerMsg::RelayEnd { from: "N1".into() },
            ServerMsg::FollowMe {
                id: 1,
                path: vec![[50.07, 19.78], [50.071, 19.781]],
                end_heading_deg: 257.0,
            },
            ServerMsg::FollowMeCancel { id: 1 },
        ];
        for m in msgs {
            let s = serde_json::to_string(&m).unwrap();
            assert_eq!(m, serde_json::from_str::<ServerMsg>(&s).unwrap(), "{s}");
        }
    }

    #[test]
    fn overlay_commands_roundtrip() {
        let m = ServerMsg::FollowMe {
            id: 7,
            path: vec![[50.07, 19.78]],
            end_heading_deg: 90.0,
        };
        let line = overlay_command(&m).unwrap();
        assert!(
            line.starts_with("FOLLOWME {\"type\":\"follow_me\""),
            "{line}"
        );
        assert_eq!(parse_overlay_command(&line), Some(m));
        let cancel = ServerMsg::FollowMeCancel { id: 7 };
        assert_eq!(
            parse_overlay_command(&overlay_command(&cancel).unwrap()),
            Some(cancel)
        );
        assert_eq!(
            overlay_command(&ServerMsg::AtcAudioEnd { utterance: 1 }),
            None
        );
        // Text lines and other messages are not commands.
        assert_eq!(parse_overlay_command("[Kraków Ground] FOLLOWME"), None);
        assert_eq!(
            parse_overlay_command("FOLLOWME {\"type\":\"relay_end\",\"from\":\"x\"}"),
            None
        );
        assert_eq!(parse_overlay_command("FOLLOWME nonsense"), None);
    }

    /// An older peer's view of a message type it does not know: an error, not a crash.
    #[test]
    fn unknown_message_type_fails_to_parse() {
        assert!(serde_json::from_str::<ServerMsg>("{\"type\":\"teleport\"}").is_err());
    }

    #[test]
    fn hello_json_shape() {
        let m = ClientMsg::Hello {
            protocol: 1,
            callsign: "N1".into(),
            aircraft_type: "C172".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"type":"hello","protocol":1,"callsign":"N1","aircraft_type":"C172"})
        );
        let v = serde_json::to_value(ClientMsg::PttEnd).unwrap();
        assert_eq!(v, serde_json::json!({"type":"ptt_end"}));
    }

    #[test]
    fn audio_frame_roundtrip() {
        let f = AudioFrame {
            stream: 7,
            seq: 42,
            pcm: vec![0, 1, -1, i16::MAX, i16::MIN],
        };
        let b = f.encode();
        assert_eq!(&b[..4], b"ATCA");
        assert_eq!(b.len(), 12 + 10);
        assert_eq!(AudioFrame::decode(&b).unwrap(), f);
        let empty = AudioFrame {
            stream: 0,
            seq: 0,
            pcm: vec![],
        };
        assert_eq!(AudioFrame::decode(&empty.encode()).unwrap(), empty);
    }

    #[test]
    fn audio_frame_rejects_bad_magic_and_odd_length() {
        let mut b = AudioFrame {
            stream: 1,
            seq: 1,
            pcm: vec![5, 6],
        }
        .encode();
        let mut bad = b.clone();
        bad[0] = b'X';
        assert_eq!(AudioFrame::decode(&bad), Err(ProtoError::BadMagic));
        assert_eq!(
            AudioFrame::decode(b"ATCA\0\0"),
            Err(ProtoError::HeaderTooShort(6))
        );
        b.push(0);
        assert_eq!(AudioFrame::decode(&b), Err(ProtoError::OddPcmLength(5)));
    }
}
