//! WebSocket sessions and the speech pipeline against the real router, FakeStt/FakeTts and
//! EPKK data.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use atc_core::atis::Weather;
use atc_core::engine::LanguageMode;
use atc_core::testkit::circuit_track;
use atc_geo::LatLon;
use atc_navdata::model::Destination;
use atc_phraseology::{Lang, Wind};
use atc_proto::{ClientMsg, ServerMsg};
use atc_voice::{FakeTts, Pcm, TextToSpeech, VoiceError, VoiceId};
use common::*;

async fn ptt(c: &mut Client, freq_khz: u32) {
    c.send(ClientMsg::PttStart { freq_khz }).await;
    c.send_audio(0, vec![100; 320]).await;
    c.send_audio(1, vec![-100; 320]).await;
    c.send(ClientMsg::PttEnd).await;
}

async fn say(c: &mut Client, freq_khz: u32, text: &str) {
    c.send(ClientMsg::TextTransmission {
        freq_khz,
        text: text.into(),
    })
    .await;
}

fn is_transcript(m: &In) -> bool {
    matches!(m, In::Msg(ServerMsg::PilotTranscript { .. }))
}

fn has_taxi(msgs: &[In]) -> bool {
    atc_texts(msgs)
        .iter()
        .any(|(st, t)| st == "Kraków Ground" && t.contains("taxi to holding point"))
}

fn is_taxi_text(m: &In) -> bool {
    has_taxi(std::slice::from_ref(m))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hello_wrong_protocol_rejected() {
    let srv = start(vec![], false).await;
    for (protocol, callsign) in [(99, "SP-ABC"), (atc_proto::PROTOCOL_VERSION, "--")] {
        let mut c = Client::connect(&srv.url).await;
        c.send(ClientMsg::Hello {
            protocol,
            callsign: callsign.into(),
            aircraft_type: "C172".into(),
        })
        .await;
        assert!(
            matches!(c.recv().await, In::Msg(ServerMsg::Rejected { .. })),
            "{protocol} {callsign}"
        );
        assert_eq!(c.recv().await, In::Closed);
    }
    assert!(srv.state.hub.lock().unwrap().callsign(1).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ground_answers_on_25k_tuning_of_833_channel() {
    // Kraków Ground is published as 118.105 (8.33 kHz name); a 25 kHz radio tunes 118.100.
    let srv = start(vec![], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(118_100).await;
    say(&mut c, 118_100, TAXI_REQUEST).await;
    let got = c.recv_until(is_audio_end).await;
    let Some(In::Msg(ServerMsg::AtcText { station, text, .. })) = got.first() else {
        panic!("{got:?}")
    };
    assert_eq!(station, "Kraków Ground");
    assert!(text.contains("taxi to holding point"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn text_taxi_request_gets_text_and_audio() {
    let srv = start(vec![], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    say(&mut c, GROUND, TAXI_REQUEST).await;
    let got = c.recv_until(is_audio_end).await;

    let Some(In::Msg(ServerMsg::AtcText {
        station,
        freq_khz,
        text,
        utterance,
    })) = got.first()
    else {
        panic!("{got:?}")
    };
    assert_eq!((station.as_str(), *freq_khz), ("Kraków Ground", GROUND));
    assert!(text.contains("taxi to holding point"), "{text}");
    assert_ne!(*utterance, 0);
    assert_eq!(
        got[1],
        In::Msg(ServerMsg::AtcAudioStart {
            utterance: *utterance,
            station: "Kraków Ground".into(),
            freq_khz: GROUND,
            sample_rate: 22050,
            looping: false,
        })
    );
    let frames: Vec<_> = got[2..got.len() - 1]
        .iter()
        .map(|m| match m {
            In::Audio(f) => f,
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(frames.iter().all(|f| f.stream == *utterance));
    assert!(frames.iter().enumerate().all(|(i, f)| f.seq == i as u32));
    let samples: usize = frames.iter().map(|f| f.pcm.len()).sum();
    assert_eq!(samples, text.split_whitespace().count() * 2205);
    assert_eq!(
        got.last(),
        Some(&In::Msg(ServerMsg::AtcAudioEnd {
            utterance: *utterance
        }))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ptt_audio_goes_through_stt() {
    let srv = start(vec![transcript(TAXI_REQUEST, 0.95)], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    ptt(&mut c, GROUND).await;
    let got = c.recv_until(is_atc_text).await;
    assert_eq!(
        got[0],
        In::Msg(ServerMsg::PilotTranscript {
            freq_khz: GROUND,
            callsign: Some("SP-ABC".into()),
            text: TAXI_REQUEST.into(),
            lang: "en".into(),
        })
    );
    assert!(has_taxi(&got), "{got:?}");
    assert_eq!(*srv.stt.languages.lock().unwrap(), [Some("en".to_string())]);
}

/// The readback is transcribed with the controller's last words on the frequency in the
/// prompt, so Whisper hears "via Sierra Zulu six" rather than "FIW Sierra Zolo six".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stt_prompt_carries_last_atc_transmission() {
    let srv = start(
        vec![
            transcript(TAXI_REQUEST, 0.95),
            transcript("Sierra Bravo Charlie, wilco", 0.95),
        ],
        false,
    )
    .await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    ptt(&mut c, GROUND).await;
    let got = c.recv_until(is_audio_end).await;
    assert!(has_taxi(&got), "{got:?}");
    ptt(&mut c, GROUND).await;
    c.recv_until(is_transcript).await;
    let prompts = srv.stt.prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert!(prompts[0].starts_with("Kraków Ground"), "{prompts:?}");
    assert!(!prompts[0].contains("taxi to holding point"), "{prompts:?}");
    assert!(prompts[1].starts_with("Kraków Ground"), "{prompts:?}");
    assert!(prompts[1].contains("taxi to holding point"), "{prompts:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_and_noise_transcripts() {
    let srv = start(
        vec![
            transcript("", 0.9),
            transcript("uh", 0.1),
            transcript("mmm brr engine noise", 0.9),
        ],
        false,
    )
    .await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;

    // Empty: the transcript only.
    ptt(&mut c, GROUND).await;
    let quiet = c.recv_until(is_transcript).await;
    // Low confidence: "say again" from the station. Unintelligible but confident: the core
    // hears no callsign and asks for it. Transmissions of one client are processed in
    // order: the taxi reply comes after anything the earlier ones produced.
    ptt(&mut c, GROUND).await;
    ptt(&mut c, GROUND).await;
    say(&mut c, GROUND, TAXI_REQUEST).await;
    let rest = c.recv_until(is_taxi_text).await;

    assert!(atc_texts(&quiet).is_empty(), "{quiet:?}");
    let texts = atc_texts(&rest);
    assert_eq!(texts.len(), 3, "{rest:?}");
    assert_eq!(
        texts[..2],
        [
            (
                "Kraków Ground".to_string(),
                "Station calling Kraków Ground, say again".to_string()
            ),
            (
                "Kraków Ground".to_string(),
                "Station calling Kraków Ground, say again your callsign".to_string()
            )
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tts_failure_still_sends_text() {
    let srv = start(vec![], true).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    say(&mut c, GROUND, TAXI_REQUEST).await;
    let mut got = c.recv_until(is_atc_text).await;
    assert!(has_taxi(&got), "{got:?}");
    // The server is still alive and answers the next transmission, still without audio.
    say(&mut c, GROUND, "Kraków Ground, request taxi").await;
    got.extend(c.recv_until(is_atc_text).await);
    assert_eq!(atc_texts(&got).len(), 2);
    assert!(
        got.iter()
            .all(|m| matches!(m, In::Msg(ServerMsg::AtcText { .. }))),
        "{got:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ptt_on_unstaffed_frequency_is_silent() {
    let srv = start(vec![transcript(TAXI_REQUEST, 0.95)], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(121_500).await;
    ptt(&mut c, 121_500).await;
    let mut got = c.recv_until(is_transcript).await;
    // Back on Ground: the first AtcText is the answer to this request.
    c.send(ClientMsg::Telemetry(parked(GROUND))).await;
    say(&mut c, GROUND, TAXI_REQUEST).await;
    got.extend(c.recv_until(is_atc_text).await);
    let texts = atc_texts(&got);
    assert_eq!(texts.len(), 1, "{got:?}");
    assert!(has_taxi(&got));
    assert!(matches!(
        got[0],
        In::Msg(ServerMsg::PilotTranscript {
            freq_khz: 121_500,
            ..
        })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_client_hears_relay_and_atc() {
    let srv = start(vec![transcript(TAXI_REQUEST, 0.95)], false).await;
    let mut c1 = Client::hello(&srv.url, "SP-ABC").await;
    let mut c2 = Client::hello(&srv.url, "SP-XYZ").await;
    c1.weather_and_parked(GROUND).await;
    c2.send(ClientMsg::Telemetry(parked(GROUND))).await;
    srv.wait_listeners(GROUND, 2).await;

    // A second PTT on the same frequency while c1 transmits is blocked.
    c1.send(ClientMsg::PttStart { freq_khz: GROUND }).await;
    c1.send_audio(0, vec![7; 320]).await;
    let relay_start = c2.recv().await;
    assert_eq!(
        relay_start,
        In::Msg(ServerMsg::RelayStart {
            from: "SP-ABC".into(),
            freq_khz: GROUND,
            sample_rate: 16000,
        })
    );
    c2.send(ClientMsg::PttStart { freq_khz: GROUND }).await;
    let got = c2
        .recv_until(|m| matches!(m, In::Msg(ServerMsg::FrequencyBlocked { .. })))
        .await;
    assert_eq!(
        got.last(),
        Some(&In::Msg(ServerMsg::FrequencyBlocked { freq_khz: GROUND }))
    );
    c1.send(ClientMsg::PttEnd).await;

    let mut heard = got;
    heard.extend(c2.recv_until(is_atc_text).await);
    let audio: Vec<_> = heard
        .iter()
        .filter_map(|m| match m {
            In::Audio(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(audio.len(), 1);
    assert_eq!((audio[0].stream, &audio[0].pcm), (0, &vec![7; 320]));
    assert!(heard.contains(&In::Msg(ServerMsg::RelayEnd {
        from: "SP-ABC".into()
    })));
    assert!(has_taxi(&heard), "{heard:?}");
    // The transmitter gets its transcript and the same reply.
    let mine = c1.recv_until(is_atc_text).await;
    assert!(mine.iter().any(is_transcript));
    assert!(has_taxi(&mine));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn atis_loops_on_atis_freq() {
    let srv = start(vec![], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(ATIS).await;
    let got = c.recv_until(is_audio_end).await;
    let first = got
        .iter()
        .find_map(|m| match m {
            In::Msg(ServerMsg::AtcAudioStart {
                utterance,
                station,
                freq_khz,
                looping,
                ..
            }) => Some((*utterance, station.clone(), *freq_khz, *looping)),
            _ => None,
        })
        .expect("audio start");
    assert_eq!(
        (&first.1[..], first.2, first.3),
        ("Kraków ATIS", ATIS, true)
    );
    let texts = atc_texts(&got);
    assert!(
        texts[0].1.starts_with("Kraków information Alpha"),
        "{texts:?}"
    );

    // Tuning away and back replays the cached letter under the same utterance.
    c.send(ClientMsg::Telemetry(parked(GROUND))).await;
    c.send(ClientMsg::Telemetry(parked(ATIS))).await;
    let again = c.recv_until(is_audio_end).await;
    assert!(again.iter().any(|m| matches!(
        m,
        In::Msg(ServerMsg::AtcAudioStart { utterance, looping: true, .. }) if *utterance == first.0
    )));
}

/// ATIS on a second radio selected for listening plays while the pilot transmits on Ground.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn atis_on_a_listened_second_radio() {
    let srv = start(vec![], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    let mut f = parked(GROUND);
    f.listen_khz = vec![GROUND, ATIS];
    c.send(ClientMsg::Telemetry(f)).await;
    let got = c.recv_until(is_audio_end).await;
    assert!(
        got.iter().any(|m| matches!(
            m,
            In::Msg(ServerMsg::AtcAudioStart { freq_khz, looping: true, .. }) if *freq_khz == ATIS
        )),
        "{got:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paused_teleport_raises_no_deviation_and_still_tunes() {
    let srv = start(vec![], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    // A paused sim (teleport, slew, sim loading): the aircraft jumps onto the runway in the
    // middle of a take-off roll with no clearance - unpaused that is a deviation.
    let (ap, procs) = epkk();
    let track = circuit_track(&ap, &procs, "25", 1.0);
    let roll = track
        .iter()
        .filter(|s| s.on_ground)
        .max_by(|a, b| a.gs_kt.total_cmp(&b.gs_kt))
        .expect("take-off roll");
    for i in 0..3 {
        let mut f = parked(GROUND);
        f.t = 11.0 + i as f64;
        f.lat = roll.pos.lat;
        f.lon = roll.pos.lon;
        f.heading_deg = roll.heading_deg;
        f.gs_kt = roll.gs_kt;
        f.ias_kt = roll.gs_kt;
        f.paused = true; // unpaused, the same frames raise a runway deviation
        c.send(ClientMsg::Telemetry(f)).await;
    }
    // Tuning while paused still reaches the hub: ATIS starts. Frames are handled in order,
    // so a false deviation would arrive before it.
    let mut f = parked(ATIS);
    f.t = 500.0;
    f.paused = true;
    c.send(ClientMsg::Telemetry(f)).await;
    let got = c.recv_until(is_audio_end).await;
    assert!(
        got.iter()
            .any(|m| matches!(m, In::Msg(ServerMsg::AtcAudioStart { looping: true, .. })))
    );
    let texts = atc_texts(&got);
    assert!(!texts.is_empty());
    assert!(
        texts.iter().all(|(st, _)| st == "Kraków ATIS"),
        "unexpected ATC text while paused: {texts:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn callsign_variants_are_one_callsign() {
    let srv = start(vec![], false).await;
    let _first = Client::hello(&srv.url, "SP-ABC").await;
    let mut c = Client::connect(&srv.url).await;
    c.send(ClientMsg::Hello {
        protocol: atc_proto::PROTOCOL_VERSION,
        callsign: "spabc".into(),
        aircraft_type: "C172".into(),
    })
    .await;
    assert!(matches!(
        c.recv().await,
        In::Msg(ServerMsg::Rejected { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_frames_skip_listeners_that_retuned() {
    let srv = start(vec![], false).await;
    let mut c1 = Client::hello(&srv.url, "SP-ABC").await;
    let mut c2 = Client::hello(&srv.url, "SP-XYZ").await;
    c1.weather_and_parked(GROUND).await;
    c2.send(ClientMsg::Telemetry(parked(GROUND))).await;
    srv.wait_listeners(GROUND, 2).await;

    c1.send(ClientMsg::PttStart { freq_khz: GROUND }).await;
    c1.send_audio(0, vec![1; 320]).await;
    let start = c2.recv().await;
    assert!(matches!(start, In::Msg(ServerMsg::RelayStart { .. })));
    let first = c2.recv().await;
    assert!(matches!(first, In::Audio(_)), "{first:?}");

    c2.send(ClientMsg::Telemetry(parked(TOWER))).await;
    srv.wait_listeners(GROUND, 1).await;
    c1.send_audio(1, vec![2; 320]).await;
    c1.send(ClientMsg::PttEnd).await;

    let rest = c2
        .recv_until(|m| matches!(m, In::Msg(ServerMsg::RelayEnd { .. })))
        .await;
    assert!(
        rest.iter().all(|m| !matches!(m, In::Audio(_))),
        "retuned listener got relay audio: {rest:?}"
    );
}

/// TTS that blocks inside `synthesize` until opened.
struct GatedTts {
    open: Mutex<bool>,
    cv: Condvar,
    entered: AtomicUsize,
}

impl TextToSpeech for GatedTts {
    fn synthesize(&self, voice: VoiceId, lang: &str, text: &str) -> Result<Pcm, VoiceError> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.cv.wait(open).unwrap();
        }
        FakeTts::default().synthesize(voice, lang, text)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_atis_synthesis_is_dropped() {
    let tts = Arc::new(GatedTts {
        open: Mutex::new(false),
        cv: Condvar::new(),
        entered: AtomicUsize::new(0),
    });
    let srv = start_with_tts(vec![], tts.clone()).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(ATIS).await;
    // Alpha is being synthesized (blocked); the weather moves on to Bravo meanwhile.
    while tts.entered.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    {
        let mut engine = srv.state.network.stations()[0].engine.lock().unwrap();
        engine.on_weather(Weather {
            wind: Wind {
                dir_deg: 100,
                speed_kt: 20,
                calm: false,
            },
            visibility_m: 10_000,
            clouds: vec![],
            temp_c: 15,
            dewpoint_c: 10,
            qnh_hpa: 990,
        });
        assert_eq!(engine.atis_broadcast().unwrap().0, 'B');
    }
    *tts.open.lock().unwrap() = true;
    tts.cv.notify_all();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Tuning away and back plays the current letter; Alpha was neither cached nor sent.
    c.send(ClientMsg::Telemetry(parked(GROUND))).await;
    c.send(ClientMsg::Telemetry(parked(ATIS))).await;
    let got = c.recv_until(is_audio_end).await;
    let texts = atc_texts(&got);
    assert_eq!(texts.len(), 1, "{got:?}");
    assert!(texts[0].1.contains("information Bravo"), "{texts:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_origin_upgrade_is_refused() {
    use tokio_tungstenite::tungstenite::{Error, client::IntoClientRequest};
    let srv = start(vec![], false).await;
    let mut req = srv.url.as_str().into_client_request().unwrap();
    req.headers_mut()
        .insert("Origin", "http://evil.example".parse().unwrap());
    match tokio_tungstenite::connect_async(req).await {
        Err(Error::Http(resp)) => assert_eq!(resp.status(), 403),
        other => panic!("expected HTTP 403, got {:?}", other.map(|_| ())),
    }
}

/// The follow-me car goes to the client of the aircraft that asked for it, before Ground's
/// "on the way".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follow_me_order_goes_to_the_aircraft() {
    let srv = start(vec![], false).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    say(&mut c, GROUND, TAXI_REQUEST).await;
    assert!(has_taxi(&c.recv_until(is_audio_end).await));
    say(
        &mut c,
        GROUND,
        "Sierra Papa Alpha Bravo Charlie, request follow-me",
    )
    .await;
    let got = c.recv_until(is_audio_end).await;
    let Some(In::Msg(ServerMsg::FollowMe { id, path, .. })) = got.first() else {
        panic!("{got:?}")
    };
    assert_eq!(*id, 1);
    assert!(path.len() >= 2, "{path:?}");
    let texts = atc_texts(&got);
    assert_eq!(texts.len(), 1, "{texts:?}");
    assert!(
        texts[0]
            .1
            .ends_with("hold position, follow-me car is on the way"),
        "{texts:?}"
    );
}

const PL_TAXI: &str =
    "Kraków Ground, Sierra Papa Alfa Bravo Charlie, informacja Alfa, proszę o kołowanie do kręgów";

fn has_polish_taxi(msgs: &[In]) -> bool {
    atc_texts(msgs)
        .iter()
        .any(|(st, t)| st == "Kraków Ground" && t.contains("kołuj do punktu oczekiwania"))
}

/// Auto: in a Polish session Whisper tags an accented call with a third language; the audio
/// is transcribed again with Polish forced, and that transcript is handled as Polish.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn third_language_is_transcribed_again() {
    let (srv, tts) = start_in(
        vec![
            transcript_in(PL_TAXI, "pl", 0.95),
            transcript_in("sierra bravo charlie, blah blah", "ru", 0.9),
            // The re-transcription (FakeStt reports the forced language, like Whisper).
            transcript_in("Sierra Bravo Charlie, blah blah", "ru", 0.95),
        ],
        LanguageMode::Auto,
    )
    .await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    ptt(&mut c, GROUND).await;
    assert!(has_polish_taxi(&c.recv_until(is_audio_end).await));
    ptt(&mut c, GROUND).await;
    let got = c.recv_until(is_audio_end).await;
    assert_eq!(
        got[0],
        In::Msg(ServerMsg::PilotTranscript {
            freq_khz: GROUND,
            callsign: Some("SP-ABC".into()),
            text: "Sierra Bravo Charlie, blah blah".into(),
            lang: "pl".into(),
        })
    );
    // Detect, detect (`ru`), then force the session language.
    assert_eq!(
        *srv.stt.languages.lock().unwrap(),
        [None, None, Some("pl".to_string())]
    );
    let prompts = srv.stt.prompts.lock().unwrap().clone();
    assert!(prompts[0].starts_with("Kraków Ground"), "{prompts:?}");
    assert!(
        prompts[1..]
            .iter()
            .all(|p| p.starts_with("Kraków, lotnisko")),
        "{prompts:?}"
    );
    let texts = atc_texts(&got);
    assert_eq!(texts.len(), 1, "{texts:?}");
    assert!(texts[0].1.contains("powtórz"), "{texts:?}");
    let strip_lang = srv.state.network.stations()[0]
        .engine
        .lock()
        .unwrap()
        .strip(&atc_phraseology::Callsign::new("SP-ABC"))
        .unwrap()
        .lang;
    assert_eq!(strip_lang, Lang::Pl);
    assert_eq!(
        *tts.calls.lock().unwrap(),
        [
            (VoiceId::Ground, "pl".to_string()),
            (VoiceId::Ground, "pl".to_string())
        ]
    );
}

/// `language = "pl"`: Whisper is told the language, the prompt is Polish and the reply is
/// spoken by the Polish Ground voice.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixed_language_is_passed_to_whisper() {
    let (srv, tts) = start_in(
        vec![transcript_in(PL_TAXI, "pl", 0.95)],
        LanguageMode::Fixed(Lang::Pl),
    )
    .await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(GROUND).await;
    ptt(&mut c, GROUND).await;
    let got = c.recv_until(is_audio_end).await;
    assert!(has_polish_taxi(&got), "{got:?}");
    assert_eq!(*srv.stt.languages.lock().unwrap(), [Some("pl".to_string())]);
    let prompts = srv.stt.prompts.lock().unwrap().clone();
    assert!(prompts[0].starts_with("Kraków, lotnisko"), "{prompts:?}");
    assert_eq!(
        *tts.calls.lock().unwrap(),
        [(VoiceId::Ground, "pl".to_string())]
    );
}

/// The ATIS is spoken by the Polish ATIS voice when the engine broadcasts it in Polish.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn polish_atis_uses_polish_voice() {
    let (srv, tts) = start_in(vec![], LanguageMode::Fixed(Lang::Pl)).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(ATIS).await;
    c.recv_until(is_audio_end).await;
    assert_eq!(
        *tts.calls.lock().unwrap(),
        [(VoiceId::Atis, "pl".to_string())]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ifr_request_without_cifp_is_unable_and_vfr_still_works() {
    let katowice = Destination {
        icao: "EPKT".into(),
        name: "Katowice".into(),
        pos: LatLon {
            lat: 50.474,
            lon: 19.08,
        },
        elevation_ft: Some(995),
    };
    let srv = start_without_cifp(vec![katowice]).await;
    let mut c = Client::hello(&srv.url, "SP-ABC").await;
    c.weather_and_parked(DELIVERY).await;

    say(
        &mut c,
        DELIVERY,
        "Kraków Delivery, SP-ABC, IFR to Katowice, request clearance",
    )
    .await;
    let got = c.recv_until(is_audio_end).await;
    let texts = atc_texts(&got);
    assert!(
        texts
            .iter()
            .any(|(st, t)| st == "Kraków Delivery" && t.contains("IFR not available, VFR only")),
        "{texts:?}"
    );

    say(
        &mut c,
        DELIVERY,
        "Kraków Delivery, SP-ABC, VFR to Katowice, request clearance",
    )
    .await;
    let got = c.recv_until(is_audio_end).await;
    let texts = atc_texts(&got);
    assert!(
        texts
            .iter()
            .any(|(st, t)| st == "Kraków Delivery" && t.contains("cleared to Katowice")),
        "{texts:?}"
    );
}

/// Two pilots at two airports: each is answered by its own station on its own frequency and
/// hears nothing of the other's exchange.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_clients_two_stations() {
    let zeta = r#"
[[positions]]
role = "Ground"
callsign = "Zeta Ground"
khz = 121700
"#;
    let srv = start_network(
        vec![fixture_station("ZZZY", ""), fixture_station("ZZZZ", zeta)],
        vec![],
        Arc::new(FakeTts::default()),
    )
    .await;
    let mut a = Client::hello(&srv.url, "SP-AAA").await;
    let mut b = Client::hello(&srv.url, "SP-BBB").await;
    a.send(ClientMsg::Weather(weather())).await;
    a.send(ClientMsg::Telemetry(parked_at(121_700, 10.0012, 10.0)))
        .await;
    b.send(ClientMsg::Telemetry(parked_at(118_500, 10.5, 10.0)))
        .await;
    srv.wait_listeners(121_700, 1).await;
    srv.wait_listeners(118_500, 1).await;
    say(&mut a, 121_700, "Zeta Ground, SP-AAA, request taxi").await;
    let got = atc_texts(&a.recv_until(is_audio_end).await);
    assert!(
        !got.is_empty() && got.iter().all(|(st, _)| st == "Zeta Ground"),
        "{got:?}"
    );
    say(
        &mut b,
        118_500,
        "Testville Tower, SP-BBB, request radio check",
    )
    .await;
    let got = b.recv_until(is_audio_end).await;
    let texts = atc_texts(&got);
    assert!(
        !texts.is_empty() && texts.iter().all(|(st, _)| st == "Testville Tower"),
        "{got:?}"
    );
    use atc_server::network::Owner;
    let net = &srv.state.network;
    assert_eq!(
        net.owner(&atc_phraseology::Callsign::new("SP-AAA")),
        Some(Owner::Station(1))
    );
    assert_eq!(
        net.owner(&atc_phraseology::Callsign::new("SP-BBB")),
        Some(Owner::Station(0))
    );
}
