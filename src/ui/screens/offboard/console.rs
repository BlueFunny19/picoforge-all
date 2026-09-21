//! Timestamped console records, shared by UI reads and native firmware output.
use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub(super) struct Entry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}
#[derive(Default)]
pub(super) struct Console {
    pub entries: VecDeque<Entry>,
}
impl Console {
    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn push(&mut self, level: &str, message: impl Into<String>) {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            % 86400;
        self.append(Entry {
            timestamp: format!(
                "{:02}:{:02}:{:02} UTC",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            ),
            level: level.to_string(),
            message: message.into(),
        });
    }
    fn append(&mut self, entry: Entry) {
        self.entries.push_back(entry);
        while self.entries.len() > 600 {
            self.entries.pop_front();
        }
    }
    pub fn worker(&mut self, line: String) {
        // Preserve the worker's event time, including multiline tool output.
        if let Some((time, rest)) = line.strip_prefix('[').and_then(|s| s.split_once("] ["))
            && let Some((level, message)) = rest.split_once("] ")
        {
            self.append(Entry {
                timestamp: time.into(),
                level: level.into(),
                message: message.into(),
            });
        } else {
            self.push("OUTPUT", line);
        }
    }
}

/// Long hashes and paths can wrap naturally, without splitting every line at an arbitrary width.
pub(super) fn wrap_tokens(text: &str) -> String {
    let mut result = String::new();
    let mut run = 0;
    for c in text.chars() {
        if c.is_whitespace() {
            run = 0;
        } else {
            if run == 32 {
                result.push('\u{200b}');
                run = 0;
            }
            run += 1;
        }
        result.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_time_and_multiline_output_are_preserved() {
        let mut log = Console::default();
        log.worker("[12:34:56 UTC] [picotool] serial:\n  432D921975CCC729".into());
        let entry = log.entries.front().unwrap();
        assert_eq!(entry.timestamp, "12:34:56 UTC");
        assert_eq!(entry.level, "picotool");
        assert_eq!(entry.message, "serial:\n  432D921975CCC729");
        log.worker("unprefixed output".into());
        assert_eq!(log.entries.back().unwrap().level, "OUTPUT");
        for _ in 0..601 {
            log.push("INFO", "é中文");
        }
        assert_eq!(log.entries.len(), 600);
        let original = "中".repeat(80);
        assert_eq!(wrap_tokens(&original).replace('\u{200b}', ""), original);
    }
}
