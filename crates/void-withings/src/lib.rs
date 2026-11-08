//! Withings connector: body measurements, daily activity and sleep.
//!
//! Read-only. Each enabled stream becomes one conversation and every
//! measurement group, day of activity or night of sleep becomes a message, so
//! health data sits in the same inbox and search index as everything else.

pub mod api;
pub mod auth;
pub mod connector;
pub mod envelope;
pub mod error;
pub mod lock;

pub const CONNECTOR_ID: &str = "withings";

/// How far back the first sync reaches when the user keeps the default.
pub const DEFAULT_BACKFILL_DAYS: u32 = 365;

/// One of the three data streams Withings exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stream {
    Measures,
    Activity,
    Sleep,
    Workouts,
    Heart,
    Devices,
}

pub const ALL_STREAMS: [Stream; 6] = [
    Stream::Measures,
    Stream::Activity,
    Stream::Sleep,
    Stream::Workouts,
    Stream::Heart,
    Stream::Devices,
];

impl Stream {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "measures" | "measure" | "body" | "weight" => Some(Stream::Measures),
            "activity" | "activities" | "steps" => Some(Stream::Activity),
            "sleep" => Some(Stream::Sleep),
            "workouts" | "workout" | "sessions" => Some(Stream::Workouts),
            "heart" | "ecg" | "afib" => Some(Stream::Heart),
            "devices" | "device" => Some(Stream::Devices),
            _ => None,
        }
    }

    /// Stable identifier used in conversation ids and external ids.
    pub fn id(&self) -> &'static str {
        match self {
            Stream::Measures => "measures",
            Stream::Activity => "activity",
            Stream::Sleep => "sleep",
            Stream::Workouts => "workouts",
            Stream::Heart => "heart",
            Stream::Devices => "devices",
        }
    }

    /// Conversation name shown in the inbox.
    pub fn label(&self) -> &'static str {
        match self {
            Stream::Measures => "Body measurements",
            Stream::Activity => "Daily activity",
            Stream::Sleep => "Sleep",
            Stream::Workouts => "Workouts",
            Stream::Heart => "Heart & ECG",
            Stream::Devices => "Devices",
        }
    }
}

/// Parse the `streams` setting, falling back to every stream when empty.
pub fn parse_streams(values: &[String]) -> anyhow::Result<Vec<Stream>> {
    if values.is_empty() {
        return Ok(ALL_STREAMS.to_vec());
    }
    let mut streams = Vec::new();
    for value in values {
        let stream = Stream::parse(value)
            .ok_or_else(|| anyhow::anyhow!("unknown Withings stream '{value}'"))?;
        if !streams.contains(&stream) {
            streams.push(stream);
        }
    }
    streams.sort();
    Ok(streams)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_streams_means_all() {
        assert_eq!(parse_streams(&[]).unwrap(), ALL_STREAMS.to_vec());
    }

    #[test]
    fn streams_are_parsed_and_deduplicated() {
        let values = vec![
            "Sleep".to_string(),
            "steps".to_string(),
            "sleep".to_string(),
        ];
        assert_eq!(
            parse_streams(&values).unwrap(),
            vec![Stream::Activity, Stream::Sleep]
        );
    }

    #[test]
    fn unknown_stream_is_rejected() {
        let err = parse_streams(&["bloodsugar".to_string()]).unwrap_err();
        assert!(err.to_string().contains("bloodsugar"));
    }

    #[test]
    fn every_stream_has_a_distinct_id_and_label() {
        let mut ids: Vec<&str> = ALL_STREAMS.iter().map(|s| s.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL_STREAMS.len());

        let mut labels: Vec<&str> = ALL_STREAMS.iter().map(|s| s.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), ALL_STREAMS.len());

        // Every id round-trips through the parser users type.
        for stream in ALL_STREAMS {
            assert_eq!(Stream::parse(stream.id()), Some(stream));
        }
    }

    #[test]
    fn aliases_reach_the_new_streams() {
        assert_eq!(Stream::parse("ecg"), Some(Stream::Heart));
        assert_eq!(Stream::parse("Workout"), Some(Stream::Workouts));
        assert_eq!(Stream::parse("device"), Some(Stream::Devices));
    }
}
