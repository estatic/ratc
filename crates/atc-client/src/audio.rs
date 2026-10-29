//! Sound card I/O with cpal: microphone → 16 kHz frames, [`Playback`] → speakers.
//! Deliberately thin (all decisions live in `capture` and `playback`); needs real hardware,
//! so it has no tests.

use std::sync::{Arc, Mutex};

use anyhow::{Context, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use tokio::sync::mpsc;

use crate::capture::{Framer, downmix};
use crate::playback::Playback;

fn device_name(d: &Device) -> String {
    d.description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "<unknown device>".into())
}

/// The device whose name contains `wanted` (case-insensitive), or the default one.
fn pick_device(wanted: Option<&str>, input: bool) -> anyhow::Result<Device> {
    let host = cpal::default_host();
    let kind = if input { "input" } else { "output" };
    let Some(wanted) = wanted else {
        let d = if input {
            host.default_input_device()
        } else {
            host.default_output_device()
        };
        return d.ok_or_else(|| anyhow!("no default audio {kind} device"));
    };
    let devices: Vec<Device> = if input {
        host.input_devices()?.collect()
    } else {
        host.output_devices()?.collect()
    };
    let needle = wanted.to_lowercase();
    if let Some(d) = devices
        .iter()
        .find(|d| device_name(d).to_lowercase().contains(&needle))
    {
        return Ok(d.clone());
    }
    let mut names: Vec<String> = devices.iter().map(device_name).collect();
    names.sort();
    names.dedup();
    bail!(
        "no audio {kind} device matching {wanted:?}; available: {}",
        names.join(", ")
    )
}

/// Starts the microphone; complete 20 ms 16 kHz frames go to `frames` (dropped when full).
/// The stream runs until dropped.
pub fn start_capture(
    wanted: Option<&str>,
    frames: mpsc::Sender<Vec<i16>>,
) -> anyhow::Result<Stream> {
    let device = pick_device(wanted, true)?;
    let supported = device
        .default_input_config()
        .with_context(|| format!("input device {}", device_name(&device)))?;
    let config = supported.config();
    tracing::info!(
        "microphone: {} ({} Hz, {} ch, {:?})",
        device_name(&device),
        config.sample_rate,
        config.channels,
        supported.sample_format()
    );
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_input::<f32>(&device, config, frames)?,
        SampleFormat::I16 => build_input::<i16>(&device, config, frames)?,
        SampleFormat::U16 => build_input::<u16>(&device, config, frames)?,
        SampleFormat::I32 => build_input::<i32>(&device, config, frames)?,
        other => bail!("unsupported microphone sample format {other:?}"),
    };
    stream.play()?;
    Ok(stream)
}

fn build_input<T>(
    device: &Device,
    config: StreamConfig,
    frames: mpsc::Sender<Vec<i16>>,
) -> anyhow::Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let mut framer = Framer::new(config.sample_rate);
    let mut buf: Vec<f32> = Vec::new();
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| {
            buf.clear();
            buf.extend(data.iter().map(|s| s.to_sample::<f32>()));
            for frame in framer.push(&downmix(&buf, channels)) {
                let _ = frames.try_send(frame);
            }
        },
        |e| tracing::error!("microphone stream error: {e}"),
        None,
    )?)
}

/// Starts playback of `Playback` (created at the device's rate). The stream runs until
/// dropped.
pub fn start_playback(wanted: Option<&str>) -> anyhow::Result<(Stream, Arc<Mutex<Playback>>)> {
    let device = pick_device(wanted, false)?;
    let supported = device
        .default_output_config()
        .with_context(|| format!("output device {}", device_name(&device)))?;
    let config = supported.config();
    tracing::info!(
        "speakers: {} ({} Hz, {} ch, {:?})",
        device_name(&device),
        config.sample_rate,
        config.channels,
        supported.sample_format()
    );
    let playback = Arc::new(Mutex::new(Playback::new(config.sample_rate)));
    let p = playback.clone();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_output::<f32>(&device, config, p)?,
        SampleFormat::I16 => build_output::<i16>(&device, config, p)?,
        SampleFormat::U16 => build_output::<u16>(&device, config, p)?,
        SampleFormat::I32 => build_output::<i32>(&device, config, p)?,
        other => bail!("unsupported speaker sample format {other:?}"),
    };
    stream.play()?;
    Ok((stream, playback))
}

fn build_output<T>(
    device: &Device,
    config: StreamConfig,
    playback: Arc<Mutex<Playback>>,
) -> anyhow::Result<Stream>
where
    T: SizedSample + FromSample<i16>,
{
    let channels = usize::from(config.channels).max(1);
    let mut mono: Vec<i16> = Vec::new();
    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            mono.resize(data.len() / channels, 0);
            if let Ok(mut p) = playback.lock() {
                p.fill(&mut mono);
            }
            for (out, &s) in data.chunks_mut(channels).zip(&mono) {
                out.fill(T::from_sample(s));
            }
        },
        |e| tracing::error!("speaker stream error: {e}"),
        None,
    )?)
}
