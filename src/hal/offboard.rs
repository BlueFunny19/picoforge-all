//! Offboard receipt — the report of a full-device wipe plus its signed
//! checkpoint. Pure data + JSON rendering, so it is host-tested; the wipe
//! orchestration lives in [`crate::hal::io`].

/// One wipe step's outcome.
#[derive(Debug, Clone)]
pub struct OffboardStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// The full offboard receipt.
#[derive(Debug, Clone)]
pub struct OffboardReport {
    pub serial: String,
    pub steps: Vec<OffboardStep>,
    pub signed: bool,
    pub fingerprint: Option<String>,
    pub signed_head: Option<String>,
    pub signature: Option<String>,
    pub pubkey: Option<String>,
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl OffboardReport {
    /// Whether every wipe step succeeded.
    pub fn all_ok(&self) -> bool {
        self.steps.iter().all(|s| s.ok)
    }

    /// The names of the failed steps.
    pub fn failures(&self) -> Vec<&str> {
        self.steps
            .iter()
            .filter(|s| !s.ok)
            .map(|s| s.name.as_str())
            .collect()
    }

    /// Render the receipt as pretty JSON. `timestamp` is supplied by the caller
    /// (this module has no clock).
    pub fn to_json(&self, timestamp: &str) -> String {
        let mut steps = String::from("{");
        for (i, s) in self.steps.iter().enumerate() {
            if i > 0 {
                steps.push(',');
            }
            steps.push_str(&format!(
                "\n    {}: {}",
                json_str(&s.name),
                json_str(&s.detail)
            ));
        }
        steps.push_str("\n  }");

        let mut out = String::from("{\n");
        out.push_str(&format!("  \"device\": {},\n", json_str(&self.serial)));
        out.push_str(&format!("  \"timestamp\": {},\n", json_str(timestamp)));
        out.push_str(&format!("  \"steps\": {},\n", steps));
        out.push_str(&format!("  \"signed\": {}", self.signed));
        if self.signed {
            let f = |o: &Option<String>| o.clone().unwrap_or_default();
            out.push_str(&format!(
                ",\n  \"fingerprint\": {}",
                json_str(&f(&self.fingerprint))
            ));
            out.push_str(&format!(
                ",\n  \"signed_head\": {}",
                json_str(&f(&self.signed_head))
            ));
            out.push_str(&format!(
                ",\n  \"signature\": {}",
                json_str(&f(&self.signature))
            ));
            out.push_str(&format!(
                ",\n  \"attestation_pubkey\": {}",
                json_str(&f(&self.pubkey))
            ));
        }
        out.push_str("\n}\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(name: &str, ok: bool) -> OffboardStep {
        OffboardStep {
            name: name.into(),
            ok,
            detail: if ok { "ok".into() } else { "boom".into() },
        }
    }

    #[test]
    fn all_ok_and_failures() {
        let r = OffboardReport {
            serial: "123".into(),
            steps: vec![step("otp", true), step("piv", false)],
            signed: false,
            fingerprint: None,
            signed_head: None,
            signature: None,
            pubkey: None,
        };
        assert!(!r.all_ok());
        assert_eq!(r.failures(), vec!["piv"]);
    }

    #[test]
    fn json_has_fields_and_escapes() {
        let r = OffboardReport {
            serial: "37302053".into(),
            steps: vec![step("otp", true)],
            signed: true,
            fingerprint: Some("abcd".into()),
            signed_head: Some("dead".into()),
            signature: Some("beef".into()),
            pubkey: Some("04aa".into()),
        };
        let j = r.to_json("2026-07-21T00:00:00");
        assert!(j.contains("\"device\": \"37302053\""));
        assert!(j.contains("\"signed\": true"));
        assert!(j.contains("\"fingerprint\": \"abcd\""));
        assert!(j.contains("\"attestation_pubkey\": \"04aa\""));
        // A quote in a detail string must be escaped.
        let bad = OffboardStep {
            name: "x".into(),
            ok: false,
            detail: "a\"b".into(),
        };
        let r2 = OffboardReport {
            steps: vec![bad],
            ..r
        };
        assert!(r2.to_json("t").contains("a\\\"b"));
    }
}
