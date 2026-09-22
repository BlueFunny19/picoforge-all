//! English source catalog and Simplified Chinese translations.
use std::{
    collections::BTreeMap,
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};
static CHINESE: AtomicBool = AtomicBool::new(false);
static EN: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/en_US.json")).expect("English catalog")
});
static ZH: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/zh_CN.json")).expect("Chinese catalog")
});
pub fn set_chinese(chinese: bool) {
    CHINESE.store(chinese, Ordering::Relaxed);
    gpui_component::set_locale(if chinese { "zh-CN" } else { "en" });
}
pub fn chinese() -> bool {
    CHINESE.load(Ordering::Relaxed)
}
pub fn tr(source: &'static str) -> &'static str {
    let catalog = if chinese() { &*ZH } else { &*EN };
    catalog.get(source).map(String::as_str).unwrap_or(source)
}
/// Translate a known UI label; never use this for user content or secrets.
pub fn text(source: impl AsRef<str>) -> String {
    let canonical = if chinese() {
        source.as_ref().to_owned()
    } else {
        source_label(source.as_ref())
    };
    let source = canonical.as_str();
    let catalog = if chinese() { &*ZH } else { &*EN };
    if let Some(value) = catalog.get(source) {
        return value.clone();
    }
    if chinese() {
        for (template, translated) in ZH
            .iter()
            .filter(|(key, _)| key.find('{').is_some_and(|n| n >= 3))
        {
            if let Some(values) = capture(template, source) {
                return substitute(translated, &values);
            }
        }
    }
    source.to_owned()
}
/// Recover the source label for dropdown items retained across a language switch.
pub fn source_label(label: &str) -> String {
    if let Some((key, _)) = ZH.iter().find(|(_, value)| value.as_str() == label) {
        return key.clone();
    }
    if !label.is_ascii() {
        for (key, value) in ZH
            .iter()
            .filter(|(key, value)| key != value && value.find('{').is_some_and(|n| n >= 3))
        {
            if let Some(values) = capture(value, label) {
                return substitute(key, &values);
            }
        }
    }
    label.to_owned()
}

fn capture(template: &str, source: &str) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut pattern = template;
    let mut input = source;
    while let Some(start) = pattern.find('{') {
        input = input.strip_prefix(&pattern[..start])?;
        let end = pattern[start + 1..].find('}')? + start + 1;
        let index: usize = pattern[start + 1..end].parse().ok()?;
        if index > 15 {
            return None;
        }
        pattern = &pattern[end + 1..];
        let literal = pattern.split('{').next().unwrap_or_default();
        let count = if literal.is_empty() {
            if pattern.is_empty() {
                input.len()
            } else {
                return None;
            }
        } else {
            input.find(literal)?
        };
        values.resize(values.len().max(index + 1), String::new());
        values[index] = input[..count].into();
        input = &input[count..];
    }
    (input == pattern).then_some(values)
}
/// Substitute formatted values into a translated template, without translating values.
pub fn format(source: &'static str, values: &[String]) -> String {
    substitute(tr(source), values)
}
fn substitute(template: &str, values: &[String]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        if let Some(end) = tail.find('}') {
            if let Ok(index) = tail[..end].parse::<usize>() {
                if let Some(value) = values.get(index) {
                    out.push_str(value);
                    rest = &tail[end + 1..];
                    continue;
                }
            }
        }
        out.push('{');
        rest = tail;
    }
    out.push_str(rest);
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates_preserve_values_and_reject_unrelated_text() {
        let values = capture("Failed to read {0}: {1}", "Failed to read a{b}.pem: denied").unwrap();
        assert_eq!(
            substitute("读取 {0} 失败：{1}", &values),
            "读取 a{b}.pem 失败：denied"
        );
        assert!(capture("Invalid {0}", "Valid key").is_none());
    }
    #[test]
    fn catalogs_have_equal_keys_and_placeholders() {
        assert_eq!(EN.keys().collect::<Vec<_>>(), ZH.keys().collect::<Vec<_>>());
        for (key, value) in ZH.iter() {
            assert!(!value.trim().is_empty(), "{key}");
            assert!(!value.contains('。'), "Chinese sentence punctuation: {key}");
            for n in 0..16 {
                let token = std::format!("{{{n}}}");
                assert_eq!(
                    key.matches(&token).count(),
                    value.matches(&token).count(),
                    "{key}"
                );
            }
        }
    }
}

thread_local! {
    static INPUT_LABELS: std::cell::RefCell<Vec<(gpui::WeakEntity<gpui_component::input::InputState>, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}
pub trait LocalizedPlaceholder: Sized {
    fn localized_placeholder(
        self,
        source: impl Into<gpui::SharedString>,
        cx: &mut gpui::Context<Self>,
    ) -> Self;
}
impl LocalizedPlaceholder for gpui_component::input::InputState {
    fn localized_placeholder(
        self,
        source: impl Into<gpui::SharedString>,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let source = source_label(source.into().as_ref());
        INPUT_LABELS.with_borrow_mut(|labels| {
            labels.retain(|(input, _)| input.upgrade().is_some());
            labels.push((cx.entity().downgrade(), source.clone()));
        });
        self.placeholder(text(source))
    }
}
pub fn refresh_inputs(window: &mut gpui::Window, cx: &mut gpui::App) {
    INPUT_LABELS.with_borrow_mut(|labels| {
        labels.retain(|(input, source)| {
            if let Some(input) = input.upgrade() {
                input.update(cx, |input, cx| {
                    input.set_placeholder(text(source), window, cx)
                });
                true
            } else {
                false
            }
        });
    });
}
