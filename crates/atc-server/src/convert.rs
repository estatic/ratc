//! Protocol frames -> core types, and core vehicle orders -> protocol messages.

use atc_core::atis::{CloudCover, Weather};
use atc_core::engine::VehicleCommand;
use atc_core::state::{AircraftState, SimTime, TransponderMode};
use atc_geo::LatLon;
use atc_phraseology::{Callsign, Wind};
use atc_proto::{ServerMsg, TelemetryFrame, WeatherFrame};

/// The message carrying a vehicle order to the aircraft's client.
pub(crate) fn vehicle_msg(c: VehicleCommand) -> ServerMsg {
    match c {
        VehicleCommand::FollowMe {
            id,
            path,
            end_heading_deg,
        } => ServerMsg::FollowMe {
            id,
            path: path.iter().map(|p| [p.lat, p.lon]).collect(),
            end_heading_deg,
        },
        VehicleCommand::FollowMeCancel { id } => ServerMsg::FollowMeCancel { id },
    }
}

/// `"few" | "sct" | "bkn" | "ovc"` (any case); anything else is unknown.
fn cloud_cover(s: &str) -> Option<CloudCover> {
    match s.to_ascii_lowercase().as_str() {
        "few" => Some(CloudCover::Few),
        "sct" => Some(CloudCover::Sct),
        "bkn" => Some(CloudCover::Bkn),
        "ovc" => Some(CloudCover::Ovc),
        _ => None,
    }
}

/// Layers with an unknown cover are skipped. Zero wind speed is calm.
pub(crate) fn weather(f: &WeatherFrame) -> Weather {
    Weather {
        wind: Wind {
            dir_deg: f.wind_dir_deg,
            speed_kt: f.wind_speed_kt,
            calm: f.wind_speed_kt == 0,
        },
        visibility_m: f.visibility_m,
        clouds: f
            .clouds
            .iter()
            .filter_map(|l| Some((cloud_cover(&l.cover)?, l.base_ft_agl)))
            .collect(),
        temp_c: f.temp_c,
        dewpoint_c: f.dewpoint_c,
        qnh_hpa: f.qnh_hpa,
    }
}

/// An unknown transponder mode value is no mode.
pub(crate) fn aircraft_state(callsign: &Callsign, f: &TelemetryFrame) -> AircraftState {
    AircraftState {
        callsign: callsign.clone(),
        t: SimTime(f.t),
        pos: LatLon {
            lat: f.lat,
            lon: f.lon,
        },
        alt_ft_msl: f.alt_ft_msl,
        heading_deg: f.heading_deg,
        ias_kt: f.ias_kt,
        gs_kt: f.gs_kt,
        vs_fpm: f.vs_fpm,
        on_ground: f.on_ground,
        com1_khz: f.com1_khz,
        xpdr_code: f.xpdr_code,
        xpdr_mode: f.xpdr_mode.and_then(TransponderMode::from_xplane),
        utc_s: f.utc_s,
        day_of_year: f.day_of_year,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atc_proto::CloudLayer;

    fn layer(cover: &str, base: u32) -> CloudLayer {
        CloudLayer {
            cover: cover.into(),
            base_ft_agl: base,
        }
    }

    #[test]
    fn weather_maps_clouds_and_skips_unknown() {
        let f = WeatherFrame {
            wind_dir_deg: 250,
            wind_speed_kt: 8,
            visibility_m: 9000,
            clouds: vec![
                layer("few", 1500),
                layer("SCT", 2500),
                layer("bkn", 4000),
                layer("ovc", 8000),
                layer("cavok", 100),
            ],
            temp_c: -2,
            dewpoint_c: -5,
            qnh_hpa: 1021,
        };
        assert_eq!(
            weather(&f),
            Weather {
                wind: Wind {
                    dir_deg: 250,
                    speed_kt: 8,
                    calm: false
                },
                visibility_m: 9000,
                clouds: vec![
                    (CloudCover::Few, 1500),
                    (CloudCover::Sct, 2500),
                    (CloudCover::Bkn, 4000),
                    (CloudCover::Ovc, 8000),
                ],
                temp_c: -2,
                dewpoint_c: -5,
                qnh_hpa: 1021,
            }
        );
    }

    #[test]
    fn zero_wind_is_calm() {
        let f = WeatherFrame {
            wind_dir_deg: 0,
            wind_speed_kt: 0,
            visibility_m: 10_000,
            clouds: vec![],
            temp_c: 10,
            dewpoint_c: 5,
            qnh_hpa: 1013,
        };
        assert!(weather(&f).wind.calm);
    }

    #[test]
    fn telemetry_maps_every_field() {
        let f = TelemetryFrame {
            t: 12.5,
            lat: 50.07,
            lon: 19.78,
            alt_ft_msl: 791.0,
            heading_deg: 250.0,
            ias_kt: 60.0,
            gs_kt: 58.0,
            vs_fpm: 500.0,
            on_ground: false,
            com1_khz: 123_255,
            listen_khz: Vec::new(),
            xpdr_code: Some(4201),
            xpdr_mode: Some(1),
            utc_s: Some(43_200.5),
            day_of_year: Some(200),
            paused: false,
        };
        let cs = Callsign::new("SP-ABC");
        assert_eq!(
            aircraft_state(&cs, &f),
            AircraftState {
                callsign: Callsign("SPABC".into()),
                t: SimTime(12.5),
                pos: LatLon {
                    lat: 50.07,
                    lon: 19.78
                },
                alt_ft_msl: 791.0,
                heading_deg: 250.0,
                ias_kt: 60.0,
                gs_kt: 58.0,
                vs_fpm: 500.0,
                on_ground: false,
                com1_khz: 123_255,
                xpdr_code: Some(4201),
                xpdr_mode: Some(TransponderMode::Standby),
                utc_s: Some(43_200.5),
                day_of_year: Some(200),
            }
        );
        let unknown = TelemetryFrame {
            xpdr_mode: Some(9),
            ..f
        };
        assert_eq!(aircraft_state(&cs, &unknown).xpdr_mode, None);
    }
}
