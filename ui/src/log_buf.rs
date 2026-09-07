//! Capped audit log buffer and virtualized viewport (no GPUI).
#![forbid(unsafe_code)]

use std::collections::VecDeque;

use interfire_proto::MAX_LOG_RECORDS_PER_SUBSCRIBER;

/// Cap aligned with `MAX_LOG_RECORDS_PER_SUBSCRIBER` (product UX contract).
pub const MAX_AUDIT_LINES: usize = MAX_LOG_RECORDS_PER_SUBSCRIBER;

/// Default Log pane viewport height in rows.
pub const DEFAULT_VIEWPORT_ROWS: usize = 24;

/// One visible window into the capped audit buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibleLog {
    pub relative_selected: usize,
    pub items: Vec<String>,
    pub start: usize,
    pub total: usize,
}

/// UI-owned audit rows: at most [`MAX_AUDIT_LINES`], virtualized for display.
#[derive(Clone, Debug, Default)]
pub struct LogBuffer {
    lines: VecDeque<String>,
    selected: usize,
    subscribed: bool,
}

impl LogBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn subscribed(&self) -> bool {
        self.subscribed
    }

    pub const fn set_subscribed(&mut self, ready: bool) {
        self.subscribed = ready;
    }

    /// Append one audit line (`SEQ|message`); drop oldest when at cap.
    pub fn push_line(&mut self, sequence: u64, message: &str) {
        if self.lines.len() == MAX_AUDIT_LINES {
            self.lines.pop_front();
            if self.selected > 0 {
                self.selected -= 1;
            }
        }
        self.lines.push_back(format!("{sequence}|{message}"));
        self.clamp_selection();
    }

    pub fn select(&mut self, index: usize) {
        if self.lines.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = index.min(self.lines.len() - 1);
    }

    fn clamp_selection(&mut self) {
        if self.lines.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.lines.len() {
            self.selected = self.lines.len() - 1;
        }
    }

    /// Visible slice around the selection for the given viewport height.
    #[must_use]
    pub fn visible_window(&self, viewport_rows: usize) -> VisibleLog {
        let total = self.lines.len();
        if total == 0 {
            return VisibleLog {
                relative_selected: 0,
                items: Vec::new(),
                start: 0,
                total: 0,
            };
        }
        let height = viewport_rows.max(1);
        let selected = self.selected.min(total - 1);
        let start = if total <= height {
            0
        } else {
            let half = height / 2;
            selected
                .saturating_sub(half)
                .min(total.saturating_sub(height))
        };
        let end = (start + height).min(total);
        VisibleLog {
            relative_selected: selected - start,
            items: self
                .lines
                .iter()
                .skip(start)
                .take(end - start)
                .cloned()
                .collect(),
            start,
            total,
        }
    }

    /// Selected line for the detail pane, if any.
    #[must_use]
    pub fn selected_line(&self) -> Option<&str> {
        self.lines.get(self.selected).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_drops_oldest_and_keeps_limit() {
        let mut buf = LogBuffer::new();
        for sequence in 1..=(MAX_AUDIT_LINES as u64 + 500) {
            buf.push_line(sequence, "m");
        }
        assert_eq!(buf.visible_window(10).total, MAX_AUDIT_LINES);
        let window = buf.visible_window(10);
        assert_eq!(window.items.len(), 10);
    }

    #[test]
    fn viewport_centers_near_selection() {
        let mut buf = LogBuffer::new();
        for sequence in 1..=40 {
            buf.push_line(sequence, &format!("m{sequence}"));
        }
        buf.select(30);
        let window = buf.visible_window(5);
        assert_eq!(window.items.len(), 5);
        assert_eq!(window.total, 40);
        assert!(window.start <= 30);
        assert_eq!(window.relative_selected, 30 - window.start);
    }

    #[test]
    fn subscription_flag_toggles_without_growth() {
        let mut buf = LogBuffer::new();
        buf.set_subscribed(true);
        assert!(buf.subscribed());
        buf.set_subscribed(false);
        assert!(!buf.subscribed());
        buf.set_subscribed(true);
        for sequence in 1..=50 {
            buf.push_line(sequence, "x");
        }
        assert!(buf.visible_window(DEFAULT_VIEWPORT_ROWS).total <= MAX_AUDIT_LINES);
    }
}
