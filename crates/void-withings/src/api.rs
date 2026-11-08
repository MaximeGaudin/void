//! Minimal Withings Health Mate API client (<https://developer.withings.com>).
//!
//! Endpoints used:
//! - `POST /measure?action=getmeas` — body measurement groups (scale, BP monitor)
//! - `POST /v2/measure?action=getactivity` — one aggregate per day
//! - `POST /v2/sleep?action=getsummary` — one aggregate per night
//!
//! Every response is an envelope carrying an application status, so failures
//! arrive as HTTP 200 and are unwrapped in [`crate::envelope`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::auth::{self, TokenCache};
use crate::envelope::decode_body;
use crate::error::WithingsError;

pub use crate::auth::DEFAULT_API_BASE;

/// Minimum spacing between two requests (Withings allows ~120/min per user).
const REQUEST_PACE: Duration = Duration::from_millis(250);
/// Status 601 is Withings' rate limit; retry a few times before giving up.
const MAX_RATE_LIMIT_RETRIES: u32 = 3;
const RATE_LIMIT_WAIT: Duration = Duration::from_secs(5);

/// Measurement types requested from `getmeas`, and how to render them.
///
/// `(type, label, unit)` — see the Withings "Measure - Getmeas" reference.
pub const MEAS_TYPES: &[(i64, &str, &str)] = &[
    (1, "Weight", "kg"),
    (4, "Height", "m"),
    (5, "Fat free mass", "kg"),
    (6, "Fat ratio", "%"),
    (8, "Fat mass", "kg"),
    (9, "Diastolic", "mmHg"),
    (10, "Systolic", "mmHg"),
    (11, "Heart rate", "bpm"),
    (12, "Temperature", "°C"),
    (54, "SpO2", "%"),
    (71, "Body temperature", "°C"),
    (73, "Skin temperature", "°C"),
    (76, "Muscle mass", "kg"),
    (77, "Hydration", "kg"),
    (88, "Bone mass", "kg"),
    (91, "Pulse wave velocity", "m/s"),
    (123, "VO2 max", "ml/min/kg"),
    (155, "Vascular age", "years"),
    (168, "Visceral fat", ""),
];

/// Label and unit for a measurement type, when we know it.
pub fn meas_label(meas_type: i64) -> Option<(&'static str, &'static str)> {
    MEAS_TYPES
        .iter()
        .find(|(t, _, _)| *t == meas_type)
        .map(|(_, label, unit)| (*label, *unit))
}

/// The `meastypes` parameter: every type we can render.
pub fn fetch_meastypes() -> String {
    MEAS_TYPES
        .iter()
        .map(|(t, _, _)| t.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Fields requested from `getactivity`.
const ACTIVITY_FIELDS: &str = "steps,distance,elevation,soft,moderate,intense,active,calories,totalcalories,hr_average,hr_min,hr_max";
/// Fields requested from `getworkouts`.
const WORKOUT_FIELDS: &str = "calories,effduration,intensity,manual_distance,manual_calories,hr_average,hr_min,hr_max,spo2_average,steps,distance,elevation,pool_laps,strokes";
/// Fields requested from sleep `getsummary`.
const SLEEP_FIELDS: &str = "wakeupduration,lightsleepduration,deepsleepduration,remsleepduration,durationtosleep,durationtowakeup,wakeupcount,hr_average,hr_min,hr_max,rr_average,breathing_disturbances_intensity,snoring,sleep_score";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MeasureGroups {
    /// Server clock for this response; feed it back as `lastupdate` next time.
    #[serde(default)]
    pub updatetime: i64,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub measuregrps: Vec<MeasureGroup>,
    #[serde(default)]
    more: Option<serde_json::Value>,
    #[serde(default)]
    offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MeasureGroup {
    pub grpid: i64,
    /// 1 means the device could not attribute the measure to a user, so it may
    /// not be yours.
    #[serde(default)]
    pub attrib: i64,
    #[serde(default)]
    pub date: i64,
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub category: Option<i64>,
    #[serde(default)]
    pub deviceid: Option<String>,
    #[serde(default)]
    pub measures: Vec<Measure>,
}

impl MeasureGroup {
    /// A group Withings could not attribute to this user.
    pub fn is_ambiguous(&self) -> bool {
        self.attrib == 1
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Measure {
    pub value: i64,
    #[serde(rename = "type")]
    pub meas_type: i64,
    pub unit: i32,
}

impl Measure {
    /// Withings encodes values as `value × 10^unit`.
    pub fn real_value(&self) -> f64 {
        self.value as f64 * 10f64.powi(self.unit)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ActivityPage {
    #[serde(default)]
    activities: Vec<Activity>,
    #[serde(default)]
    more: Option<serde_json::Value>,
    #[serde(default)]
    offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Activity {
    /// `YYYY-MM-DD` in the user's timezone.
    pub date: String,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub steps: Option<i64>,
    /// Metres.
    #[serde(default)]
    pub distance: Option<f64>,
    #[serde(default)]
    pub elevation: Option<f64>,
    /// Seconds of soft / moderate / intense activity.
    #[serde(default)]
    pub soft: Option<i64>,
    #[serde(default)]
    pub moderate: Option<i64>,
    #[serde(default)]
    pub intense: Option<i64>,
    #[serde(default)]
    pub active: Option<i64>,
    #[serde(default)]
    pub calories: Option<f64>,
    #[serde(default)]
    pub totalcalories: Option<f64>,
    #[serde(default)]
    pub hr_average: Option<i64>,
    #[serde(default)]
    pub hr_min: Option<i64>,
    #[serde(default)]
    pub hr_max: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SleepPage {
    #[serde(default)]
    series: Vec<SleepSummary>,
    #[serde(default)]
    more: Option<serde_json::Value>,
    #[serde(default)]
    offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SleepSummary {
    pub id: i64,
    /// `YYYY-MM-DD` of the night.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub startdate: i64,
    #[serde(default)]
    pub enddate: i64,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub data: SleepData,
}

/// Durations are seconds.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SleepData {
    #[serde(default)]
    pub wakeupduration: Option<i64>,
    #[serde(default)]
    pub lightsleepduration: Option<i64>,
    #[serde(default)]
    pub deepsleepduration: Option<i64>,
    #[serde(default)]
    pub remsleepduration: Option<i64>,
    #[serde(default)]
    pub durationtosleep: Option<i64>,
    #[serde(default)]
    pub durationtowakeup: Option<i64>,
    #[serde(default)]
    pub wakeupcount: Option<i64>,
    #[serde(default)]
    pub hr_average: Option<i64>,
    #[serde(default)]
    pub hr_min: Option<i64>,
    #[serde(default)]
    pub hr_max: Option<i64>,
    #[serde(default)]
    pub rr_average: Option<f64>,
    #[serde(default)]
    pub breathing_disturbances_intensity: Option<i64>,
    #[serde(default)]
    pub snoring: Option<i64>,
    #[serde(default)]
    pub sleep_score: Option<i64>,
}

impl SleepData {
    /// Light + deep + REM, the time actually asleep.
    pub fn total_sleep_secs(&self) -> i64 {
        self.lightsleepduration.unwrap_or(0)
            + self.deepsleepduration.unwrap_or(0)
            + self.remsleepduration.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WorkoutPage {
    #[serde(default)]
    series: Vec<Workout>,
    #[serde(default)]
    more: Option<serde_json::Value>,
    #[serde(default)]
    offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workout {
    pub id: i64,
    /// Withings' sport code; see [`workout_label`].
    #[serde(default)]
    pub category: i64,
    #[serde(default)]
    pub startdate: i64,
    #[serde(default)]
    pub enddate: i64,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub deviceid: Option<String>,
    #[serde(default)]
    pub data: WorkoutData,
}

/// Durations are seconds, distance metres.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkoutData {
    #[serde(default)]
    pub calories: Option<f64>,
    #[serde(default)]
    pub manual_calories: Option<f64>,
    /// Time actually moving, pauses removed.
    #[serde(default)]
    pub effduration: Option<i64>,
    #[serde(default)]
    pub intensity: Option<i64>,
    #[serde(default)]
    pub distance: Option<f64>,
    #[serde(default)]
    pub manual_distance: Option<f64>,
    #[serde(default)]
    pub elevation: Option<f64>,
    #[serde(default)]
    pub steps: Option<i64>,
    #[serde(default)]
    pub hr_average: Option<i64>,
    #[serde(default)]
    pub hr_min: Option<i64>,
    #[serde(default)]
    pub hr_max: Option<i64>,
    #[serde(default)]
    pub spo2_average: Option<i64>,
    #[serde(default)]
    pub pool_laps: Option<i64>,
    #[serde(default)]
    pub strokes: Option<i64>,
}

impl Workout {
    /// Wall-clock length, pauses included.
    pub fn duration_secs(&self) -> i64 {
        (self.enddate - self.startdate).max(0)
    }
}

/// Sport name for a workout category. Unknown codes keep their number so a new
/// Withings sport still renders something meaningful.
pub fn workout_label(category: i64) -> Option<&'static str> {
    Some(match category {
        1 => "Walk",
        2 => "Run",
        3 => "Hiking",
        4 => "Skating",
        5 => "BMX",
        6 => "Cycling",
        7 => "Swimming",
        8 => "Surfing",
        9 => "Kitesurfing",
        10 => "Windsurfing",
        11 => "Bodyboard",
        12 => "Tennis",
        13 => "Table tennis",
        14 => "Squash",
        15 => "Badminton",
        16 => "Weights",
        17 => "Calisthenics",
        18 => "Elliptical",
        19 => "Pilates",
        20 => "Basketball",
        21 => "Soccer",
        22 => "Football",
        23 => "Rugby",
        24 => "Volleyball",
        25 => "Water polo",
        26 => "Horse riding",
        27 => "Golf",
        28 => "Yoga",
        29 => "Dancing",
        30 => "Boxing",
        31 => "Fencing",
        32 => "Wrestling",
        33 => "Martial arts",
        34 => "Skiing",
        35 => "Snowboarding",
        36 => "Other",
        187 => "Rowing",
        188 => "Zumba",
        191 => "Baseball",
        192 => "Handball",
        193 => "Hockey",
        194 => "Ice hockey",
        195 => "Climbing",
        196 => "Ice skating",
        272 => "Multi-sport",
        306 => "Indoor walk",
        307 => "Indoor run",
        308 => "Indoor cycling",
        _ => return None,
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
struct HeartPage {
    #[serde(default)]
    series: Vec<HeartMeasure>,
    #[serde(default)]
    more: Option<serde_json::Value>,
    #[serde(default)]
    offset: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeartMeasure {
    #[serde(default)]
    pub deviceid: Option<String>,
    #[serde(default)]
    pub model: Option<i64>,
    #[serde(default)]
    pub heart_rate: Option<i64>,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub ecg: Option<Ecg>,
    #[serde(default)]
    pub bloodpressure: Option<BloodPressure>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ecg {
    pub signalid: i64,
    #[serde(default)]
    pub afib: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BloodPressure {
    #[serde(default)]
    pub diastole: Option<i64>,
    #[serde(default)]
    pub systole: Option<i64>,
}

impl HeartMeasure {
    /// A stable id: the ECG signal when there is one, else the instant.
    pub fn stable_id(&self) -> String {
        match &self.ecg {
            Some(ecg) => format!("ecg{}", ecg.signalid),
            None => format!("hr{}", self.timestamp),
        }
    }
}

/// Withings' AFib classification for an ECG recording.
pub fn afib_label(afib: i64) -> &'static str {
    match afib {
        0 => "no AFib",
        1 => "AFib detected",
        2 => "inconclusive (high heart rate)",
        3 => "inconclusive (low heart rate)",
        4 => "inconclusive (poor recording)",
        _ => "inconclusive",
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct DeviceList {
    #[serde(default)]
    devices: Vec<Device>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    pub deviceid: String,
    /// "Scale", "Blood Pressure Monitor", "Activity Tracker", "Sleep Monitor", …
    #[serde(default)]
    #[serde(rename = "type")]
    pub device_type: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_id: Option<i64>,
    /// "high", "medium", "low" — Withings does not report a percentage.
    #[serde(default)]
    pub battery: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub first_session_date: Option<i64>,
    #[serde(default)]
    pub last_session_date: Option<i64>,
}

/// `more` is `1` on `/measure` and `true` on the v2 endpoints.
fn has_more(value: &Option<serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(serde_json::Value::String(s)) => s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

pub struct WithingsClient {
    http: Client,
    client_id: String,
    client_secret: String,
    token_path: PathBuf,
    api_base: String,
    token: Mutex<Option<TokenCache>>,
}

impl WithingsClient {
    pub fn new(client_id: &str, client_secret: &str, token_path: &Path) -> Self {
        Self::with_api_base(client_id, client_secret, token_path, DEFAULT_API_BASE)
    }

    /// Override the API base URL (tests point it at a mock server).
    pub fn with_api_base(
        client_id: &str,
        client_secret: &str,
        token_path: &Path,
        api_base: &str,
    ) -> Self {
        Self {
            http: Client::new(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            token_path: token_path.to_path_buf(),
            api_base: api_base.trim_end_matches('/').to_string(),
            token: Mutex::new(None),
        }
    }

    pub fn token_path(&self) -> &Path {
        &self.token_path
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    /// Run the browser flow and persist the resulting tokens.
    pub async fn authorize_interactive(&self) -> Result<(), WithingsError> {
        let tokens = auth::authorize_interactive(
            &self.http,
            &self.client_id,
            &self.client_secret,
            &self.api_base,
        )
        .await?;
        self.store(tokens).await
    }

    /// Persist a token pair obtained elsewhere (the setup wizard).
    pub async fn store(&self, tokens: TokenCache) -> Result<(), WithingsError> {
        tokens.save(&self.token_path)?;
        *self.token.lock().await = Some(tokens);
        Ok(())
    }

    /// A valid access token, refreshing and rewriting the token file if needed.
    ///
    /// Withings rotates the refresh token on every refresh and kills the old
    /// one, so an in-memory copy goes stale the moment another process (the
    /// sync daemon, a CLI command) refreshes. The file on disk is the only
    /// source of truth: it is re-read on every call, and the refresh itself is
    /// serialized across processes.
    pub async fn access_token(&self) -> Result<String, WithingsError> {
        let mut guard = self.token.lock().await;

        let current = match TokenCache::load(&self.token_path) {
            Ok(from_disk) => from_disk,
            // A transient read failure should not kill a sync that already
            // holds a usable token.
            Err(e) => match guard.as_ref() {
                Some(cached) if !cached.needs_refresh(chrono::Utc::now().timestamp()) => {
                    warn!(error = %e, "could not re-read the Withings token file, using the cached token");
                    return Ok(cached.access_token.clone());
                }
                _ => return Err(e),
            },
        };

        if !current.needs_refresh(chrono::Utc::now().timestamp()) {
            let access_token = current.access_token.clone();
            *guard = Some(current);
            return Ok(access_token);
        }

        let lock = crate::lock::RefreshLock::acquire(&self.token_path).await?;

        // Whoever held the lock has just rotated the pair: re-read before
        // spending our (possibly dead) refresh token.
        let current = match TokenCache::load(&self.token_path) {
            Ok(from_disk) => {
                if !from_disk.needs_refresh(chrono::Utc::now().timestamp()) {
                    let access_token = from_disk.access_token.clone();
                    *guard = Some(from_disk);
                    return Ok(access_token);
                }
                from_disk
            }
            Err(e) => return Err(e),
        };

        if lock.is_none() {
            return Err(WithingsError::Auth(
                "another process has been refreshing the Withings token for too long".into(),
            ));
        }

        debug!("refreshing the Withings access token");
        let refreshed = auth::refresh_tokens(
            &self.http,
            &self.client_id,
            &self.client_secret,
            &current.refresh_token,
            &self.api_base,
        )
        .await
        .map_err(|e| {
            if e.is_invalid_token() {
                WithingsError::Auth(
                    "the Withings refresh token was rejected — re-authorize with `void setup`"
                        .into(),
                )
            } else {
                e
            }
        })?;

        // The refresh token rotated: the file is the only copy that still works.
        refreshed.save(&self.token_path)?;
        let access_token = refreshed.access_token.clone();
        *guard = Some(refreshed);
        Ok(access_token)
    }

    /// Replace an access token the API just rejected.
    ///
    /// Another process may already have rotated it, in which case the file
    /// holds a working token and spending our refresh token would be wasteful.
    async fn replace_rejected_token(&self, rejected: &str) -> Result<String, WithingsError> {
        let mut guard = self.token.lock().await;
        let lock = crate::lock::RefreshLock::acquire(&self.token_path).await?;

        let current = TokenCache::load(&self.token_path)?;
        if current.access_token != rejected {
            let access_token = current.access_token.clone();
            *guard = Some(current);
            return Ok(access_token);
        }

        if lock.is_none() {
            return Err(WithingsError::Auth(
                "another process has been refreshing the Withings token for too long".into(),
            ));
        }

        warn!("Withings rejected the access token, forcing a refresh");
        let refreshed = auth::refresh_tokens(
            &self.http,
            &self.client_id,
            &self.client_secret,
            &current.refresh_token,
            &self.api_base,
        )
        .await
        .map_err(|e| {
            if e.is_invalid_token() {
                WithingsError::Auth(
                    "the Withings refresh token was rejected — re-authorize with `void setup`"
                        .into(),
                )
            } else {
                e
            }
        })?;

        refreshed.save(&self.token_path)?;
        let access_token = refreshed.access_token.clone();
        *guard = Some(refreshed);
        Ok(access_token)
    }

    async fn post(&self, path: &str, params: &[(&str, String)]) -> Result<String, WithingsError> {
        let url = format!("{}/{}", self.api_base, path.trim_start_matches('/'));
        let mut rate_limit_attempt = 0;
        let mut refreshed = false;

        loop {
            tokio::time::sleep(REQUEST_PACE).await;
            let token = self.access_token().await?;
            let raw = self
                .http
                .post(&url)
                .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
                .form(params)
                .send()
                .await?
                .text()
                .await?;

            match crate::envelope::unwrap_body(&raw) {
                Ok(_) => return Ok(raw),
                // Withings occasionally rejects a token it just issued; one
                // forced refresh separates that from a dead grant.
                Err(e) if e.is_invalid_token() && !refreshed => {
                    self.replace_rejected_token(&token).await?;
                    refreshed = true;
                }
                Err(WithingsError::Api { status: 601, .. })
                    if rate_limit_attempt < MAX_RATE_LIMIT_RETRIES =>
                {
                    rate_limit_attempt += 1;
                    warn!(
                        attempt = rate_limit_attempt,
                        "Withings rate limited the request, waiting"
                    );
                    tokio::time::sleep(RATE_LIMIT_WAIT).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Measurement groups in a closed time range (the first, backfilling sync).
    pub async fn measures_between(
        &self,
        start: i64,
        end: i64,
    ) -> Result<MeasureGroups, WithingsError> {
        self.measures(&[
            ("startdate", start.to_string()),
            ("enddate", end.to_string()),
        ])
        .await
    }

    /// Measurement groups created or edited since `lastupdate` (incremental).
    pub async fn measures_since(&self, lastupdate: i64) -> Result<MeasureGroups, WithingsError> {
        self.measures(&[("lastupdate", lastupdate.to_string())])
            .await
    }

    async fn measures(&self, window: &[(&str, String)]) -> Result<MeasureGroups, WithingsError> {
        let mut merged = MeasureGroups::default();
        let mut offset: Option<i64> = None;

        loop {
            let mut params = vec![
                ("action", "getmeas".to_string()),
                ("meastypes", fetch_meastypes()),
                ("category", "1".to_string()),
            ];
            params.extend(window.iter().map(|(k, v)| (*k, v.clone())));
            if let Some(offset) = offset {
                params.push(("offset", offset.to_string()));
            }

            let raw = self.post("measure", &params).await?;
            let mut page: MeasureGroups = decode_body(&raw)?;
            merged.updatetime = merged.updatetime.max(page.updatetime);
            if merged.timezone.is_none() {
                merged.timezone = page.timezone.clone();
            }
            merged.measuregrps.append(&mut page.measuregrps);

            match (has_more(&page.more), page.offset) {
                (true, Some(next)) if Some(next) != offset => offset = Some(next),
                _ => break,
            }
        }

        Ok(merged)
    }

    /// Daily activity aggregates between two `YYYY-MM-DD` dates, inclusive.
    pub async fn activities(
        &self,
        start_ymd: &str,
        end_ymd: &str,
    ) -> Result<Vec<Activity>, WithingsError> {
        let mut all = Vec::new();
        let mut offset: Option<i64> = None;

        loop {
            let mut params = vec![
                ("action", "getactivity".to_string()),
                ("startdateymd", start_ymd.to_string()),
                ("enddateymd", end_ymd.to_string()),
                ("data_fields", ACTIVITY_FIELDS.to_string()),
            ];
            if let Some(offset) = offset {
                params.push(("offset", offset.to_string()));
            }

            let raw = self.post("v2/measure", &params).await?;
            let mut page: ActivityPage = decode_body(&raw)?;
            all.append(&mut page.activities);

            match (has_more(&page.more), page.offset) {
                (true, Some(next)) if Some(next) != offset => offset = Some(next),
                _ => break,
            }
        }

        Ok(all)
    }

    /// Sleep summaries between two `YYYY-MM-DD` dates, inclusive.
    pub async fn sleep_summaries(
        &self,
        start_ymd: &str,
        end_ymd: &str,
    ) -> Result<Vec<SleepSummary>, WithingsError> {
        let mut all = Vec::new();
        let mut offset: Option<i64> = None;

        loop {
            let mut params = vec![
                ("action", "getsummary".to_string()),
                ("startdateymd", start_ymd.to_string()),
                ("enddateymd", end_ymd.to_string()),
                ("data_fields", SLEEP_FIELDS.to_string()),
            ];
            if let Some(offset) = offset {
                params.push(("offset", offset.to_string()));
            }

            let raw = self.post("v2/sleep", &params).await?;
            let mut page: SleepPage = decode_body(&raw)?;
            all.append(&mut page.series);

            match (has_more(&page.more), page.offset) {
                (true, Some(next)) if Some(next) != offset => offset = Some(next),
                _ => break,
            }
        }

        Ok(all)
    }

    /// Workout sessions between two `YYYY-MM-DD` dates, inclusive.
    pub async fn workouts(
        &self,
        start_ymd: &str,
        end_ymd: &str,
    ) -> Result<Vec<Workout>, WithingsError> {
        let mut all = Vec::new();
        let mut offset: Option<i64> = None;

        loop {
            let mut params = vec![
                ("action", "getworkouts".to_string()),
                ("startdateymd", start_ymd.to_string()),
                ("enddateymd", end_ymd.to_string()),
                ("data_fields", WORKOUT_FIELDS.to_string()),
            ];
            if let Some(offset) = offset {
                params.push(("offset", offset.to_string()));
            }

            let raw = self.post("v2/measure", &params).await?;
            let mut page: WorkoutPage = decode_body(&raw)?;
            all.append(&mut page.series);

            match (has_more(&page.more), page.offset) {
                (true, Some(next)) if Some(next) != offset => offset = Some(next),
                _ => break,
            }
        }

        Ok(all)
    }

    /// Heart measurements (ECG recordings, blood pressure) in a time range.
    ///
    /// Unlike activity and sleep, this endpoint takes unix seconds.
    pub async fn heart_measures(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<HeartMeasure>, WithingsError> {
        let mut all = Vec::new();
        let mut offset: Option<i64> = None;

        loop {
            let mut params = vec![
                ("action", "list".to_string()),
                ("startdate", start.to_string()),
                ("enddate", end.to_string()),
            ];
            if let Some(offset) = offset {
                params.push(("offset", offset.to_string()));
            }

            let raw = self.post("v2/heart", &params).await?;
            let mut page: HeartPage = decode_body(&raw)?;
            all.append(&mut page.series);

            match (has_more(&page.more), page.offset) {
                (true, Some(next)) if Some(next) != offset => offset = Some(next),
                _ => break,
            }
        }

        Ok(all)
    }

    /// Devices linked to the account, with their battery level.
    pub async fn devices(&self) -> Result<Vec<Device>, WithingsError> {
        let raw = self
            .post("v2/user", &[("action", "getdevice".to_string())])
            .await?;
        let list: DeviceList = decode_body(&raw)?;
        Ok(list.devices)
    }

    /// Cheapest call that proves the credentials work.
    pub async fn probe(&self) -> Result<(), WithingsError> {
        let now = chrono::Utc::now().timestamp();
        self.measures_between(now - 86_400, now).await?;
        info!("Withings credentials verified");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn token_file(dir: &Path, expires_at: i64) -> PathBuf {
        let path = dir.join("conn-withings-token.json");
        let tokens = TokenCache {
            access_token: "acc-1".into(),
            refresh_token: "ref-1".into(),
            expires_at,
            userid: Some("42".into()),
            scope: None,
        };
        tokens.save(&path).unwrap();
        path
    }

    fn client(server: &MockServer, token_path: &Path) -> WithingsClient {
        WithingsClient::with_api_base("cid", "secret", token_path, &server.uri())
    }

    #[test]
    fn measure_values_apply_the_unit_exponent() {
        let measure = Measure {
            value: 78_400,
            meas_type: 1,
            unit: -3,
        };
        assert!((measure.real_value() - 78.4).abs() < 1e-9);
    }

    #[test]
    fn known_measure_types_have_labels() {
        assert_eq!(meas_label(1), Some(("Weight", "kg")));
        assert_eq!(meas_label(10), Some(("Systolic", "mmHg")));
        assert_eq!(meas_label(9999), None);
        assert!(fetch_meastypes().starts_with("1,4,5,6,8"));
    }

    #[test]
    fn more_is_read_as_bool_int_or_string() {
        assert!(has_more(&Some(serde_json::json!(true))));
        assert!(has_more(&Some(serde_json::json!(1))));
        assert!(has_more(&Some(serde_json::json!("1"))));
        assert!(!has_more(&Some(serde_json::json!(false))));
        assert!(!has_more(&Some(serde_json::json!(0))));
        assert!(!has_more(&None));
    }

    #[test]
    fn sleep_total_sums_the_three_phases() {
        let data = SleepData {
            lightsleepduration: Some(10_000),
            deepsleepduration: Some(5_000),
            remsleepduration: Some(3_000),
            ..SleepData::default()
        };
        assert_eq!(data.total_sleep_secs(), 18_000);
    }

    #[test]
    fn deserializes_a_realistic_measure_group() {
        let raw = r#"{"status":0,"body":{"updatetime":1758000000,"timezone":"Europe/Paris",
            "measuregrps":[{"grpid":111,"attrib":0,"date":1757900000,"created":1757900001,
            "category":1,"deviceid":"dev","measures":[
                {"value":78400,"type":1,"unit":-3},
                {"value":142,"type":6,"unit":-1}]}]}}"#;
        let groups: MeasureGroups = decode_body(raw).unwrap();
        assert_eq!(groups.updatetime, 1_758_000_000);
        let group = &groups.measuregrps[0];
        assert!(!group.is_ambiguous());
        assert!((group.measures[0].real_value() - 78.4).abs() < 1e-9);
        assert!((group.measures[1].real_value() - 14.2).abs() < 1e-9);
    }

    #[test]
    fn ambiguous_groups_are_flagged() {
        let raw = r#"{"status":0,"body":{"measuregrps":[{"grpid":1,"attrib":1,"date":1,"measures":[]}]}}"#;
        let groups: MeasureGroups = decode_body(raw).unwrap();
        assert!(groups.measuregrps[0].is_ambiguous());
    }

    #[tokio::test]
    async fn measures_between_sends_the_window_and_decodes_groups() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), chrono::Utc::now().timestamp() + 3_600);

        Mock::given(method("POST"))
            .and(path("/measure"))
            .and(body_string_contains("action=getmeas"))
            .and(body_string_contains("startdate=100"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"updatetime":900,"measuregrps":[
                    {"grpid":7,"attrib":0,"date":500,"measures":[{"value":705,"type":1,"unit":-1}]}]}}"#,
            ))
            .mount(&server)
            .await;

        let groups = client(&server, &token_path)
            .measures_between(100, 900)
            .await
            .unwrap();
        assert_eq!(groups.measuregrps.len(), 1);
        assert_eq!(groups.updatetime, 900);
    }

    #[tokio::test]
    async fn an_expired_access_token_is_refreshed_and_rewritten() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        // Already expired, so the first call must refresh.
        let token_path = token_file(dir.path(), 0);

        Mock::given(method("POST"))
            .and(path("/v2/oauth2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"access_token":"acc-2","refresh_token":"ref-2","expires_in":10800,"userid":"42"}}"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v2/measure"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"activities":[{"date":"2026-09-17","steps":8000}]}}"#,
            ))
            .mount(&server)
            .await;

        let activities = client(&server, &token_path)
            .activities("2026-09-17", "2026-09-17")
            .await
            .unwrap();
        assert_eq!(activities[0].steps, Some(8_000));

        // The rotated refresh token replaced the old one on disk.
        let stored = TokenCache::load(&token_path).unwrap();
        assert_eq!(stored.access_token, "acc-2");
        assert_eq!(stored.refresh_token, "ref-2");
    }

    #[tokio::test]
    async fn a_dead_refresh_token_points_at_setup() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), 0);

        Mock::given(method("POST"))
            .and(path("/v2/oauth2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"status":401,"body":{},"error":"invalid_grant"}"#),
            )
            .mount(&server)
            .await;

        let err = client(&server, &token_path)
            .measures_since(1)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("void setup"));
    }

    #[tokio::test]
    async fn a_token_rotated_by_another_process_is_picked_up() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let valid_for_an_hour = chrono::Utc::now().timestamp() + 3_600;
        let token_path = token_file(dir.path(), valid_for_an_hour);

        let client = client(&server, &token_path);
        assert_eq!(client.access_token().await.unwrap(), "acc-1");

        // Another process (the sync daemon, say) refreshes and rotates the pair.
        TokenCache {
            access_token: "acc-rotated".into(),
            refresh_token: "ref-rotated".into(),
            expires_at: valid_for_an_hour,
            userid: Some("42".into()),
            scope: None,
        }
        .save(&token_path)
        .unwrap();

        // No token endpoint is mounted: reading the file is the only way this
        // can succeed, which is exactly the point.
        assert_eq!(client.access_token().await.unwrap(), "acc-rotated");
    }

    #[tokio::test]
    async fn two_clients_sharing_a_token_file_refresh_only_once() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), 0);

        Mock::given(method("POST"))
            .and(path("/v2/oauth2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"access_token":"acc-2","refresh_token":"ref-2","expires_in":10800}}"#,
            ))
            // Withings kills the old refresh token, so a second refresh with
            // the stale one would fail in production.
            .expect(1)
            .mount(&server)
            .await;

        let first = client(&server, &token_path);
        let second = client(&server, &token_path);
        assert_eq!(first.access_token().await.unwrap(), "acc-2");
        assert_eq!(second.access_token().await.unwrap(), "acc-2");
    }

    #[tokio::test]
    async fn sleep_summaries_follow_pagination() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), chrono::Utc::now().timestamp() + 3_600);

        Mock::given(method("POST"))
            .and(path("/v2/sleep"))
            .and(body_string_contains("offset=1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"series":[{"id":2,"date":"2026-09-16","startdate":10,"enddate":20,"data":{"sleep_score":70}}],"more":false}}"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v2/sleep"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"series":[{"id":1,"date":"2026-09-17","startdate":30,"enddate":40,"data":{"sleep_score":88}}],"more":true,"offset":1}}"#,
            ))
            .mount(&server)
            .await;

        let series = client(&server, &token_path)
            .sleep_summaries("2026-09-16", "2026-09-17")
            .await
            .unwrap();
        assert_eq!(series.len(), 2);
        assert_eq!(series[1].data.sleep_score, Some(70));
    }

    #[test]
    fn workout_labels_cover_known_sports_only() {
        assert_eq!(workout_label(2), Some("Run"));
        assert_eq!(workout_label(6), Some("Cycling"));
        assert_eq!(workout_label(307), Some("Indoor run"));
        assert_eq!(workout_label(9_999), None);
    }

    #[test]
    fn afib_labels_split_positive_from_inconclusive() {
        assert_eq!(afib_label(0), "no AFib");
        assert_eq!(afib_label(1), "AFib detected");
        assert!(afib_label(2).starts_with("inconclusive"));
        assert!(afib_label(42).starts_with("inconclusive"));
    }

    #[test]
    fn heart_measures_key_on_the_ecg_signal_when_there_is_one() {
        let with_ecg = HeartMeasure {
            deviceid: None,
            model: None,
            heart_rate: Some(60),
            timestamp: 100,
            timezone: None,
            ecg: Some(Ecg {
                signalid: 9,
                afib: Some(0),
            }),
            bloodpressure: None,
        };
        assert_eq!(with_ecg.stable_id(), "ecg9");

        let without = HeartMeasure {
            ecg: None,
            ..with_ecg
        };
        assert_eq!(without.stable_id(), "hr100");
    }

    #[test]
    fn workout_duration_never_goes_negative() {
        let workout = Workout {
            id: 1,
            category: 1,
            startdate: 200,
            enddate: 100,
            date: None,
            timezone: None,
            deviceid: None,
            data: WorkoutData::default(),
        };
        assert_eq!(workout.duration_secs(), 0);
    }

    #[tokio::test]
    async fn workouts_are_decoded_from_the_v2_measure_endpoint() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), chrono::Utc::now().timestamp() + 3_600);

        Mock::given(method("POST"))
            .and(path("/v2/measure"))
            .and(body_string_contains("action=getworkouts"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"series":[{"id":5,"category":2,"startdate":100,"enddate":3700,
                    "date":"2026-09-15","data":{"calories":600.5,"distance":10000,"hr_average":150}}],"more":false}}"#,
            ))
            .mount(&server)
            .await;

        let workouts = client(&server, &token_path)
            .workouts("2026-09-15", "2026-09-15")
            .await
            .unwrap();
        assert_eq!(workouts.len(), 1);
        assert_eq!(workouts[0].duration_secs(), 3_600);
        assert_eq!(workouts[0].data.hr_average, Some(150));
    }

    #[tokio::test]
    async fn heart_measures_are_decoded_with_ecg_and_blood_pressure() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), chrono::Utc::now().timestamp() + 3_600);

        Mock::given(method("POST"))
            .and(path("/v2/heart"))
            .and(body_string_contains("action=list"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"series":[{"deviceid":"d1","model":91,"heart_rate":61,
                    "timestamp":1594159644,"ecg":{"signalid":123,"afib":0},
                    "bloodpressure":{"diastole":76,"systole":115}}],"more":false}}"#,
            ))
            .mount(&server)
            .await;

        let measures = client(&server, &token_path)
            .heart_measures(0, 1_600_000_000)
            .await
            .unwrap();
        assert_eq!(measures[0].stable_id(), "ecg123");
        assert_eq!(
            measures[0].bloodpressure.as_ref().unwrap().systole,
            Some(115)
        );
    }

    #[tokio::test]
    async fn devices_are_decoded_from_the_user_endpoint() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), chrono::Utc::now().timestamp() + 3_600);

        Mock::given(method("POST"))
            .and(path("/v2/user"))
            .and(body_string_contains("action=getdevice"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"status":0,"body":{"devices":[{"type":"Scale","battery":"high","model":"Body+",
                    "model_id":5,"deviceid":"abc","last_session_date":1757900000}]}}"#,
            ))
            .mount(&server)
            .await;

        let devices = client(&server, &token_path).devices().await.unwrap();
        assert_eq!(devices[0].deviceid, "abc");
        assert_eq!(devices[0].battery.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn an_api_error_is_surfaced_not_swallowed() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let token_path = token_file(dir.path(), chrono::Utc::now().timestamp() + 3_600);

        Mock::given(method("POST"))
            .and(path("/measure"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"status":503,"body":{},"error":"Invalid Params"}"#),
            )
            .mount(&server)
            .await;

        let err = client(&server, &token_path)
            .measures_between(0, 1)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Invalid Params"));
    }
}
