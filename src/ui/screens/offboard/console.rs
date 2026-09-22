//! Timestamped console records, shared by UI reads and native firmware output.
use std::collections::VecDeque;

pub(super) const LEVELS: [&str; 5] = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

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
        let Some(level) = normalize_level(level) else {
            return;
        };
        self.append(Entry {
            timestamp: crate::logging::local_timestamp(),
            level: level.into(),
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
            let Some(level) = normalize_level(level) else {
                return;
            };
            self.append(Entry {
                timestamp: time.into(),
                level: level.into(),
                message: message.into(),
            });
        }
    }
}

fn normalize_level(level: &str) -> Option<&str> {
    match level {
        "SUCCESS" => Some("INFO"),
        "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR" => Some(level),
        _ => None,
    }
}
fn priority(level: &str) -> u8 {
    match level {
        "ERROR" => 4,
        "WARN" => 3,
        "INFO" => 2,
        "DEBUG" => 1,
        _ => 0,
    }
}
impl Entry {
    pub fn visible(&self, minimum: &str) -> bool {
        priority(&self.level) >= priority(minimum)
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
    fn levels_filter_messages_without_retaining_raw_output() {
        let mut log = Console::default();
        for level in LEVELS {
            log.push(level, level);
        }
        for (index, level) in LEVELS.iter().enumerate() {
            assert_eq!(
                log.entries.iter().filter(|e| e.visible(level)).count(),
                index + 1
            );
        }
        for _ in 0..601 {
            log.push("picotool", "raw bytes");
            log.worker("[12:34:56] [OUTPUT] raw bytes".into());
            log.worker("unprefixed output".into());
        }
        assert_eq!(log.entries.len(), LEVELS.len());
        log.clear();
        assert!(log.entries.is_empty());
    }

    #[test]
    fn worker_time_and_multiline_output_are_preserved() {
        let mut log = Console::default();
        log.worker("[12:34:56 +08:00] [INFO] serial:\n  432D921975CCC729".into());
        let entry = log.entries.front().unwrap();
        assert_eq!(entry.timestamp, "12:34:56 +08:00");
        assert_eq!(entry.level, "INFO");
        assert_eq!(entry.message, "serial:\n  432D921975CCC729");
        log.worker("[12:34:57 +08:00] [SUCCESS] Complete".into());
        assert_eq!(log.entries.back().unwrap().level, "INFO");
        for _ in 0..601 {
            log.push("INFO", "é中文");
        }
        assert_eq!(log.entries.len(), 600);
        let original = "中".repeat(80);
        assert_eq!(wrap_tokens(&original).replace('\u{200b}', ""), original);
    }
}
