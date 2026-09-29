//! Attendee list building for calendar event creation and RSVP.

use crate::api::{AttendeeRequest, AttendeeResponseRequest, EventAttendee};

/// Pick the attendee entry that represents the calendar owner.
///
/// Priority: explicit `email` override, then the attendee Google flags with
/// `self: true`, then `account_email` (the primary calendar id). Matching is
/// case-insensitive. Returns the attendee's email exactly as Google stores it,
/// or `None` when the owner is not on the guest list.
pub(crate) fn find_self_attendee(
    attendees: &[EventAttendee],
    email_override: Option<&str>,
    account_email: Option<&str>,
) -> Option<String> {
    let by_email = |wanted: &str| {
        attendees
            .iter()
            .filter_map(|a| a.email.as_deref())
            .find(|e| e.eq_ignore_ascii_case(wanted))
            .map(str::to_string)
    };
    if let Some(wanted) = email_override {
        return by_email(wanted);
    }
    if let Some(me) = attendees
        .iter()
        .find(|a| a.is_self == Some(true))
        .and_then(|a| a.email.clone())
    {
        return Some(me);
    }
    account_email.and_then(by_email)
}

/// Rebuild the attendee list with `me`'s response set to `status`.
/// Other attendees keep their current response; no attendee is ever added.
pub(crate) fn build_response_attendees(
    attendees: &[EventAttendee],
    me: &str,
    status: &str,
    comment: Option<&str>,
) -> Vec<AttendeeResponseRequest> {
    attendees
        .iter()
        .map(|a| {
            let is_me = a.email.as_deref() == Some(me);
            AttendeeResponseRequest {
                email: a.email.clone().unwrap_or_default(),
                response_status: if is_me {
                    Some(status.to_string())
                } else {
                    a.response_status.clone()
                },
                comment: if is_me {
                    comment.map(str::to_string)
                } else {
                    None
                },
            }
        })
        .collect()
}

/// Derive the calendar account owner email from a connection ID.
/// Convention: `{email}-calendar` (e.g. `mgaudin@gladia.io-calendar`).
pub(crate) fn connection_owner_email(connection_id: &str) -> Option<String> {
    let email = connection_id.strip_suffix("-calendar")?;
    if email.contains('@') {
        Some(email.to_string())
    } else {
        None
    }
}

/// Build the attendee list for event creation, ensuring the calendar owner
/// is included when other attendees are specified.
pub(crate) fn build_attendee_list(
    connection_id: &str,
    attendees: Option<&str>,
) -> Option<Vec<AttendeeRequest>> {
    let mut emails: Vec<String> = attendees
        .map(|a| {
            a.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    if emails.is_empty() {
        return None;
    }

    if let Some(owner) = connection_owner_email(connection_id) {
        if !emails.iter().any(|e| e.eq_ignore_ascii_case(&owner)) {
            emails.insert(0, owner);
        }
    }

    Some(
        emails
            .into_iter()
            .map(|email| AttendeeRequest { email })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_owner_email_from_standard_id() {
        assert_eq!(
            connection_owner_email("mgaudin@gladia.io-calendar").as_deref(),
            Some("mgaudin@gladia.io")
        );
    }

    #[test]
    fn connection_owner_email_rejects_non_email_id() {
        assert_eq!(connection_owner_email("my-calendar"), None);
    }

    #[test]
    fn connection_owner_email_without_suffix() {
        assert_eq!(connection_owner_email("mgaudin@gladia.io"), None);
    }

    #[test]
    fn build_attendee_list_adds_owner_when_missing() {
        let list =
            build_attendee_list("mgaudin@gladia.io-calendar", Some("alice@example.com")).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].email, "mgaudin@gladia.io");
        assert_eq!(list[1].email, "alice@example.com");
    }

    #[test]
    fn build_attendee_list_does_not_duplicate_owner() {
        let list = build_attendee_list(
            "mgaudin@gladia.io-calendar",
            Some("mgaudin@gladia.io,alice@example.com"),
        )
        .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].email, "mgaudin@gladia.io");
        assert_eq!(list[1].email, "alice@example.com");
    }

    #[test]
    fn build_attendee_list_case_insensitive_dedup() {
        let list = build_attendee_list(
            "mgaudin@gladia.io-calendar",
            Some("MGAUDIN@gladia.io,alice@example.com"),
        )
        .unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn build_attendee_list_none_for_work_block() {
        assert!(build_attendee_list("mgaudin@gladia.io-calendar", None).is_none());
        assert!(build_attendee_list("mgaudin@gladia.io-calendar", Some("")).is_none());
    }

    #[test]
    fn build_attendee_list_no_owner_for_generic_connection() {
        let list = build_attendee_list("my-calendar", Some("alice@example.com")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "alice@example.com");
    }

    #[test]
    fn insert_request_serializes_owner_attendee() {
        use crate::api::{EventDateTimeRequest, InsertEventRequest};

        let attendees =
            build_attendee_list("mgaudin@gladia.io-calendar", Some("alice@example.com"));
        let request = InsertEventRequest {
            summary: "Team Sync".into(),
            description: None,
            start: EventDateTimeRequest {
                date_time: "2026-06-11T07:00:00Z".into(),
                time_zone: "UTC".into(),
            },
            end: EventDateTimeRequest {
                date_time: "2026-06-11T07:30:00Z".into(),
                time_zone: "UTC".into(),
            },
            attendees,
            conference_data: None,
        };

        let json = serde_json::to_value(&request).unwrap();
        let emails: Vec<&str> = json["attendees"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["email"].as_str().unwrap())
            .collect();
        assert_eq!(emails, ["mgaudin@gladia.io", "alice@example.com"]);
    }
}
