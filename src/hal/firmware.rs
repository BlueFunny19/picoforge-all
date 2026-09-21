//! Local firmware signing and verified update workflow.
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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
}
#[derive(Deserialize)]
pub struct Response {
    pub ok: bool,
    pub log: String,
    pub error: Option<String>,
    pub review: Option<serde_json::Value>,
}
pub fn run(python: &str, request: Request) -> Result<Response, String> {
    let _guard = super::transport::pcsc::lock_device().map_err(|e| e.to_string())?;
    let state = directories::ProjectDirs::from("org", "PicoForge", "PicoForge All")
        .ok_or("Cannot locate application data folder")?;
    std::fs::create_dir_all(state.data_local_dir()).map_err(|e| e.to_string())?;
    let mut payload = serde_json::to_value(request).map_err(|e| e.to_string())?;
    payload["state_dir"] = state.data_local_dir().to_string_lossy().to_string().into();
    payload["engine"] = include_str!("../../vendor/pico_all/firmware.py").into();
    let mut command = Command::new(if python.trim().is_empty() {
        "python"
    } else {
        python.trim()
    });
    command
        .args(["-u", "-c", include_str!("firmware_bridge.py")])
        .env("PYTHONUTF8", "1")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| format!("Cannot start Python: {e}. Choose a Python installation with rich, cryptography and pyscard."))?;
    let bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("Cannot open firmware worker input")?
        .write_all(&bytes)
        .map_err(|e| e.to_string())?;
    // Drain both pipes while the worker runs, including on Windows with small pipe buffers.
    let mut stdout = child
        .stdout
        .take()
        .ok_or("Cannot read firmware worker output")?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("Cannot read firmware worker errors")?;
    let out_reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.read_to_end(&mut b).map(|_| b)
    });
    let err_reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.read_to_end(&mut b).map(|_| b)
    });
    let started = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(600) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Firmware operation timed out. Check device state before retrying.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait().map_err(|e| e.to_string())?;
    let stdout = out_reader
        .join()
        .map_err(|_| "Firmware output reader stopped")?
        .map_err(|e| e.to_string())?;
    let stderr = err_reader
        .join()
        .map_err(|_| "Firmware error reader stopped")?
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&stdout).map_err(|_| {
        format!(
            "Firmware worker could not start: {}",
            String::from_utf8_lossy(&stderr)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires Python dependencies, PICOFORGE_TEST_UF2 and PICOTOOL; no USB operation"]
    fn inspect_local_firmware_worker() {
        let image = std::env::var("PICOFORGE_TEST_UF2").expect("explicit local test image");
        let tool = std::env::var("PICOTOOL").expect("explicit picotool path");
        let response = run(
            "python",
            Request {
                action: "inspect".into(),
                firmware: image,
                picotool: tool,
                ..Default::default()
            },
        )
        .expect("worker response");
        assert!(response.ok, "{:?}", response.error);
        assert!(
            response.log.contains("verified"),
            "signed image must be verified"
        );
        assert!(response.review.is_none());
    }
}
