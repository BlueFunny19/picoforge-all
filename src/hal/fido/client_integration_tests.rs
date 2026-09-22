//! The current PicoForge request encoder, response decoder and verifier against
//! the real Pico All C protocol handlers. Only USB and Flash are replaced.
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Firmware {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Firmware {
    fn new() -> Self {
        let args: Vec<String> = serde_json::from_str(
            &std::env::var("PICOFORGE_PROTOCOL_COMMAND")
                .expect("Set PICOFORGE_PROTOCOL_COMMAND to a JSON argv"),
        )
        .unwrap();
        let mut child = Command::new(&args[0])
            .args(&args[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }
    fn line(&mut self, request: &str) -> String {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        assert!(
            self.output.read_line(&mut line).unwrap() > 0,
            "firmware exited"
        );
        line.trim().to_string()
    }
    fn request(
        &mut self,
        cmd: u8,
        params: Option<Value>,
        token: Option<&[u8]>,
    ) -> (u8, Option<Value>) {
        let bytes = ops::vendor_payload(cmd, params, token).unwrap();
        let response = self.line(&hex::encode(bytes));
        let (status, data) = response.split_once(' ').unwrap_or((&response, ""));
        let mut raw = vec![status.parse::<u8>().unwrap()];
        raw.extend(hex::decode(data).unwrap());
        ops::vendor_response(&raw).unwrap()
    }
    fn target(&mut self, target: i128, token: Option<&[u8]>) -> (u8, Option<Value>) {
        self.request(
            RSKEY_VENDOR_AUDIT_CONFIG,
            Some(Value::Map(BTreeMap::from([(
                Value::Integer(1),
                Value::Integer(target),
            )]))),
            token,
        )
    }
    fn journal(&mut self, token: Option<&[u8]>) -> audit::AuditJournal {
        let (status, map) = self.request(RSKEY_VENDOR_AUDIT_READ, None, token);
        parse_journal_response(status, map).unwrap()
    }
}
impl Drop for Firmware {
    fn drop(&mut self) {
        drop(self.input.take());
        assert!(self.child.wait().unwrap().success());
    }
}
#[test]
#[ignore = "requires the local production C protocol executable; no hardware"]
fn current_client_audit_roundtrip() {
    let mut fw = Firmware::new();
    let (status, map) = fw.target(2, None);
    assert!(!m_bool(&vendor_map(status, map, "status").unwrap(), 1));
    fw.line("append 5");
    assert!(fw.journal(None).entries.is_empty());
    fw.line("touch 1");
    assert_eq!(fw.target(1, None).0, 0x2f);
    fw.line("touch 0");
    assert_eq!(fw.target(1, None).0, 0);
    let first = fw.journal(None);
    assert_eq!(
        first.entries.iter().map(|e| e.event).collect::<Vec<_>>(),
        [1, 0x16]
    );
    fw.line("pin 1");
    assert_ne!(fw.request(RSKEY_VENDOR_AUDIT_READ, None, None).0, 0);
    assert_ne!(
        fw.request(RSKEY_VENDOR_AUDIT_READ, None, Some(&[0x43; 32]))
            .0,
        0
    );
    let token = [0x42; 32];
    fw.line("append 140");
    let before = fw.journal(Some(&token));
    assert_eq!(before.entries.len(), 128);
    assert_eq!(before.start, 14);
    fw.line("run 1000");
    let journal = fw.journal(Some(&token));
    assert_eq!(journal.entries.len(), 128);
    assert_eq!(journal.seq_next, before.seq_next + 1);
    let challenge = vec![0xa5; 16];
    let (status, map) = fw.request(
        RSKEY_VENDOR_AUDIT_CHECKPOINT,
        Some(Value::Map(BTreeMap::from([(
            Value::Integer(1),
            Value::Bytes(challenge.clone()),
        )]))),
        Some(&token),
    );
    let m = vendor_map(status, map, "checkpoint").unwrap();
    let head = m_bytes(&m, 1).unwrap();
    let seq = m_int(&m, 2).unwrap() as u32;
    let sig = m_bytes(&m, 3).unwrap();
    let pubkey = m_bytes(&m, 4).unwrap();
    assert_eq!(head, journal.head);
    assert_eq!(seq, journal.seq_next);
    assert!(audit::verify_checkpoint(
        &head, seq, &sig, &pubkey, &challenge
    ));
    assert!(!audit::verify_checkpoint(
        &head,
        seq,
        &sig,
        &pubkey,
        &[0xa6; 16]
    ));
    assert_eq!(fw.journal(Some(&token)).entries.last().unwrap().event, 0x11);
    // The actual reset handler folds details, preserves opt-in and appends RESET.
    assert_eq!(fw.line("07"), "0");
    let after = fw.journal(None);
    assert_eq!(after.entries.len(), 1);
    assert_eq!(after.entries[0].event, 4);
    assert!(m_bool(
        &vendor_map(fw.target(2, None).0, fw.target(2, None).1, "status").unwrap(),
        1
    ));
    assert_eq!(fw.target(0, None).0, 0);
    let stopped = fw.journal(None).seq_next;
    fw.line("append 4");
    assert_eq!(fw.journal(None).seq_next, stopped);
}
#[test]
fn ea_success_requires_readback_true() {
    for enabled in [None, Some(false), Some(true)] {
        let options = enabled
            .map(|on| BTreeMap::from([(Value::Text("ep".into()), Value::Bool(on))]))
            .unwrap_or_default();
        let info = parse_fido_get_info(&Value::Map(BTreeMap::from([(
            Value::Integer(4),
            Value::Map(options),
        )])))
        .unwrap();
        assert_eq!(
            verify_enterprise_attestation_enabled(&info).is_ok(),
            enabled == Some(true)
        );
    }
}
