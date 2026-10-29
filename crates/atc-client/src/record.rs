//! `--record`: telemetry to a CSV track in the `atc-replay` `track_file` format.

use std::io::Write;

use atc_proto::TelemetryFrame;

pub const HEADER: &str = "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz,xpdr_code,xpdr_mode";

/// Writes the header, then one row per unpaused frame (paused frames repeat `t`). An
/// unknown transponder code or mode is an empty field.
/// Each row is flushed so the file survives the client being killed.
pub struct TrackWriter<W: Write> {
    out: W,
}

impl<W: Write> TrackWriter<W> {
    pub fn new(mut out: W) -> std::io::Result<Self> {
        writeln!(out, "{HEADER}")?;
        out.flush()?;
        Ok(TrackWriter { out })
    }

    pub fn write(&mut self, f: &TelemetryFrame) -> std::io::Result<()> {
        if f.paused {
            return Ok(());
        }
        let code = f.xpdr_code.map(|c| format!("{c:04}")).unwrap_or_default();
        let mode = f.xpdr_mode.map(|m| m.to_string()).unwrap_or_default();
        writeln!(
            self.out,
            "{:.2},{:.7},{:.7},{:.1},{:.1},{:.1},{:.1},{:.0},{},{},{code},{mode}",
            f.t,
            f.lat,
            f.lon,
            f.alt_ft_msl,
            f.heading_deg,
            f.ias_kt,
            f.gs_kt,
            f.vs_fpm,
            u8::from(f.on_ground),
            f.com1_khz
        )?;
        self.out.flush()
    }

    #[cfg(test)]
    pub fn into_inner(self) -> W {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: f64, paused: bool) -> TelemetryFrame {
        TelemetryFrame {
            t,
            lat: 50.076_115_3,
            lon: 19.796_116_9,
            alt_ft_msl: 791.04,
            heading_deg: 257.0,
            ias_kt: 0.0,
            gs_kt: 3.26,
            vs_fpm: -12.4,
            on_ground: true,
            com1_khz: 118_105,
            listen_khz: Vec::new(),
            xpdr_code: None,
            xpdr_mode: None,
            utc_s: None,
            day_of_year: None,
            paused,
        }
    }

    #[test]
    fn writes_header_and_unpaused_rows() {
        let mut w = TrackWriter::new(Vec::new()).unwrap();
        w.write(&frame(1.0, false)).unwrap();
        w.write(&frame(1.0, true)).unwrap();
        w.write(&frame(1.2, false)).unwrap();
        w.write(&TelemetryFrame {
            xpdr_code: Some(421),
            xpdr_mode: Some(2),
            ..frame(1.4, false)
        })
        .unwrap();
        let s = String::from_utf8(w.into_inner()).unwrap();
        assert_eq!(
            s,
            "t,lat,lon,alt_ft,hdg,ias,gs,vs,on_ground,com1_khz,xpdr_code,xpdr_mode\n\
             1.00,50.0761153,19.7961169,791.0,257.0,0.0,3.3,-12,1,118105,,\n\
             1.20,50.0761153,19.7961169,791.0,257.0,0.0,3.3,-12,1,118105,,\n\
             1.40,50.0761153,19.7961169,791.0,257.0,0.0,3.3,-12,1,118105,0421,2\n"
        );
    }
}
