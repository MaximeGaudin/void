use std::time::Instant;

/// Lightweight backfill progress tracker with rate-based ETA.
///
/// Prints a human-readable status line to stderr at most once per
/// `interval` (default 2 s), keeping output readable when multiple
/// connectors run in parallel.
pub struct BackfillProgress {
    connector: String,
    start: Instant,
    last_print: Instant,
    interval_secs: f64,

    pub items: u64,
    items_label: String,
    items_total: Option<u64>,

    pub secondary: u64,
    secondary_label: String,

    pub pages_done: u64,
    pages_total: Option<u64>,
}

impl BackfillProgress {
    pub fn new(connector: &str, items_label: &str) -> Self {
        let epoch = Instant::now() - std::time::Duration::from_secs(120);
        Self {
            connector: connector.into(),
            start: Instant::now(),
            last_print: epoch,
            interval_secs: 2.0,
            items: 0,
            items_label: items_label.into(),
            items_total: None,
            secondary: 0,
            secondary_label: String::new(),
            pages_done: 0,
            pages_total: None,
        }
    }

    pub fn with_secondary(mut self, label: &str) -> Self {
        self.secondary_label = label.into();
        self
    }

    pub fn set_items_total(&mut self, total: u64) {
        self.items_total = Some(total);
    }

    pub fn set_pages(&mut self, total: u64) {
        self.pages_total = Some(total);
    }

    pub fn inc_page(&mut self) {
        self.pages_done += 1;
        self.maybe_print();
    }

    pub fn inc(&mut self, n: u64) {
        self.items += n;
        self.maybe_print();
    }

    pub fn inc_secondary(&mut self, n: u64) {
        self.secondary += n;
        self.maybe_print();
    }

    fn maybe_print(&mut self) {
        if self.last_print.elapsed().as_secs_f64() >= self.interval_secs {
            self.print();
        }
    }

    fn print(&mut self) {
        self.last_print = Instant::now();
        let elapsed = self.start.elapsed().as_secs_f64();

        let mut parts = Vec::with_capacity(6);

        if let Some(total) = self.pages_total {
            parts.push(format!("page {}/{total}", self.pages_done));
        }

        if let Some(total) = self.items_total {
            parts.push(format!("{}/{} {}", self.items, total, self.items_label));
        } else {
            parts.push(format!("{} {}", self.items, self.items_label));
        }

        if !self.secondary_label.is_empty() && self.secondary > 0 {
            parts.push(format!("{} {}", self.secondary, self.secondary_label));
        }

        if elapsed > 0.5 && self.items > 0 {
            let rate = self.items as f64 / elapsed;
            let unit = rate_unit(&self.items_label);
            parts.push(format!("{:.0} {unit}/s", rate));
        }

        if let Some(eta) = self.estimate_remaining(elapsed) {
            parts.push(format!("ETA {}", format_duration(eta)));
        }

        let body = parts.join(" · ");
        eprintln!("[{}] {body}", self.connector);
    }

    fn estimate_remaining(&self, elapsed: f64) -> Option<f64> {
        if elapsed < 0.5 || self.items == 0 {
            return None;
        }
        // Page-based ETA takes priority when available
        if let Some(total) = self.pages_total {
            if self.pages_done > 0 && self.pages_done < total {
                let per_page = elapsed / self.pages_done as f64;
                return Some((total - self.pages_done) as f64 * per_page);
            }
        }
        // Item-based ETA when we know the total items
        if let Some(total) = self.items_total {
            if self.items < total {
                let per_item = elapsed / self.items as f64;
                return Some((total - self.items) as f64 * per_item);
            }
        }
        None
    }

    pub fn finish(&self) {
        let elapsed = self.start.elapsed().as_secs_f64();

        let mut parts = Vec::with_capacity(4);
        parts.push(format!("{} {}", self.items, self.items_label));

        if !self.secondary_label.is_empty() && self.secondary > 0 {
            parts.push(format!("{} {}", self.secondary, self.secondary_label));
        }

        parts.push(format!("in {}", format_duration(elapsed)));

        let body = parts.join(" · ");
        eprintln!("[{}] done · {body}", self.connector);
    }
}

/// Shorten a plural label to a compact rate unit: "conversations" → "conv", etc.
fn rate_unit(label: &str) -> &str {
    match label {
        "conversations" => "conv",
        "messages" => "msg",
        "events" => "ev",
        other => other,
    }
}

fn format_duration(secs: f64) -> String {
    let secs = secs.ceil() as u64;
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(0.0), "0s");
        assert_eq!(format_duration(1.0), "1s");
        assert_eq!(format_duration(59.0), "59s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60.0), "1m00s");
        assert_eq!(format_duration(90.0), "1m30s");
        assert_eq!(format_duration(3599.0), "59m59s");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(3600.0), "1h00m");
        assert_eq!(format_duration(5400.0), "1h30m");
    }

    #[test]
    fn format_duration_rounds_up() {
        assert_eq!(format_duration(0.1), "1s");
        assert_eq!(format_duration(59.1), "1m00s");
    }

    #[test]
    fn progress_new_returns_valid_state() {
        let p = BackfillProgress::new("slack", "messages");
        assert_eq!(p.items, 0);
        assert_eq!(p.secondary, 0);
        assert_eq!(p.pages_done, 0);
        assert!(p.pages_total.is_none());
    }

    #[test]
    fn progress_inc_updates_count() {
        let mut p = BackfillProgress::new("test", "items");
        p.inc(5);
        assert_eq!(p.items, 5);
        p.inc(3);
        assert_eq!(p.items, 8);
    }

    #[test]
    fn progress_secondary_tracks_independently() {
        let mut p = BackfillProgress::new("test", "conversations").with_secondary("messages");
        p.inc(1);
        p.inc_secondary(50);
        assert_eq!(p.items, 1);
        assert_eq!(p.secondary, 50);
    }

    #[test]
    fn progress_pages() {
        let mut p = BackfillProgress::new("gmail", "messages");
        p.set_pages(5);
        p.inc_page();
        assert_eq!(p.pages_done, 1);
        assert_eq!(p.pages_total, Some(5));
    }

    #[test]
    fn estimate_remaining_none_without_totals() {
        let mut p = BackfillProgress::new("test", "items");
        p.start = Instant::now() - std::time::Duration::from_secs(10);
        p.items = 100;
        assert!(p.estimate_remaining(10.0).is_none());
    }

    #[test]
    fn estimate_remaining_with_pages() {
        let mut p = BackfillProgress::new("test", "items");
        p.set_pages(4);
        p.pages_done = 2;
        p.items = 50;
        let eta = p.estimate_remaining(10.0).unwrap();
        assert!((eta - 10.0).abs() < 0.01);
    }

    #[test]
    fn estimate_remaining_with_items_total() {
        let mut p = BackfillProgress::new("test", "conversations");
        p.set_items_total(100);
        p.items = 50;
        let eta = p.estimate_remaining(10.0).unwrap();
        assert!((eta - 10.0).abs() < 0.01);
    }

    #[test]
    fn estimate_remaining_pages_takes_priority_over_items() {
        let mut p = BackfillProgress::new("test", "items");
        p.set_pages(4);
        p.pages_done = 1;
        p.set_items_total(400);
        p.items = 200;
        // Pages: 3 remaining × (10/1) = 30s
        // Items: 200 remaining × (10/200) = 10s
        // Pages should win
        let eta = p.estimate_remaining(10.0).unwrap();
        assert!((eta - 30.0).abs() < 0.01);
    }

    #[test]
    fn rate_unit_shortens_labels() {
        assert_eq!(rate_unit("conversations"), "conv");
        assert_eq!(rate_unit("messages"), "msg");
        assert_eq!(rate_unit("events"), "ev");
        assert_eq!(rate_unit("widgets"), "widgets");
    }
}
