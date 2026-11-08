use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use crate::api::{
    afib_label, meas_label, workout_label, Activity, Device, HeartMeasure, MeasureGroup,
    SleepSummary, WithingsClient, Workout,
};
use crate::{Stream, CONNECTOR_ID};

/// Wall-clock threshold to detect hibernation gaps (same rationale as Gmail/Slack).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

/// After the first sync, activity and sleep are re-read over this window: a day
/// keeps changing until it ends, and a night lands hours after it started.
const RECHECK_WINDOW_DAYS: i64 = 7;

const STATE_LAST_POLL: &str = "withings_last_poll";
const STATE_MEAS_LASTUPDATE: &str = "withings_meas_lastupdate";

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    client: WithingsClient,
    streams: Vec<Stream>,
    backfill_days: u32,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    for stream in &streams {
        ensure_conversation(db, connection_id, *stream)?;
    }

    info!(
        connection_id,
        backfill_days, "running initial Withings sync"
    );
    if let Err(e) = sync_once(
        &client,
        db,
        connection_id,
        &streams,
        backfill_days,
        &cancel,
        true,
    )
    .await
    {
        error!(connection_id, error = %e, "initial Withings sync failed");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(poll_interval_secs.max(60)));
    // First tick fires immediately; skip it since we just did the initial sync.
    interval.tick().await;
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "Withings sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                let catching_up = elapsed > IDLE_THRESHOLD + Duration::from_secs(poll_interval_secs);
                if catching_up {
                    warn!(connection_id, idle_secs = elapsed.as_secs(), "Withings sync was idle, catching up");
                    void_core::status!(
                        "[withings:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                } else {
                    info!(connection_id, "polling Withings");
                }
                if let Err(e) = sync_once(
                    &client,
                    db,
                    connection_id,
                    &streams,
                    backfill_days,
                    &cancel,
                    catching_up,
                )
                .await
                {
                    error!(connection_id, error = %e, "Withings poll error");
                }
                last_poll = SystemTime::now();
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SyncStats {
    pub seen: u64,
    pub imported: u64,
}

pub(super) async fn sync_once(
    client: &WithingsClient,
    db: &Arc<Database>,
    connection_id: &str,
    streams: &[Stream],
    backfill_days: u32,
    cancel: &CancellationToken,
    show_progress: bool,
) -> anyhow::Result<SyncStats> {
    let now = chrono::Utc::now().timestamp();
    let first_run = db.get_sync_state(connection_id, STATE_LAST_POLL)?.is_none();
    let backfill_start = now - i64::from(backfill_days.max(1)) * 86_400;

    let mut progress = show_progress
        .then(|| BackfillProgress::new(&format!("withings:{connection_id}"), "records"));
    let mut stats = SyncStats::default();

    for stream in streams {
        if cancel.is_cancelled() {
            break;
        }
        let result = match stream {
            Stream::Measures => {
                sync_measures(
                    client,
                    db,
                    connection_id,
                    backfill_start,
                    now,
                    &mut progress,
                )
                .await
            }
            Stream::Activity => {
                sync_activity(
                    client,
                    db,
                    connection_id,
                    window_start(first_run, backfill_start, now),
                    now,
                    &mut progress,
                )
                .await
            }
            Stream::Sleep => {
                sync_sleep(
                    client,
                    db,
                    connection_id,
                    window_start(first_run, backfill_start, now),
                    now,
                    &mut progress,
                )
                .await
            }
            Stream::Workouts => {
                sync_workouts(
                    client,
                    db,
                    connection_id,
                    window_start(first_run, backfill_start, now),
                    now,
                    &mut progress,
                )
                .await
            }
            Stream::Heart => {
                sync_heart(
                    client,
                    db,
                    connection_id,
                    window_start(first_run, backfill_start, now),
                    now,
                    &mut progress,
                )
                .await
            }
            // Devices are current state, not history: there is no window to walk.
            Stream::Devices => sync_devices(client, db, connection_id, &mut progress).await,
        };

        match result {
            Ok(stream_stats) => {
                stats.seen += stream_stats.seen;
                stats.imported += stream_stats.imported;
            }
            Err(e) => {
                warn!(connection_id, stream = stream.id(), error = %e, "Withings stream sync failed");
            }
        }
    }

    if let Some(p) = progress {
        p.finish();
    }

    if !cancel.is_cancelled() {
        db.set_sync_state(connection_id, STATE_LAST_POLL, &now.to_string())?;
    }

    Ok(stats)
}

/// Backfill on the first run, then only re-read the recent window.
fn window_start(first_run: bool, backfill_start: i64, now: i64) -> i64 {
    if first_run {
        backfill_start
    } else {
        backfill_start.max(now - RECHECK_WINDOW_DAYS * 86_400)
    }
}

async fn sync_measures(
    client: &WithingsClient,
    db: &Arc<Database>,
    connection_id: &str,
    backfill_start: i64,
    now: i64,
    progress: &mut Option<BackfillProgress>,
) -> anyhow::Result<SyncStats> {
    // `lastupdate` is Withings' own incremental cursor: it returns every group
    // created *or edited* since that server timestamp.
    let lastupdate: Option<i64> = db
        .get_sync_state(connection_id, STATE_MEAS_LASTUPDATE)?
        .and_then(|v| v.parse().ok());

    let groups = match lastupdate {
        Some(since) => client.measures_since(since).await?,
        None => client.measures_between(backfill_start, now).await?,
    };

    let conv_id = conversation_id(connection_id, Stream::Measures);
    let mut stats = SyncStats::default();

    for group in &groups.measuregrps {
        stats.seen += 1;
        if let Some(p) = progress.as_mut() {
            p.inc(1);
        }
        // A group the scale could not attribute to a user may belong to someone
        // else in the household.
        if group.is_ambiguous() || group.measures.is_empty() {
            continue;
        }
        db.upsert_message(&measure_message(group, connection_id, &conv_id))?;
        stats.imported += 1;
        if let Some(p) = progress.as_mut() {
            p.inc_secondary(1);
        }
    }

    let cursor = if groups.updatetime > 0 {
        groups.updatetime
    } else {
        now
    };
    db.set_sync_state(connection_id, STATE_MEAS_LASTUPDATE, &cursor.to_string())?;

    Ok(stats)
}

async fn sync_activity(
    client: &WithingsClient,
    db: &Arc<Database>,
    connection_id: &str,
    start: i64,
    now: i64,
    progress: &mut Option<BackfillProgress>,
) -> anyhow::Result<SyncStats> {
    let activities = client.activities(&to_ymd(start), &to_ymd(now)).await?;
    let conv_id = conversation_id(connection_id, Stream::Activity);
    let mut stats = SyncStats::default();

    for activity in &activities {
        stats.seen += 1;
        if let Some(p) = progress.as_mut() {
            p.inc(1);
        }
        // A day with no steps at all is a day the watch was not worn.
        if activity.steps.unwrap_or(0) == 0 && activity.totalcalories.is_none() {
            continue;
        }
        db.upsert_message(&activity_message(activity, connection_id, &conv_id))?;
        stats.imported += 1;
        if let Some(p) = progress.as_mut() {
            p.inc_secondary(1);
        }
    }

    Ok(stats)
}

async fn sync_sleep(
    client: &WithingsClient,
    db: &Arc<Database>,
    connection_id: &str,
    start: i64,
    now: i64,
    progress: &mut Option<BackfillProgress>,
) -> anyhow::Result<SyncStats> {
    let series = client.sleep_summaries(&to_ymd(start), &to_ymd(now)).await?;
    let conv_id = conversation_id(connection_id, Stream::Sleep);
    let mut stats = SyncStats::default();

    for night in &series {
        stats.seen += 1;
        if let Some(p) = progress.as_mut() {
            p.inc(1);
        }
        db.upsert_message(&sleep_message(night, connection_id, &conv_id))?;
        stats.imported += 1;
        if let Some(p) = progress.as_mut() {
            p.inc_secondary(1);
        }
    }

    Ok(stats)
}

async fn sync_workouts(
    client: &WithingsClient,
    db: &Arc<Database>,
    connection_id: &str,
    start: i64,
    now: i64,
    progress: &mut Option<BackfillProgress>,
) -> anyhow::Result<SyncStats> {
    let workouts = client.workouts(&to_ymd(start), &to_ymd(now)).await?;
    let conv_id = conversation_id(connection_id, Stream::Workouts);
    let mut stats = SyncStats::default();

    for workout in &workouts {
        stats.seen += 1;
        if let Some(p) = progress.as_mut() {
            p.inc(1);
        }
        db.upsert_message(&workout_message(workout, connection_id, &conv_id))?;
        stats.imported += 1;
        if let Some(p) = progress.as_mut() {
            p.inc_secondary(1);
        }
    }

    Ok(stats)
}

async fn sync_heart(
    client: &WithingsClient,
    db: &Arc<Database>,
    connection_id: &str,
    start: i64,
    now: i64,
    progress: &mut Option<BackfillProgress>,
) -> anyhow::Result<SyncStats> {
    let measures = client.heart_measures(start, now).await?;
    let conv_id = conversation_id(connection_id, Stream::Heart);
    let mut stats = SyncStats::default();

    for measure in &measures {
        stats.seen += 1;
        if let Some(p) = progress.as_mut() {
            p.inc(1);
        }
        db.upsert_message(&heart_message(measure, connection_id, &conv_id))?;
        stats.imported += 1;
        if let Some(p) = progress.as_mut() {
            p.inc_secondary(1);
        }
    }

    Ok(stats)
}

async fn sync_devices(
    client: &WithingsClient,
    db: &Arc<Database>,
    connection_id: &str,
    progress: &mut Option<BackfillProgress>,
) -> anyhow::Result<SyncStats> {
    let devices = client.devices().await?;
    let conv_id = conversation_id(connection_id, Stream::Devices);
    let mut stats = SyncStats::default();

    for device in &devices {
        stats.seen += 1;
        if let Some(p) = progress.as_mut() {
            p.inc(1);
        }
        // One row per device, rewritten on each poll: the battery level and the
        // last session are state, not events.
        db.upsert_message(&device_message(device, connection_id, &conv_id))?;
        stats.imported += 1;
        if let Some(p) = progress.as_mut() {
            p.inc_secondary(1);
        }
    }

    Ok(stats)
}

fn conversation_id(connection_id: &str, stream: Stream) -> String {
    format!("{connection_id}-{}", stream.id())
}

fn conversation_external_id(connection_id: &str, stream: Stream) -> String {
    format!("withings_{connection_id}_{}", stream.id())
}

fn ensure_conversation(
    db: &Arc<Database>,
    connection_id: &str,
    stream: Stream,
) -> anyhow::Result<()> {
    let conv = Conversation {
        id: conversation_id(connection_id, stream),
        connection_id: connection_id.to_string(),
        connector: CONNECTOR_ID.to_string(),
        external_id: conversation_external_id(connection_id, stream),
        name: Some(stream.label().to_string()),
        kind: ConversationKind::Channel,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;
    Ok(())
}

fn base_message(
    id: String,
    external_id: String,
    connection_id: &str,
    conv_id: &str,
    timestamp: i64,
    body: String,
    metadata: serde_json::Value,
) -> Message {
    Message {
        id,
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: CONNECTOR_ID.to_string(),
        external_id,
        sender: "withings".to_string(),
        sender_name: Some("Withings".to_string()),
        sender_avatar_url: None,
        body: Some(body),
        timestamp,
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: Some(metadata),
        context_id: None,
        context: None,
    }
}

pub(super) fn measure_message(group: &MeasureGroup, connection_id: &str, conv_id: &str) -> Message {
    let grpid = group.grpid;
    let mut values = serde_json::Map::new();
    let mut parts = Vec::with_capacity(group.measures.len());

    for measure in &group.measures {
        let value = measure.real_value();
        match meas_label(measure.meas_type) {
            Some((label, unit)) => {
                parts.push(if unit.is_empty() {
                    format!("{label} {}", format_number(value))
                } else {
                    format!("{label} {} {unit}", format_number(value))
                });
                values.insert(
                    label.to_lowercase().replace(' ', "_"),
                    serde_json::json!(value),
                );
            }
            None => {
                values.insert(
                    format!("type_{}", measure.meas_type),
                    serde_json::json!(value),
                );
            }
        }
    }

    let body = format!(
        "{}\n{}",
        format_timestamp(group.date),
        if parts.is_empty() {
            "(no readable measurement)".to_string()
        } else {
            parts.join(" · ")
        }
    );

    let metadata = serde_json::json!({
        "stream": Stream::Measures.id(),
        "grpid": grpid,
        "attrib": group.attrib,
        "category": group.category,
        "deviceid": group.deviceid,
        "measures": values,
    });

    base_message(
        format!("{connection_id}-meas-{grpid}"),
        format!("withings_{connection_id}_meas_{grpid}"),
        connection_id,
        conv_id,
        group.date,
        body,
        metadata,
    )
}

pub(super) fn activity_message(activity: &Activity, connection_id: &str, conv_id: &str) -> Message {
    let date = activity.date.as_str();
    let mut parts = Vec::new();
    if let Some(steps) = activity.steps {
        parts.push(format!("{steps} steps"));
    }
    if let Some(distance) = activity.distance {
        parts.push(format!("{} km", format_number(distance / 1000.0)));
    }
    if let Some(calories) = activity.totalcalories.or(activity.calories) {
        parts.push(format!("{} kcal", format_number(calories)));
    }
    let active =
        activity.soft.unwrap_or(0) + activity.moderate.unwrap_or(0) + activity.intense.unwrap_or(0);
    if active > 0 {
        parts.push(format!("{} active", format_duration(active)));
    }
    if let Some(hr) = activity.hr_average {
        parts.push(format!("{hr} bpm avg"));
    }

    let body = format!(
        "{date}\n{}",
        if parts.is_empty() {
            "(no activity recorded)".to_string()
        } else {
            parts.join(" · ")
        }
    );

    let metadata = serde_json::json!({
        "stream": Stream::Activity.id(),
        "date": date,
        "steps": activity.steps,
        "distance_m": activity.distance,
        "elevation_m": activity.elevation,
        "calories": activity.calories,
        "total_calories": activity.totalcalories,
        "soft_secs": activity.soft,
        "moderate_secs": activity.moderate,
        "intense_secs": activity.intense,
        "hr_average": activity.hr_average,
        "hr_min": activity.hr_min,
        "hr_max": activity.hr_max,
    });

    base_message(
        format!("{connection_id}-activity-{date}"),
        format!("withings_{connection_id}_activity_{date}"),
        connection_id,
        conv_id,
        day_start_timestamp(date),
        body,
        metadata,
    )
}

pub(super) fn sleep_message(night: &SleepSummary, connection_id: &str, conv_id: &str) -> Message {
    let id = night.id;
    let data = &night.data;
    let mut parts = Vec::new();

    let asleep = data.total_sleep_secs();
    if asleep > 0 {
        parts.push(format!("{} asleep", format_duration(asleep)));
    }
    if let Some(score) = data.sleep_score {
        parts.push(format!("score {score}"));
    }
    if let Some(deep) = data.deepsleepduration {
        parts.push(format!("{} deep", format_duration(deep)));
    }
    if let Some(rem) = data.remsleepduration {
        parts.push(format!("{} REM", format_duration(rem)));
    }
    if let Some(awake) = data.wakeupduration {
        parts.push(format!("{} awake", format_duration(awake)));
    }
    if let Some(hr) = data.hr_average {
        parts.push(format!("{hr} bpm avg"));
    }

    let header = match night.date.as_deref() {
        Some(date) => format!("Night of {date}"),
        None => format_timestamp(night.startdate),
    };
    let body = format!(
        "{header}\n{}",
        if parts.is_empty() {
            "(no sleep data)".to_string()
        } else {
            parts.join(" · ")
        }
    );

    let metadata = serde_json::json!({
        "stream": Stream::Sleep.id(),
        "sleep_id": id,
        "date": night.date,
        "startdate": night.startdate,
        "enddate": night.enddate,
        "sleep_score": data.sleep_score,
        "light_secs": data.lightsleepduration,
        "deep_secs": data.deepsleepduration,
        "rem_secs": data.remsleepduration,
        "awake_secs": data.wakeupduration,
        "wakeup_count": data.wakeupcount,
        "hr_average": data.hr_average,
        "hr_min": data.hr_min,
        "hr_max": data.hr_max,
        "rr_average": data.rr_average,
        "snoring_secs": data.snoring,
        "breathing_disturbances": data.breathing_disturbances_intensity,
    });

    // Timestamped at wake-up: that is when the night is complete and known.
    let timestamp = if night.enddate > 0 {
        night.enddate
    } else {
        night.startdate
    };

    base_message(
        format!("{connection_id}-sleep-{id}"),
        format!("withings_{connection_id}_sleep_{id}"),
        connection_id,
        conv_id,
        timestamp,
        body,
        metadata,
    )
}

pub(super) fn workout_message(workout: &Workout, connection_id: &str, conv_id: &str) -> Message {
    let id = workout.id;
    let sport = workout_label(workout.category)
        .map(str::to_string)
        .unwrap_or_else(|| format!("Workout #{}", workout.category));
    let data = &workout.data;

    let mut parts = Vec::new();
    let duration = workout.duration_secs();
    if duration > 0 {
        parts.push(format_duration(duration));
    }
    if let Some(effective) = data.effduration.filter(|e| *e > 0 && *e != duration) {
        parts.push(format!("{} moving", format_duration(effective)));
    }
    if let Some(distance) = data.distance.or(data.manual_distance).filter(|d| *d > 0.0) {
        parts.push(format!("{} km", format_number(distance / 1000.0)));
    }
    if let Some(calories) = data.calories.or(data.manual_calories) {
        parts.push(format!("{} kcal", format_number(calories)));
    }
    if let Some(hr) = data.hr_average {
        parts.push(format!("{hr} bpm avg"));
    }
    if let Some(hr_max) = data.hr_max {
        parts.push(format!("{hr_max} bpm max"));
    }

    let body = format!(
        "{sport} — {}\n{}",
        format_timestamp(workout.startdate),
        if parts.is_empty() {
            "(no workout detail)".to_string()
        } else {
            parts.join(" · ")
        }
    );

    let metadata = serde_json::json!({
        "stream": Stream::Workouts.id(),
        "workout_id": id,
        "category": workout.category,
        "sport": sport,
        "startdate": workout.startdate,
        "enddate": workout.enddate,
        "duration_secs": duration,
        "effective_secs": data.effduration,
        "intensity": data.intensity,
        "distance_m": data.distance.or(data.manual_distance),
        "elevation_m": data.elevation,
        "steps": data.steps,
        "calories": data.calories.or(data.manual_calories),
        "hr_average": data.hr_average,
        "hr_min": data.hr_min,
        "hr_max": data.hr_max,
        "spo2_average": data.spo2_average,
        "pool_laps": data.pool_laps,
        "strokes": data.strokes,
        "deviceid": workout.deviceid,
    });

    base_message(
        format!("{connection_id}-workout-{id}"),
        format!("withings_{connection_id}_workout_{id}"),
        connection_id,
        conv_id,
        workout.startdate,
        body,
        metadata,
    )
}

pub(super) fn heart_message(measure: &HeartMeasure, connection_id: &str, conv_id: &str) -> Message {
    let key = measure.stable_id();
    let mut parts = Vec::new();

    let (header, afib) = match &measure.ecg {
        Some(ecg) => {
            let afib = ecg.afib.map(afib_label);
            if let Some(afib) = afib {
                parts.push(afib.to_string());
            }
            ("ECG recording", afib)
        }
        None => ("Heart measurement", None),
    };

    if let Some(hr) = measure.heart_rate {
        parts.push(format!("{hr} bpm"));
    }
    if let Some(bp) = &measure.bloodpressure {
        if let (Some(systole), Some(diastole)) = (bp.systole, bp.diastole) {
            parts.push(format!("{systole}/{diastole} mmHg"));
        }
    }

    let body = format!(
        "{header} — {}\n{}",
        format_timestamp(measure.timestamp),
        if parts.is_empty() {
            "(no heart detail)".to_string()
        } else {
            parts.join(" · ")
        }
    );

    let metadata = serde_json::json!({
        "stream": Stream::Heart.id(),
        "timestamp": measure.timestamp,
        "heart_rate": measure.heart_rate,
        "signalid": measure.ecg.as_ref().map(|e| e.signalid),
        "afib": measure.ecg.as_ref().and_then(|e| e.afib),
        "afib_label": afib,
        "systole": measure.bloodpressure.as_ref().and_then(|bp| bp.systole),
        "diastole": measure.bloodpressure.as_ref().and_then(|bp| bp.diastole),
        "deviceid": measure.deviceid,
        "model": measure.model,
    });

    base_message(
        format!("{connection_id}-heart-{key}"),
        format!("withings_{connection_id}_heart_{key}"),
        connection_id,
        conv_id,
        measure.timestamp,
        body,
        metadata,
    )
}

pub(super) fn device_message(device: &Device, connection_id: &str, conv_id: &str) -> Message {
    let deviceid = device.deviceid.as_str();
    let name = device
        .model
        .clone()
        .or_else(|| device.device_type.clone())
        .unwrap_or_else(|| "Withings device".to_string());

    let mut parts = Vec::new();
    if let Some(kind) = &device.device_type {
        parts.push(kind.clone());
    }
    if let Some(battery) = &device.battery {
        parts.push(format!("battery {battery}"));
    }
    if let Some(last) = device.last_session_date {
        parts.push(format!("last session {}", format_timestamp(last)));
    }

    let body = format!(
        "{name}\n{}",
        if parts.is_empty() {
            "(no device detail)".to_string()
        } else {
            parts.join(" · ")
        }
    );

    let metadata = serde_json::json!({
        "stream": Stream::Devices.id(),
        "deviceid": deviceid,
        "type": device.device_type,
        "model": device.model,
        "model_id": device.model_id,
        "battery": device.battery,
        "first_session_date": device.first_session_date,
        "last_session_date": device.last_session_date,
        "timezone": device.timezone,
    });

    base_message(
        format!("{connection_id}-device-{deviceid}"),
        format!("withings_{connection_id}_device_{deviceid}"),
        connection_id,
        conv_id,
        device
            .last_session_date
            .unwrap_or_else(|| chrono::Utc::now().timestamp()),
        body,
        metadata,
    )
}

/// Unix seconds as `YYYY-MM-DD`, the only date form the v2 endpoints take.
fn to_ymd(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d")
        .to_string()
}

/// Midnight UTC of a `YYYY-MM-DD` date, so days sort in order.
fn day_start_timestamp(date: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or_else(|| chrono::Utc::now().timestamp())
}

fn format_timestamp(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| timestamp.to_string())
}

/// Two decimals at most, and no trailing zeros: `78.4`, not `78.40`.
fn format_number(value: f64) -> String {
    let rounded = format!("{value:.2}");
    let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Seconds as `7h 42m`, `42m`, or `30s`.
fn format_duration(secs: i64) -> String {
    let hours = secs / 3_600;
    let minutes = (secs % 3_600) / 60;
    match (hours, minutes) {
        (0, 0) => format!("{secs}s"),
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{BloodPressure, Ecg, Measure, SleepData, WorkoutData};

    fn group(attrib: i64, measures: Vec<Measure>) -> MeasureGroup {
        MeasureGroup {
            grpid: 77,
            attrib,
            date: 1_757_900_000,
            created: Some(1_757_900_001),
            category: Some(1),
            deviceid: Some("dev".into()),
            measures,
        }
    }

    #[test]
    fn formats_numbers_without_trailing_zeros() {
        assert_eq!(format_number(78.40), "78.4");
        assert_eq!(format_number(70.0), "70");
        assert_eq!(format_number(6.126), "6.13");
        assert_eq!(format_number(0.0), "0");
    }

    #[test]
    fn formats_durations_by_magnitude() {
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(2_520), "42m");
        assert_eq!(format_duration(7_200), "2h");
        assert_eq!(format_duration(27_720), "7h 42m");
    }

    #[test]
    fn measure_message_renders_every_known_type() {
        let msg = measure_message(
            &group(
                0,
                vec![
                    Measure {
                        value: 78_400,
                        meas_type: 1,
                        unit: -3,
                    },
                    Measure {
                        value: 142,
                        meas_type: 6,
                        unit: -1,
                    },
                    Measure {
                        value: 62,
                        meas_type: 11,
                        unit: 0,
                    },
                ],
            ),
            "health",
            "health-measures",
        );

        assert_eq!(msg.id, "health-meas-77");
        assert_eq!(msg.external_id, "withings_health_meas_77");
        assert_eq!(msg.timestamp, 1_757_900_000);
        let body = msg.body.unwrap();
        assert!(body.contains("Weight 78.4 kg"));
        assert!(body.contains("Fat ratio 14.2 %"));
        assert!(body.contains("Heart rate 62 bpm"));
        let meta = msg.metadata.unwrap();
        assert_eq!(meta["grpid"], 77);
        assert_eq!(meta["measures"]["weight"], 78.4);
    }

    #[test]
    fn measure_message_keeps_unknown_types_in_metadata() {
        let msg = measure_message(
            &group(
                0,
                vec![Measure {
                    value: 5,
                    meas_type: 9_999,
                    unit: 0,
                }],
            ),
            "health",
            "health-measures",
        );
        assert!(msg.body.unwrap().contains("(no readable measurement)"));
        assert_eq!(msg.metadata.unwrap()["measures"]["type_9999"], 5.0);
    }

    #[test]
    fn activity_message_is_stable_across_reimports() {
        let activity = Activity {
            date: "2026-09-17".into(),
            timezone: Some("Europe/Paris".into()),
            steps: Some(8_123),
            distance: Some(6_100.0),
            elevation: Some(12.0),
            soft: Some(3_600),
            moderate: Some(900),
            intense: Some(300),
            active: Some(1_200),
            calories: Some(420.5),
            totalcalories: Some(2_450.0),
            hr_average: Some(68),
            hr_min: Some(48),
            hr_max: Some(151),
        };
        let first = activity_message(&activity, "health", "health-activity");
        let second = activity_message(&activity, "health", "health-activity");

        // The same day always yields the same row, so a re-read updates it.
        assert_eq!(first.id, "health-activity-2026-09-17");
        assert_eq!(first.id, second.id);
        assert_eq!(first.external_id, second.external_id);
        assert_eq!(first.timestamp, day_start_timestamp("2026-09-17"));

        let body = first.body.unwrap();
        assert!(body.contains("8123 steps"));
        assert!(body.contains("6.1 km"));
        assert!(body.contains("2450 kcal"));
        assert!(body.contains("1h 20m active"));
    }

    #[test]
    fn sleep_message_reports_the_night_at_wake_up() {
        let night = SleepSummary {
            id: 909,
            date: Some("2026-09-17".into()),
            startdate: 1_757_800_000,
            enddate: 1_757_827_720,
            timezone: None,
            data: SleepData {
                lightsleepduration: Some(14_400),
                deepsleepduration: Some(7_200),
                remsleepduration: Some(6_120),
                wakeupduration: Some(1_800),
                hr_average: Some(54),
                sleep_score: Some(88),
                ..SleepData::default()
            },
        };
        let msg = sleep_message(&night, "health", "health-sleep");

        assert_eq!(msg.id, "health-sleep-909");
        assert_eq!(msg.timestamp, 1_757_827_720);
        let body = msg.body.unwrap();
        assert!(body.contains("Night of 2026-09-17"));
        assert!(body.contains("7h 42m asleep"));
        assert!(body.contains("score 88"));
        assert_eq!(msg.metadata.unwrap()["sleep_score"], 88);
    }

    #[test]
    fn workout_message_names_the_sport_and_sums_the_session() {
        let workout = Workout {
            id: 4242,
            category: 2,
            startdate: 1_757_900_000,
            enddate: 1_757_903_600,
            date: Some("2026-09-15".into()),
            timezone: None,
            deviceid: Some("watch".into()),
            data: WorkoutData {
                calories: Some(612.0),
                effduration: Some(3_300),
                distance: Some(10_500.0),
                hr_average: Some(148),
                hr_max: Some(176),
                ..WorkoutData::default()
            },
        };
        let msg = workout_message(&workout, "health", "health-workouts");

        assert_eq!(msg.id, "health-workout-4242");
        assert_eq!(msg.timestamp, 1_757_900_000);
        let body = msg.body.unwrap();
        assert!(body.starts_with("Run — "));
        assert!(body.contains("1h"));
        assert!(body.contains("10.5 km"));
        assert!(body.contains("612 kcal"));
        assert!(body.contains("148 bpm avg"));
        assert_eq!(msg.metadata.unwrap()["sport"], "Run");
    }

    #[test]
    fn workout_message_falls_back_on_an_unknown_sport() {
        let workout = Workout {
            id: 1,
            category: 9_999,
            startdate: 10,
            enddate: 20,
            date: None,
            timezone: None,
            deviceid: None,
            data: WorkoutData::default(),
        };
        let body = workout_message(&workout, "health", "c").body.unwrap();
        assert!(body.starts_with("Workout #9999 — "));
    }

    #[test]
    fn heart_message_reports_an_ecg_with_its_afib_verdict() {
        let measure = HeartMeasure {
            deviceid: Some("scanwatch".into()),
            model: Some(91),
            heart_rate: Some(61),
            timestamp: 1_757_900_000,
            timezone: None,
            ecg: Some(Ecg {
                signalid: 777,
                afib: Some(1),
            }),
            bloodpressure: None,
        };
        let msg = heart_message(&measure, "health", "health-heart");

        assert_eq!(msg.id, "health-heart-ecg777");
        let body = msg.body.unwrap();
        assert!(body.starts_with("ECG recording — "));
        assert!(body.contains("AFib detected"));
        assert!(body.contains("61 bpm"));
        assert_eq!(msg.metadata.unwrap()["signalid"], 777);
    }

    #[test]
    fn heart_message_without_an_ecg_keys_on_the_instant() {
        let measure = HeartMeasure {
            deviceid: None,
            model: None,
            heart_rate: Some(58),
            timestamp: 1_757_900_123,
            timezone: None,
            ecg: None,
            bloodpressure: Some(BloodPressure {
                systole: Some(118),
                diastole: Some(74),
            }),
        };
        let msg = heart_message(&measure, "health", "health-heart");

        assert_eq!(msg.id, "health-heart-hr1757900123");
        let body = msg.body.unwrap();
        assert!(body.starts_with("Heart measurement — "));
        assert!(body.contains("118/74 mmHg"));
    }

    #[test]
    fn device_message_is_one_stable_row_per_device() {
        let device = Device {
            deviceid: "abc123".into(),
            device_type: Some("Scale".into()),
            model: Some("Body+".into()),
            model_id: Some(5),
            battery: Some("high".into()),
            timezone: None,
            first_session_date: Some(1_700_000_000),
            last_session_date: Some(1_757_900_000),
        };
        let first = device_message(&device, "health", "health-devices");
        let second = device_message(&device, "health", "health-devices");

        assert_eq!(first.id, "health-device-abc123");
        assert_eq!(first.id, second.id);
        assert_eq!(first.timestamp, 1_757_900_000);
        let body = first.body.unwrap();
        assert!(body.starts_with("Body+"));
        assert!(body.contains("battery high"));
        assert_eq!(second.metadata.unwrap()["battery"], "high");
    }

    #[test]
    fn window_start_backfills_once_then_narrows() {
        let now = 1_000_000_000;
        let backfill = now - 365 * 86_400;
        assert_eq!(window_start(true, backfill, now), backfill);
        assert_eq!(
            window_start(false, backfill, now),
            now - RECHECK_WINDOW_DAYS * 86_400
        );
        // A backfill shorter than the re-check window is never widened.
        let short = now - 2 * 86_400;
        assert_eq!(window_start(false, short, now), short);
    }

    #[test]
    fn ymd_round_trips_through_day_start() {
        let ts = day_start_timestamp("2026-09-17");
        assert_eq!(to_ymd(ts), "2026-09-17");
    }

    #[test]
    fn conversation_ids_are_namespaced_per_stream() {
        assert_eq!(conversation_id("health", Stream::Sleep), "health-sleep");
        assert_eq!(
            conversation_external_id("health", Stream::Measures),
            "withings_health_measures"
        );
    }
}
