//! RP2350 boot policy ported from Pico All firmware.py; no Python runtime.
use super::*;
use serde_json::{Value, json};
const CRIT: [u16; 8] = [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
const FLAGS: [u16; 3] = [0x4b, 0x4c, 0x4d];
const LOCKS: [u16; 2] = [0xf83, 0xf85];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct State {
    critical: Vec<u32>,
    flags: Vec<u32>,
    locks: Vec<u32>,
    key_locks: Vec<u32>,
    fps: Vec<Option<String>>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
struct Write {
    row: u16,
    value: u32,
    ecc: bool,
}
fn vote(values: &[u32], threshold: usize) -> u32 {
    (0..24)
        .filter(|b| values.iter().filter(|v| **v & (1 << b) != 0).count() >= threshold)
        .fold(0, |v, b| v | (1 << b))
}
fn lock_vote(value: u32) -> u32 {
    vote(&[value & 255, (value >> 8) & 255, (value >> 16) & 255], 2)
}
pub(super) fn read(w: &Worker, row: u16, ecc: bool) -> Result<u32, String> {
    let text = w.device_command(&[
        "otp",
        "get",
        "-c",
        "1",
        if ecc { "-e" } else { "-r" },
        "-n",
        &format!("0x{row:x}"),
    ])?;
    parse_row(&text, row, ecc)
}
fn parse_row(text: &str, row: u16, ecc: bool) -> Result<u32, String> {
    let words: Vec<_> = text.split_whitespace().collect();
    let vals: Vec<_> = words
        .windows(2)
        .filter(|p| p[0] == "VALUE")
        .map(|p| p[1])
        .collect();
    let rows: Vec<_> = words
        .windows(2)
        .filter(|p| p[0] == "ROW")
        .map(|p| p[1].trim_end_matches(':'))
        .collect();
    let number = |v: &str| {
        u32::from_str_radix(v.trim_start_matches("0x"), 16)
            .map_err(|_| "Invalid OTP output".to_owned())
    };
    if rows.len() != 1 || vals.len() != 1 || number(rows[0])? != row as u32 {
        return Err("Unexpected OTP row output.".into());
    }
    let value = number(vals[0])?;
    if value > if ecc { 0xffff } else { 0xffffff } {
        return Err("Unexpected OTP value.".into());
    }
    Ok(value)
}
pub(super) fn secure_boot_enabled(
    mut read_word: impl FnMut(u16, bool) -> Result<u32, String>,
) -> Result<bool, String> {
    let critical = CRIT
        .iter()
        .map(|r| read_word(*r, false))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(vote(&critical, 3) & 1 != 0)
}
/// Trust comes from valid, non-revoked OTP slots, including on an empty board.
pub(super) fn trusted_boot_key(
    fingerprint: Option<&str>,
    mut read_word: impl FnMut(u16, bool) -> Result<u32, String>,
) -> Result<Option<String>, String> {
    let Some(fingerprint) = fingerprint else {
        return Ok(None);
    };
    let flags = FLAGS
        .iter()
        .map(|row| read_word(*row, false))
        .collect::<Result<Vec<_>, _>>()?;
    let flags = vote(&flags, 2);
    let active = (flags & 15) & !((flags >> 8) & 15);
    let mut matched = None;
    for slot in 0..4 {
        if active & (1 << slot) == 0 {
            continue;
        }
        let mut bytes = Vec::with_capacity(32);
        for row in 0x80 + slot * 16..0x90 + slot * 16 {
            let word = read_word(row, true)
                .map_err(|e| format!("Cannot verify OTP signing key {slot}: {e}"))?;
            bytes.extend_from_slice(&(word as u16).to_le_bytes());
        }
        if hex::encode(bytes) == fingerprint {
            matched = Some(fingerprint.to_owned());
        }
    }
    Ok(matched)
}
pub(super) fn factory_serial(w: &Worker) -> Result<String, String> {
    let words = (0..4)
        .map(|r| read(w, r, true))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(words.iter().rev().map(|v| format!("{v:04X}")).collect())
}
fn state(w: &Worker) -> Result<State, String> {
    let all = |rows: &[u16]| {
        rows.iter()
            .map(|r| read(w, *r, false))
            .collect::<Result<Vec<_>, _>>()
    };
    let mut s = State {
        critical: all(&CRIT)?,
        flags: all(&FLAGS)?,
        locks: all(&LOCKS)?,
        key_locks: all(&[0xf82, 0xf84])?,
        fps: vec![],
    };
    for slot in 0..4 {
        let words = (0..16)
            .map(|i| read(w, 0x80 + slot * 16 + i, true))
            .collect::<Result<Vec<_>, _>>();
        s.fps.push(words.ok().map(|v| {
            hex::encode(
                v.into_iter()
                    .flat_map(|v| (v as u16).to_le_bytes())
                    .collect::<Vec<_>>(),
            )
        }));
    }
    Ok(s)
}
fn plan(
    s: &State,
    action: &str,
    fp: &str,
    slot: u8,
    mut read_word: impl FnMut(u16, bool) -> Result<u32, String>,
) -> Result<Vec<Write>, String> {
    let bytes = hex::decode(fp).map_err(|_| "Invalid signing key")?;
    if slot > 3 || bytes.len() != 32 || bytes.iter().all(|b| *b == 0) {
        return Err("Invalid boot key or slot.".into());
    }
    if s.key_locks.iter().any(|v| *v != 0) {
        return Err("Boot pages have a hardware access-key policy.".into());
    }
    let flags = vote(&s.flags, 2);
    let trusted = (flags & 15) & !((flags >> 8) & 15);
    let matches: Vec<_> = (0..4)
        .filter(|i| s.fps[*i].as_deref() == Some(fp) && trusted & (1 << i) != 0)
        .collect();
    let mut writes = vec![];
    fn raw(w: &mut Vec<Write>, row: u16, old: u32, bits: u32) {
        if old | bits != old {
            w.push(Write {
                row,
                value: old | bits,
                ecc: false,
            });
        }
    }
    if action == "load-key" {
        if s.flags.iter().any(|v| v & (1 << (slot + 8)) != 0) {
            return Err("Selected key slot is revoked.".into());
        }
        for i in 0..16 {
            let row = 0x80 + slot as u16 * 16 + i;
            let wanted =
                u16::from_le_bytes([bytes[i as usize * 2], bytes[i as usize * 2 + 1]]) as u32;
            if read_word(row, false)? != 0 {
                if read_word(row, true)? != wanted {
                    return Err("Key slot contains different or incomplete data.".into());
                }
            } else if wanted != 0 {
                writes.push(Write {
                    row,
                    value: wanted,
                    ecc: true,
                });
            }
        }
        for (row, old) in FLAGS.iter().zip(&s.flags) {
            raw(&mut writes, *row, *old, 1 << slot);
        }
    } else {
        if matches.len() != 1 {
            return Err("Image must match exactly one trusted OTP signing key.".into());
        }
        match action {
            "harden" => {
                for (r, v) in CRIT.iter().zip(&s.critical) {
                    raw(&mut writes, *r, *v, 0x74);
                }
            }
            "enable" => {
                if s.critical.iter().any(|v| v & 0x74 != 0x74)
                    || s.flags.iter().any(|v| v & (1 << matches[0]) == 0)
                {
                    return Err("Complete key registration and hardening first.".into());
                }
                for (r, v) in CRIT.iter().zip(&s.critical) {
                    raw(&mut writes, *r, *v, 1);
                }
            }
            "lock" => {
                if s.critical.iter().any(|v| v & 0x75 != 0x75) || trusted != 1 << matches[0] {
                    return Err("Lock requires hardened Secure Boot and one trusted key.".into());
                }
                for (r, v) in FLAGS.iter().zip(&s.flags) {
                    raw(&mut writes, *r, *v, (15 ^ trusted) << 8);
                }
                for (r, v) in LOCKS.iter().zip(&s.locks) {
                    if v & !0x151515 != 0 {
                        return Err("Incompatible existing boot locks.".into());
                    }
                    if lock_vote(*v) != 0x15 {
                        raw(&mut writes, *r, *v, 0x151515);
                    }
                }
            }
            _ => return Err("Unknown security stage.".into()),
        }
    }
    for wr in &writes {
        let idx = usize::from((0x80..0xc0).contains(&wr.row) || wr.row == 0xf85);
        if lock_vote(s.locks[idx]) & 0x30 != 0 {
            return Err("BOOTSEL cannot write this locked page.".into());
        }
    }
    Ok(writes)
}
fn state_dir() -> Result<PathBuf, String> {
    let dirs = directories::ProjectDirs::from("org", "PicoForge", "PicoForge All")
        .ok_or("No application data directory")?;
    fs::create_dir_all(dirs.data_local_dir()).map_err(|e| e.to_string())?;
    Ok(dirs.data_local_dir().into())
}
fn proof_path(serial: &str) -> Result<PathBuf, String> {
    Ok(state_dir()?.join(format!("boot-check-{serial}.json")))
}
fn empty_storage(w: &Worker, path: &Path) -> Result<(), String> {
    let text = w.command(&["info", "-m", &path.to_string_lossy()])?;
    let line = text
        .lines()
        .find(|l| l.trim().starts_with("partition 1 ("))
        .ok_or("Cannot identify credential partition")?;
    let span = line
        .split_once(':')
        .ok_or("Invalid partition")?
        .1
        .split_whitespace()
        .next()
        .ok_or("Missing partition extent")?;
    let (a, b) = span.split_once("->").ok_or("Invalid partition extent")?;
    let start = u32::from_str_radix(a, 16).map_err(|_| "Invalid partition start")?;
    let end = u32::from_str_radix(b, 16).map_err(|_| "Invalid partition end")?;
    if start < 0x102000 || end != 0x400000 || start >= end - 0x2000 || start % 4096 != 0 {
        return Err("Unsupported credential storage layout.".into());
    }
    let tmp = state_dir()?.join(format!("storage-check-{}.bin", std::process::id()));
    if tmp.exists() {
        return Err("A previous storage check file exists.".into());
    }
    let result = (|| {
        w.device_command(&[
            "save",
            "-r",
            &format!("0x{:x}", start + 0x10000000),
            &format!("0x{:x}", end + 0x10000000 - 0x2000),
            &tmp.to_string_lossy(),
            "-t",
            "bin",
        ])?;
        let data = fs::read(&tmp).map_err(|e| e.to_string())?;
        if data.len() != (end - start - 0x2000) as usize || data.iter().any(|v| *v != 255) {
            return Err(
                "Credential storage is not empty. Prepare storage before enabling Secure Boot."
                    .into(),
            );
        }
        Ok(())
    })();
    let _ = fs::remove_file(tmp);
    result
}
pub(super) fn execute(w: &Worker, r: &Request) -> Result<Option<Value>, String> {
    if r.action == "prepare" {
        let d = management(&w.serial, &[0x80, 0x1e, 6, 0, 0])?;
        if d.len() != 7 || d[0] != 1 || d[1] != 0 || d[6] & 1 != 0 {
            return Err("Preparation requires Secure Boot and OTP root to be off.".into());
        }
        if r.phrase != format!("ERASE {}", w.serial) {
            return Err("Confirm erasing application storage first.".into());
        }
        w.log(
            "WARN",
            "Press the board button to erase application credentials and enter update mode.",
        );
        management(&w.serial, &[0x80, 0x1d, 0, 2, 0])?;
        return Ok(None);
    }
    if r.action == "prove" {
        let path = image_path(&r.firmware)?;
        let image = w.image(&path)?;
        if !image.signed {
            return Err("Select verified signed firmware.".into());
        }
        let d = management(&w.serial, &[0x80, 0x1e, 6, 0, 0])?;
        if d.len() != 7
            || d[0] != 1
            || d[1] != 1
            || !(56..59).contains(&d[2])
            || d[6] & 0x75 != 0x75
        {
            return Err("Protected boot and OTP root are not active.".into());
        }
        let proof = json!({"serial":w.serial,"sha256":image.hash,"critical":u32::from_be_bytes(d[3..7].try_into().unwrap()),"root_page":d[2]});
        fs::write(proof_path(&w.serial)?, serde_json::to_vec(&proof).unwrap())
            .map_err(|e| e.to_string())?;
        w.log("INFO", "Protected boot verified and saved.");
        return Ok(None);
    }
    if !matches!(
        r.action.as_str(),
        "status" | "load-key" | "harden" | "enable" | "lock"
    ) {
        return Err("Unknown firmware operation.".into());
    }
    w.ensure_bootsel()?;
    let board = w.device_command(&["info", "-d"])?;
    let s = state(w)?;
    w.log(
        "INFO",
        format!("Security state: {}", serde_json::to_string(&s).unwrap()),
    );
    if r.action == "status" {
        return Ok(None);
    }
    let path = image_path(&r.firmware)?;
    let image = w.image(&path)?;
    let fp = image
        .fingerprint
        .as_deref()
        .ok_or("Select verified signed firmware.")?;
    let writes = plan(&s, &r.action, fp, r.slot, |row, ecc| read(w, row, ecc))?;
    if writes.is_empty() {
        w.log("INFO", "This stage is already complete.");
        return Ok(None);
    }
    if r.action == "enable" && vote(&s.critical, 3) & 1 == 0 {
        empty_storage(w, &path)?;
    }
    w.device_command(&["verify", &path.to_string_lossy()])?;
    if r.action == "lock" {
        let proof: Value = serde_json::from_slice(
            &fs::read(proof_path(&w.serial)?)
                .map_err(|_| "Verify protected boot before locking.")?,
        )
        .map_err(|_| "Invalid boot verification")?;
        if proof["serial"] != w.serial
            || proof["sha256"] != image.hash
            || property(&board, "secure boot") != Some("1")
        {
            return Err("Power-cycle and verify protected boot for this image and board.".into());
        }
    }
    for wr in &writes {
        w.log(
            "WARN",
            format!(
                "Planned {} row 0x{:03X} → 0x{:06X}",
                if wr.ecc { "ECC" } else { "RAW" },
                wr.row,
                wr.value
            ),
        );
    }
    let review = json!({"action":r.action,"serial":w.serial,"state":s,"image_hash":image.hash,"writes":writes,"slot":r.slot});
    if r.review.is_none() {
        return Ok(Some(review));
    }
    if r.review.as_ref() != Some(&review)
        || r.phrase != format!("{} {}", r.action.to_uppercase(), w.serial)
    {
        return Err("Review changed; confirm this exact stage again.".into());
    }
    if matches!(r.action.as_str(), "harden" | "enable") && !r.boot_tested {
        return Err("Power-cycle and test the previous stage first.".into());
    }
    if state(w)? != s || hash(&fs::read(&path).map_err(|e| e.to_string())?) != image.hash {
        return Err("Device or image changed during review.".into());
    }
    w.device_command(&["verify", &path.to_string_lossy()])?;
    for wr in writes {
        let allowed = (wr.ecc && (0x80..0xc0).contains(&wr.row))
            || (!wr.ecc
                && (CRIT.contains(&wr.row) || FLAGS.contains(&wr.row) || LOCKS.contains(&wr.row)));
        if !allowed {
            return Err("Unsupported OTP write.".into());
        }
        w.device_command(&[
            "otp",
            "set",
            "-c",
            "1",
            if wr.ecc { "-e" } else { "-r" },
            &format!("0x{:x}", wr.row),
            &format!("0x{:x}", wr.value),
        ])?;
        if read(w, wr.row, wr.ecc)? != wr.value {
            return Err("OTP verification failed. Stop and inspect the device.".into());
        }
    }
    w.log(
        "INFO",
        "Stage verified. Fully unplug before testing the next stage.",
    );
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn blank() -> State {
        State {
            critical: vec![0; 8],
            flags: vec![0; 3],
            locks: vec![0; 2],
            key_locks: vec![0; 2],
            fps: vec![Some("00".repeat(32)); 4],
        }
    }
    #[test]
    fn key_words_before_valid_flags() {
        let p = plan(&blank(), "load-key", &"11".repeat(32), 1, |_, _| Ok(0)).unwrap();
        assert_eq!(p.len(), 19);
        assert!(p[..16].iter().all(|w| w.ecc));
        assert_eq!(p[16].row, 0x4b);
        assert_eq!(p[16].value, 2);
    }
    #[test]
    fn refuse_revoked_torn_locked_slots() {
        let fp = "11".repeat(32);
        let mut s = blank();
        s.flags[1] = 0x100;
        assert!(plan(&s, "load-key", &fp, 0, |_, _| Ok(0)).is_err());
        assert!(plan(&blank(), "load-key", &fp, 0, |_, _| Ok(1)).is_err());
        s = blank();
        s.locks[1] = 0x303030;
        assert!(plan(&s, "load-key", &fp, 0, |_, _| Ok(0)).is_err());
    }
    #[test]
    fn enforce_hardening_and_single_trusted_key() {
        let fp = "11".repeat(32);
        let mut s = blank();
        s.flags = vec![1; 3];
        s.fps[0] = Some(fp.clone());
        assert!(plan(&s, "enable", &fp, 0, |_, _| Ok(0)).is_err());
        s.critical = vec![0x74; 8];
        assert_eq!(plan(&s, "enable", &fp, 0, |_, _| Ok(0)).unwrap().len(), 8);
        s.critical = vec![0x75; 8];
        assert_eq!(plan(&s, "lock", &fp, 0, |_, _| Ok(0)).unwrap().len(), 5);
        s.flags = vec![3; 3];
        assert!(plan(&s, "lock", &fp, 0, |_, _| Ok(0)).is_err());
    }
    #[test]
    fn otp_output_matches_row_and_width() {
        assert_eq!(
            parse_row("ROW 0x40:\nVALUE 0x75", 0x40, false).unwrap(),
            0x75
        );
        assert!(parse_row("ROW 0x41:\nVALUE 0x75", 0x40, false).is_err());
        assert!(parse_row("ROW 0x40:\nVALUE 0x10000", 0x40, true).is_err());
    }
}
