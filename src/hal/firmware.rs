//! Native firmware workflow. picotool is the USB/UF2 transport; policy lives in Rust.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};
mod security;

#[derive(Default)]
struct SigningArtifacts {
    closing: bool,
    files: Vec<PathBuf>,
}
static SIGNING_ARTIFACTS: std::sync::Mutex<SigningArtifacts> =
    std::sync::Mutex::new(SigningArtifacts {
        closing: false,
        files: Vec::new(),
    });
pub fn cleanup_signed_images() {
    let mut artifacts = SIGNING_ARTIFACTS.lock().unwrap_or_else(|e| e.into_inner());
    artifacts.closing = true;
    for path in artifacts.files.drain(..) {
        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("Could not remove temporary signed image: {error}");
            }
        }
    }
}

#[derive(Clone, Default, Serialize)]
pub struct Request {
    pub action: String,
    pub picotool: String,
    pub serial: String,
    pub firmware: String,
    pub key: String,
    pub output: String,
    pub phrase: String,
    pub slot: u8,
    pub boot_tested: bool,
    pub review: Option<serde_json::Value>,
    pub mismatch_accepted: bool,
}
#[derive(Default, Debug)]
pub struct Response {
    pub review: Option<serde_json::Value>,
    pub image: Option<ImageInfo>,
    pub assessment: Option<Assessment>,
    pub boards: Vec<String>,
    pub output: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub signed: bool,
    #[serde(default)]
    pub nuke: bool,
    pub fingerprint: Option<String>,
    pub hash: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub image: ImageInfo,
    pub serial: String,
    pub secure_boot: bool,
    /// Matching OTP key when Secure Boot is enabled; installed key otherwise.
    pub board_key: Option<String>,
    pub allowed: bool,
    pub mismatch: bool,
}
pub fn serial_valid(serial: &str) -> bool {
    serial.len() == 16 && serial.bytes().all(|b| b.is_ascii_hexdigit())
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}
fn image_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if !path.is_file()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("uf2"))
    {
        return Err("Choose an existing .uf2 firmware file.".into());
    }
    fs::canonicalize(path).map_err(|e| e.to_string())
}
fn property<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|l| l.trim().split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case(label))
        .map(|(_, v)| v.trim())
}
/// Require a single RP2350 ARM Secure image; never interpret an invalid signature as unsigned.
pub fn parse_image(text: &str, digest: String) -> Result<ImageInfo, String> {
    let blocks: Vec<_> = text
        .split("Metadata Block ")
        .skip(1)
        .filter(|b| property(b, "image type").is_some())
        .collect();
    let signed: Vec<_> = blocks
        .iter()
        .filter(|b| property(b, "signature").is_some())
        .collect();
    let b = if signed.len() == 1 {
        *signed[0]
    } else if signed.is_empty() && blocks.len() == 1 {
        blocks[0]
    } else {
        return Err("Select a single RP2350 ARM firmware image.".into());
    };
    if property(b, "target chip") != Some("RP2350")
        || property(b, "image type") != Some("ARM Secure")
    {
        return Err("The image must target RP2350 ARM Secure.".into());
    }
    let signature = property(b, "signature");
    let public = property(b, "public key");
    let fingerprint = match signature {
        Some("verified") => {
            let key = hex::decode(public.ok_or("Signed image has no public key")?)
                .map_err(|_| "Invalid public key")?;
            if key.len() != 64 {
                return Err("Invalid firmware public key length".into());
            }
            Some(hash(&key))
        }
        None | Some("none") | Some("not present") | Some("unsigned") if public.is_none() => None,
        _ => return Err("Firmware signature could not be verified.".into()),
    };
    let program = text.split("Metadata Block ").next().unwrap_or("");
    let nuke = property(program, "name") == Some("flash_nuke")
        && property(program, "binary start") == Some("0x20000000");
    Ok(ImageInfo {
        signed: fingerprint.is_some(),
        nuke,
        fingerprint,
        hash: digest,
    })
}
fn installed_image(text: &str) -> Result<Option<ImageInfo>, String> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if !text.contains("Metadata Block ")
        && lines.any(|line| line == "Metadata Blocks")
        && lines.next() == Some("none")
    {
        return Ok(None);
    }
    parse_image(text, String::new()).map(Some)
}

fn assess_device(
    image: ImageInfo,
    serial: String,
    installed: &str,
    mut read_otp: impl FnMut(u16, bool) -> Result<u32, String>,
) -> Result<Assessment, String> {
    let secure_boot = match property(installed, "secure boot") {
        Some("1") => true,
        Some("0") => false,
        _ => return Err("Cannot determine the board's Secure Boot state.".into()),
    };
    let secure_boot = secure_boot || security::secure_boot_enabled(&mut read_otp)?;
    let board_key = if secure_boot {
        security::trusted_boot_key(image.fingerprint.as_deref(), &mut read_otp)?
    } else {
        installed_image(installed)?.and_then(|image| image.fingerprint)
    };
    Ok(assess(image, serial, secure_boot, board_key))
}

pub fn assess(
    image: ImageInfo,
    serial: String,
    secure_boot: bool,
    board_key: Option<String>,
) -> Assessment {
    let mismatch = image.fingerprint != board_key;
    let allowed = !secure_boot || (image.signed && board_key.is_some() && !mismatch);
    Assessment {
        image,
        serial,
        secure_boot,
        board_key,
        allowed,
        mismatch,
    }
}

fn validate_flash(request: &Request, current: &Assessment) -> Result<(), String> {
    if !current.allowed {
        return Err("FLASH disabled: signing key is not trusted by this board.".into());
    }
    if request.phrase != format!("FLASH {}", current.serial) {
        return Err("Confirm the target before flashing.".into());
    }
    if current.mismatch && !request.mismatch_accepted {
        return Err("Confirm the different signing key before flashing.".into());
    }
    if request.review.as_ref() != Some(&serde_json::to_value(current).unwrap()) {
        return Err("Device or firmware changed. Inspect compatibility again.".into());
    }
    Ok(())
}

fn update_command(nuke: bool) -> [u8; 5] {
    [0x80, 0x1f, if nuke { 2 } else { 1 }, 0, 0]
}

struct Worker {
    tool: String,
    serial: String,
    log: Sender<String>,
}
impl Worker {
    fn log(&self, level: &str, message: impl AsRef<str>) {
        let _ = self.log.send(format!(
            "[{}] [{level}] {}",
            crate::logging::local_timestamp(),
            message.as_ref()
        ));
    }
    fn command(&self, args: &[&str]) -> Result<String, String> {
        let mut cmd = Command::new(&self.tool);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Cannot start picotool: {e}"))?;
        let tx = self.log.clone();
        let out = child.stdout.take().ok_or("Missing process output")?;
        let err = child.stderr.take().ok_or("Missing process error output")?;
        fn drain(pipe: impl std::io::Read, tx: Sender<String>) -> String {
            let mut result = String::new();
            for line in BufReader::new(pipe).lines() {
                match line {
                    Ok(line) => {
                        result.push_str(&line);
                        result.push('\n');
                        let _ = tx.send(format!(
                            "[{}] [picotool] {line}",
                            crate::logging::local_timestamp()
                        ));
                    }
                    Err(_) => break,
                }
            }
            result
        }
        let a = std::thread::spawn(move || drain(out, tx));
        let tx = self.log.clone();
        let b = std::thread::spawn(move || drain(err, tx));
        let began = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            if began.elapsed() > Duration::from_secs(180) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = a.join();
                let _ = b.join();
                return Err("picotool timed out. Check the board before retrying.".into());
            }
            std::thread::sleep(Duration::from_millis(40));
        };
        let text = a.join().map_err(|_| "Output reader stopped")?
            + &b.join().map_err(|_| "Output reader stopped")?;
        if !status.success() {
            return Err(if text.contains("Signature verification failed") {
                "Signature verification failed".into()
            } else if text.to_lowercase().contains("no accessible") {
                "No device detected in update mode.".into()
            } else {
                format!(
                    "picotool failed with exit code {}. Enable Raw output for details.",
                    status.code().unwrap_or(-1)
                )
            });
        }
        Ok(text)
    }
    fn device_command(&self, args: &[&str]) -> Result<String, String> {
        let mut args = args.to_vec();
        args.extend(["--ser", &self.serial]);
        self.command(&args)
    }
    fn bootsel(&self) -> Result<Vec<String>, String> {
        let result = if self.serial.is_empty() {
            self.command(&["info", "-d"])
        } else {
            self.device_command(&["info", "-d"])
        };
        let text = match result {
            Ok(t) => t,
            Err(e) => {
                let t = e.split_whitespace().collect::<Vec<_>>().join(" ");
                let absence = if self.serial.is_empty() {
                    "No accessible RP-series devices in BOOTSEL mode were found.".to_owned()
                } else {
                    format!(
                        "No accessible RP-series devices in BOOTSEL mode were found with serial number {}.",
                        self.serial
                    )
                };
                if t == absence {
                    return Ok(vec![]);
                }
                if t.starts_with("ERROR: Block loop is not valid") {
                    let found = security::factory_serial(self)?;
                    if self.serial.is_empty() || found == self.serial {
                        return Ok(vec![found]);
                    }
                    return Err("Recovery device serial does not match the selected board.".into());
                }
                return Err(e);
            }
        };
        let ids: Vec<_> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("chipid:"))
            .map(|s| s.trim().trim_start_matches("0x").to_uppercase())
            .collect();
        if ids.is_empty()
            || ids
                .iter()
                .any(|s| !serial_valid(s) || (!self.serial.is_empty() && s != &self.serial))
            || text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("type:"))
                .any(|s| s.trim() != "RP2350")
        {
            return Err("Could not identify the selected RP2350.".into());
        }
        Ok(ids)
    }
    fn ensure_bootsel(&self) -> Result<(), String> {
        self.ensure_bootsel_for(false)
    }
    fn ensure_bootsel_for(&self, nuke: bool) -> Result<(), String> {
        if self.bootsel()? == vec![self.serial.clone()] {
            return Ok(());
        }
        self.log(
            "INFO",
            if nuke {
                "Nuke selected: press the board button while its light breathes red."
            } else {
                "When the light flashes, press and release the device button (BOOTSEL)."
            },
        );
        management(&self.serial, &update_command(nuke)).map_err(|e| {
            if nuke && (e.contains("(6B00)") || e.contains("(6A86)") || e.contains("(6D00)")) {
                "The installed firmware does not support Nuke confirmation lights. Update Pico All first.".into()
            } else if nuke && e.contains("(6985)") {
                "Nuke was not confirmed. Press and release the button during the red breathing prompt, then retry.".into()
            } else { e }
        })?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            if self.bootsel()? == vec![self.serial.clone()] {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err("The selected board did not enter update mode.".into())
    }
    fn image(&self, path: &Path) -> Result<ImageInfo, String> {
        let text = self.command(&["info", "-a", &path.to_string_lossy()])?;
        parse_image(&text, hash(&fs::read(path).map_err(|e| e.to_string())?))
    }
    fn assessment(&self, path: &Path) -> Result<Assessment, String> {
        let image = self.image(path)?;
        self.ensure_bootsel_for(image.nuke)?;
        let installed = self.device_command(&["info", "-m", "-d"])?;
        let a = assess_device(image, self.serial.clone(), &installed, |row, ecc| {
            security::read(self, row, ecc)
        })?;
        self.log(if a.allowed {"INFO"} else {"WARN"},if !a.allowed {
            "FLASH disabled: the firmware must match an active OTP signing key."
        } else if a.secure_boot {
            "Firmware signing key matches an active OTP signing key."
        } else if a.mismatch {
            "Signing key differs from installed firmware or no installed key is available. Secure Boot is off; FLASH requires an extra confirmation."
        } else { "Firmware is compatible with the board's current signing policy." });
        Ok(a)
    }
}
fn normal_cards() -> Result<Vec<(String, pcsc::Card)>, String> {
    let context = pcsc::Context::establish(pcsc::Scope::User).map_err(|e| e.to_string())?;
    let mut readers = [0u8; 4096];
    let mut found = vec![];
    for reader in context
        .list_readers(&mut readers)
        .map_err(|e| e.to_string())?
    {
        let Ok(card) = context.connect(reader, pcsc::ShareMode::Shared, pcsc::Protocols::ANY)
        else {
            continue;
        };
        let mut rx = [0u8; 512];
        let Ok(data) = card.transmit(
            &[
                0, 0xa4, 4, 0, 8, 0xa0, 0x58, 0x3f, 0xc1, 0x9b, 0x7e, 0x4f, 0x21,
            ],
            &mut rx,
        ) else {
            continue;
        };
        if data.len() == 14 && data.ends_with(&[0x90, 0]) && data[1] == 0 && data[2] >= 8 {
            found.push((hex::encode_upper(&data[4..12]), card));
        }
    }
    Ok(found)
}
fn management(serial: &str, command: &[u8]) -> Result<Vec<u8>, String> {
    let mut matches: Vec<_> = normal_cards()?
        .into_iter()
        .filter(|(s, _)| s == serial)
        .collect();
    if matches.len() != 1 {
        return Err("Connect exactly the selected board in normal mode.".into());
    }
    let (_, card) = matches.pop().unwrap();
    let mut rx = [0u8; 4096];
    // Windows may return raw ERROR_GEN_FAILURE during USB re-enumeration.
    // pcsc 2.9 panics on that unmapped code. Only reboot commands tolerate it;
    // callers still require the same serial to appear in the requested mode.
    let transition = command.starts_with(&[0x80, 0x1f]);
    let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        card.transmit(command, &mut rx).map(|data| data.to_vec())
    }));
    let data = match response {
        Ok(Ok(data)) => data,
        Ok(Err(_)) | Err(_) if transition => return Ok(vec![]),
        Ok(Err(e)) => return Err(e.to_string()),
        Err(_) => return Err("The smart-card driver returned an unexpected error.".into()),
    };
    if !data.ends_with(&[0x90, 0]) {
        return Err(format!(
            "Device declined the operation ({})",
            hex::encode_upper(&data)
        ));
    }
    Ok(data[..data.len() - 2].to_vec())
}
pub fn run(request: Request, log: Sender<String>) -> Result<Response, String> {
    let _guard = super::transport::pcsc::lock_device().map_err(|e| e.to_string())?;
    let action = request.action.as_str();
    if !matches!(action, "image" | "inspect" | "sign" | "scan") && !serial_valid(&request.serial) {
        return Err("Choose a board or enter its 16-digit serial.".into());
    }
    let tool = if request.picotool.trim().is_empty() {
        std::env::var("PICOTOOL").unwrap_or_else(|_| "picotool".into())
    } else {
        request.picotool.clone()
    };
    let w = Worker {
        tool,
        serial: request.serial.to_uppercase(),
        log,
    };
    let operation = match action {
        "image" | "inspect" | "check" => "firmware inspection",
        "sign" => "firmware signing",
        "flash" => "firmware update",
        "reboot" => "device restart",
        "bootsel" => "update mode",
        _ => action,
    };
    w.log("INFO", format!("Starting {operation}"));
    let mut result = Response::default();
    match action {
        "scan" => {
            result.boards = normal_cards()?.into_iter().map(|(s, _)| s).collect();
            let scanner = Worker {
                tool: w.tool.clone(),
                serial: String::new(),
                log: w.log.clone(),
            };
            match scanner.bootsel() {
                Ok(ids) => result.boards.extend(ids),
                Err(e) => w.log("WARN", e),
            }
            result.boards.sort();
            result.boards.dedup();
        }
        "image" | "inspect" => {
            let path = image_path(&request.firmware)?;
            result.image = Some(w.image(&path)?);
            if action == "inspect" && serial_valid(&request.serial) {
                result.assessment = Some(w.assessment(&path)?);
            }
        }
        "sign" => {
            let source = image_path(&request.firmware)?;
            if w.image(&source)?.signed {
                return Err("This firmware is already signed.".into());
            }
            let key = fs::canonicalize(&request.key)
                .map_err(|_| "Choose an existing signing key PEM.")?;
            let mut artifacts = SIGNING_ARTIFACTS.lock().unwrap_or_else(|e| e.into_inner());
            if artifacts.closing {
                return Err("Application is closing.".into());
            }
            let dest = std::env::temp_dir().join(format!(
                "picoforge-signed-{}-{:032x}.uf2",
                std::process::id(),
                rand::random::<u128>()
            ));
            if dest.exists() || dest == key || source == key {
                return Err(
                    "Choose a new output path; existing files are never overwritten.".into(),
                );
            }
            let tmp = dest.with_extension(format!("{}.tmp.uf2", std::process::id()));
            if tmp.exists() {
                return Err("A signing temporary file already exists.".into());
            }
            artifacts.files.extend([dest.clone(), tmp.clone()]);
            let signed = (|| {
                w.command(&[
                    "seal",
                    "--sign",
                    &source.to_string_lossy(),
                    &tmp.to_string_lossy(),
                    &key.to_string_lossy(),
                ]).map_err(|error| {
                    if error.contains("Signature verification failed") {
                        format!("{error}. RP2350 firmware signing requires a secp256k1 private key; a PIV P-256 key cannot be used.")
                    } else { error }
                })?;
                let info = w.image(&tmp)?;
                if !info.signed {
                    return Err("Output signature was not verified.".into());
                }
                let mut output = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&dest)
                    .map_err(|e| e.to_string())?;
                output
                    .write_all(&fs::read(&tmp).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                output.sync_all().map_err(|e| e.to_string())?;
                result.image = Some(info);
                result.output = Some(dest.to_string_lossy().to_string());
                Ok::<(), String>(())
            })();
            let _ = fs::remove_file(tmp);
            signed?;
            w.log(
                "INFO",
                format!(
                    "Temporary signed image (deleted when PicoForge closes): {}",
                    dest.display()
                ),
            );
        }
        "check" | "flash" => {
            let path = image_path(&request.firmware)?;
            let current = w.assessment(&path)?;
            if action == "flash" {
                validate_flash(&request, &current)?;
                if hash(&fs::read(&path).map_err(|e| e.to_string())?) != current.image.hash {
                    return Err("Firmware changed during verification.".into());
                }
                w.device_command(&["load", "-v", "-x", &path.to_string_lossy()])?;
            }
            result.image = Some(current.image.clone());
            result.assessment = Some(current);
        }
        "info" => {
            if !normal_cards()?
                .iter()
                .any(|(serial, _)| serial == &w.serial)
            {
                return Err("Device is not connected in normal mode.".into());
            }
            w.log(
                "INFO",
                format!("Device {} is connected in normal mode.", w.serial),
            );
        }
        "bootsel" => w.ensure_bootsel()?,
        "reboot" => {
            if w.bootsel()? == vec![w.serial.clone()] {
                w.device_command(&["reboot", "-a"])?;
            } else {
                management(&w.serial, &[0x80, 0x1f, 0, 0, 0])?;
            }
            let started = Instant::now();
            loop {
                if normal_cards()
                    .is_ok_and(|cards| cards.iter().filter(|(s, _)| s == &w.serial).count() == 1)
                {
                    break;
                }
                if started.elapsed() > Duration::from_secs(30) {
                    return Err("The selected board did not return to normal mode.".into());
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        _ => result.review = security::execute(&w, &request)?,
    }
    let mut complete = operation.to_owned();
    if let Some(first) = complete.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    w.log("INFO", format!("{complete} completed"));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn metadata(signature: &str) -> String {
        format!("Metadata Block 0\n image type: ARM Secure\n target chip: RP2350\n{signature}")
    }
    #[test]
    fn signature_policy() {
        let unsigned = parse_image(&metadata(""), "a".into()).unwrap();
        assert!(!unsigned.signed);
        assert!(!assess(unsigned.clone(), "s".into(), true, Some("key".into())).allowed);
        assert!(assess(unsigned, "s".into(), false, Some("key".into())).mismatch);
        let signed = parse_image(
            &metadata(&format!(
                " signature: verified\n public key: {}\n",
                "ab".repeat(64)
            )),
            "a".into(),
        )
        .unwrap();
        assert!(assess(signed.clone(), "s".into(), true, signed.fingerprint.clone()).allowed);
        assert!(!assess(signed, "s".into(), true, Some("different".into())).allowed);
    }
    fn signed_image() -> ImageInfo {
        parse_image(
            &metadata(&format!(
                " signature: verified\n public key: {}\n",
                "ab".repeat(64)
            )),
            "digest".into(),
        )
        .unwrap()
    }
    fn empty_board(secure: bool) -> String {
        format!(
            "Metadata Blocks\n none\nDevice Information\n type: RP2350\n secure boot: {}\n",
            u8::from(secure)
        )
    }
    fn otp(
        fingerprint: String,
        flags: [u32; 3],
        critical: u32,
    ) -> impl FnMut(u16, bool) -> Result<u32, String> {
        let bytes = hex::decode(fingerprint).unwrap();
        move |row, ecc| match row {
            0x40..=0x47 => {
                assert!(!ecc);
                Ok(critical)
            }
            0x4b..=0x4d => {
                assert!(!ecc);
                Ok(flags[(row - 0x4b) as usize])
            }
            0x80..=0xbf => {
                assert!(ecc);
                let i = ((row - 0x80) % 16) as usize * 2;
                Ok(u16::from_le_bytes([bytes[i], bytes[i + 1]]) as u32)
            }
            _ => panic!("unexpected OTP read: {row:x}"),
        }
    }
    #[test]
    fn empty_secure_board_recovers_only_with_active_otp_key() {
        let image = signed_image();
        let fp = image.fingerprint.clone().unwrap();
        for (flags, expected) in [
            ([1, 1, 1], true),
            ([8, 8, 0], true), // Slot 3, majority-valid
            ([1, 0, 0], false),
            ([0, 0, 0], false),
            ([0x101, 0x101, 1], false), // Majority-revoked
        ] {
            let result = assess_device(
                image.clone(),
                "serial".into(),
                &empty_board(true),
                otp(fp.clone(), flags, 1),
            )
            .unwrap();
            assert_eq!(result.allowed, expected, "flags {flags:?}");
        }
        let wrong = assess_device(
            image.clone(),
            "serial".into(),
            &empty_board(true),
            otp("11".repeat(32), [1; 3], 1),
        )
        .unwrap();
        assert!(!wrong.allowed);
        let unreadable = assess_device(image, "serial".into(), &empty_board(true), |_, _| {
            Err("OTP read failed".into())
        });
        assert!(unreadable.is_err());
    }
    #[test]
    fn installed_metadata_cannot_substitute_for_otp_trust() {
        let image = signed_image();
        let installed = format!(
            "{}Device Information\n secure boot: 1\n",
            metadata(&format!(
                " signature: verified\n public key: {}\n",
                "ab".repeat(64)
            ))
        );
        let result = assess_device(
            image,
            "serial".into(),
            &installed,
            otp("11".repeat(32), [1; 3], 1),
        )
        .unwrap();
        assert!(!result.allowed);
    }
    #[test]
    fn empty_unlocked_board_and_otp_secure_boot_fallback() {
        let image = signed_image();
        let fp = image.fingerprint.clone().unwrap();
        let unlocked = assess_device(
            image.clone(),
            "serial".into(),
            &empty_board(false),
            otp(fp.clone(), [0; 3], 0),
        )
        .unwrap();
        assert!(unlocked.allowed && unlocked.mismatch);
        assert_eq!(unlocked.board_key, None);
        let locked = assess_device(
            image,
            "serial".into(),
            &empty_board(false),
            otp(fp, [0; 3], 1),
        )
        .unwrap();
        assert!(locked.secure_boot);
        assert!(!locked.allowed);
        let unsigned = parse_image(&metadata(""), String::new()).unwrap();
        let locked = assess_device(unsigned, "serial".into(), &empty_board(true), |_, _| {
            panic!("unsigned image needs no key lookup")
        })
        .unwrap();
        assert!(!locked.allowed);
    }
    #[test]
    fn missing_installed_image_is_not_an_invalid_input_image() {
        assert!(installed_image(&empty_board(false)).unwrap().is_none());
        assert!(parse_image(&empty_board(false), String::new()).is_err());
        assert!(installed_image("Device Information\n secure boot: 0\n").is_err());
        assert!(installed_image(&metadata(" signature: invalid\n")).is_err());
        let image = signed_image();
        assert!(
            assess_device(
                image,
                "serial".into(),
                "Metadata Blocks\n none\n",
                |_, _| Ok(0)
            )
            .is_err()
        );
    }
    #[test]
    fn reject_invalid_and_multiple_images() {
        assert!(parse_image(&metadata(" signature: invalid\n"), "".into()).is_err());
        assert!(parse_image(&(metadata("") + &metadata("")), "".into()).is_err());
        assert!(
            parse_image(
                &metadata(" signature: verified\n public key: aa"),
                "".into()
            )
            .is_err()
        );
    }
    #[test]
    fn nuke_prompt_uses_program_metadata_not_filename() {
        let blocks = metadata("");
        let nuke = parse_image(
            &format!("Program Information\n name: flash_nuke\n binary start: 0x20000000\n{blocks}"),
            "".into(),
        )
        .unwrap();
        assert!(nuke.nuke);
        let ordinary = parse_image(
            &format!("File flash_nuke.uf2\n name: pico_all\n binary start: 0x10000000\n{blocks}"),
            "".into(),
        )
        .unwrap();
        assert!(!ordinary.nuke);
        assert_eq!(update_command(nuke.nuke), [0x80, 0x1f, 2, 0, 0]);
        assert_eq!(update_command(ordinary.nuke), [0x80, 0x1f, 1, 0, 0]);
        // The program summary duplicates the signature status in picotool -a.
        let signed = metadata(&format!(
            " signature: verified\n public key: {}\n",
            "ab".repeat(64)
        ));
        assert!(
            parse_image(
                &format!(
                    "Program Information\n image type: ARM Secure\n signature: verified\n{signed}"
                ),
                "".into(),
            )
            .unwrap()
            .signed
        );
    }
    #[test]
    fn exact_serial() {
        assert!(serial_valid("432D921975CCC729"));
        assert!(!serial_valid("--all"));
        assert!(!serial_valid(""));
    }
}

#[cfg(test)]
mod native_integration {
    use super::*;
    #[test]
    #[ignore = "requires PICOTOOL and PICOFORGE_TEST_UF2; local files only"]
    fn native_sign_and_inspect() {
        let input = std::env::var("PICOFORGE_TEST_UF2").unwrap();
        let tool = std::env::var("PICOTOOL").unwrap();
        let key = std::env::var("PICOFORGE_TEST_KEY").unwrap();
        let external_output = std::env::temp_dir().join("picoforge-must-not-write-external.uf2");
        let input_path = PathBuf::from(&input);
        let (tx, _rx) = std::sync::mpsc::channel();
        let unsigned = run(
            Request {
                action: "inspect".into(),
                picotool: tool.clone(),
                firmware: input.clone(),
                ..Default::default()
            },
            tx.clone(),
        )
        .unwrap();
        assert!(!unsigned.image.unwrap().signed);
        let signed = run(
            Request {
                action: "sign".into(),
                picotool: tool.clone(),
                firmware: input,
                key,
                output: external_output.to_string_lossy().into_owned(),
                ..Default::default()
            },
            tx.clone(),
        )
        .unwrap();
        assert!(signed.image.unwrap().signed);
        let output = signed.output.unwrap();
        assert_ne!(PathBuf::from(&output), external_output);
        assert_eq!(
            Path::new(&output).parent(),
            Some(std::env::temp_dir().as_path())
        );
        assert!(Path::new(&output).exists());
        let inspected = run(
            Request {
                action: "inspect".into(),
                picotool: tool.clone(),
                firmware: output.clone(),
                ..Default::default()
            },
            tx.clone(),
        )
        .unwrap();
        assert!(inspected.image.unwrap().signed);
        let rejected = run(
            Request {
                action: "sign".into(),
                picotool: tool,
                firmware: output.clone(),
                ..Default::default()
            },
            tx,
        );
        assert!(rejected.unwrap_err().contains("already signed"));
        cleanup_signed_images();
        assert!(!Path::new(&output).exists());
        assert!(
            input_path.exists(),
            "cleanup must preserve the user's original firmware"
        );
    }
}

#[cfg(test)]
mod confirmation_tests {
    use super::*;
    #[test]
    fn mismatch_needs_both_confirmations_and_exact_snapshot() {
        let image = ImageInfo {
            signed: true,
            nuke: false,
            fingerprint: Some("new".into()),
            hash: "digest".into(),
        };
        let current = assess(image, "432D921975CCC729".into(), false, Some("old".into()));
        let mut r = Request {
            phrase: "FLASH 432D921975CCC729".into(),
            review: Some(serde_json::to_value(&current).unwrap()),
            ..Default::default()
        };
        assert!(validate_flash(&r, &current).is_err());
        r.mismatch_accepted = true;
        assert!(validate_flash(&r, &current).is_ok());
        r.phrase = "FLASH DIFFERENT".into();
        assert!(validate_flash(&r, &current).is_err());
        r.phrase = "FLASH 432D921975CCC729".into();
        let mut changed = current.clone();
        changed.image.hash = "changed".into();
        assert!(validate_flash(&r, &changed).is_err());
        let locked = assess(current.image, current.serial, true, current.board_key);
        assert!(validate_flash(&r, &locked).is_err());
    }
    #[test]
    #[ignore = "read-only USB inventory, requires connected Pico All"]
    fn native_board_inventory() {
        let serial = std::env::var("PICOFORGE_TEST_SERIAL").unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let r = run(
            Request {
                action: "scan".into(),
                ..Default::default()
            },
            tx,
        )
        .unwrap();
        assert!(r.boards.contains(&serial));
    }
}
