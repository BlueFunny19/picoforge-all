//! Opt-in tests of the same DeviceRepo/HAL operations called by PicoForge views.
use super::{
    applets::{hsm, openpgp},
    io,
    transport::fido::HidTransport,
};
use crate::ui::models::device::DeviceRepo;

fn select_target() {
    let expected = std::env::var("PICOFORGE_TEST_SERIAL").expect("Set the intended board serial");
    assert_eq!(
        io::read_device_details()
            .expect("device discovery")
            .info
            .serial,
        expected
    );
}
#[test]
#[ignore = "requires the specified local board; read-only"]
fn current_app_read_flows() {
    select_target();
    let state = DeviceRepo::read_device_state_blocking().expect("Configuration read flow");
    let fido = DeviceRepo::get_fido_info_blocking().expect("Passkeys GetInfo flow");
    let pgp = DeviceRepo::openpgp_read_info_blocking().expect("OpenPGP card information flow");
    let piv = DeviceRepo::piv_read_info_blocking().expect("PIV card information flow");
    let hsm = hsm::read_info().expect("HSM card information flow");
    let presence =
        HidTransport::selected_fingerprint().expect("non-intrusive reset presence check");
    assert!(HidTransport::has_fingerprint(&presence));
    println!(
        "Current PicoForge production flows: device={}, FIDO ep={:?}, OpenPGP={:?}, HSM initialized={:?}, PIV readable",
        state.status.info.serial,
        fido.options.get("ep"),
        pgp.version,
        hsm.initialized
    );
    println!(
        "Status: FIDO PIN set={:?}, Audit enabled={}, OpenPGP slots present={:?}, HSM user={}, HSM SO={}, HSM files={:04X?}",
        fido.options.get("clientPin"),
        DeviceRepo::audit_status_blocking().expect("Audit status"),
        pgp.keys.iter().map(|key| key.present).collect::<Vec<_>>(),
        hsm.pin,
        hsm.so_pin,
        hsm.files,
    );
    drop(piv);
}
#[test]
#[ignore = "requires explicit local test PIN and an EMPTY OpenPGP signature slot; creates one RSA-2048 test key"]
fn current_app_openpgp_generate_empty_slot() {
    select_target();
    let admin = std::env::var("PICOFORGE_TEST_OPENPGP_ADMIN")
        .expect("Set the current OpenPGP admin PIN locally");
    let before = DeviceRepo::openpgp_read_info_blocking().unwrap();
    assert!(
        !before.keys[0].present,
        "Refuse to overwrite an existing signature key"
    );
    let started = std::time::Instant::now();
    DeviceRepo::openpgp_generate_blocking(admin, openpgp::PgpSlot::Sig, 0)
        .expect("same generation entry point as the UI");
    let after = DeviceRepo::openpgp_read_info_blocking().unwrap();
    assert!(after.keys[0].present);
    println!(
        "OpenPGP RSA-2048 generation + refresh completed in {:.1}s",
        started.elapsed().as_secs_f32()
    );
}

#[test]
#[ignore = "requires the specified board and up to four touches; restores the original Audit switch"]
fn current_app_audit_roundtrip() {
    select_target();
    let fido = DeviceRepo::get_fido_info_blocking().unwrap();
    let pin = std::env::var("PICOFORGE_TEST_FIDO_PIN").ok();
    assert!(
        fido.options.get("clientPin") != Some(&true) || pin.is_some(),
        "Supply the current FIDO PIN locally; do not guess it"
    );
    let initial = DeviceRepo::audit_status_blocking().expect("Audit status");
    if !initial {
        println!("AUDIT: press and release BOOTSEL to enable journalling");
        assert!(DeviceRepo::audit_set_enabled_blocking(true, pin.clone()).expect("enable Audit"));
    }
    println!("AUDIT: press BOOTSEL for the journal read, then again for the signed checkpoint");
    let result = DeviceRepo::audit_verify_blocking(pin.clone(), None);
    let restored = if !initial {
        println!("AUDIT: press and release BOOTSEL to restore journalling to OFF");
        DeviceRepo::audit_set_enabled_blocking(false, pin)
    } else {
        Ok(initial)
    };
    assert_eq!(restored.expect("restore original Audit switch"), initial);
    assert_eq!(DeviceRepo::audit_status_blocking().unwrap(), initial);
    let verification = result.expect("production Audit verification");
    assert!(verification.signature_ok, "checkpoint signature");
    assert!(
        verification.head_matches,
        "checkpoint covers the exported journal"
    );
    assert!(!verification.journal.entries.is_empty());
    println!(
        "AUDIT PASS: signature={}, journal head={}, entries={}, initial/restored enabled={}",
        verification.signature_ok,
        verification.head_matches,
        verification.journal.entries.len(),
        initial,
    );
}

#[test]
#[ignore = "requires an empty HSM and explicit test PINs; leaves it initialized with those PINs and no test objects"]
fn current_app_hsm_setup_and_auto_ids() {
    select_target();
    let pin = std::env::var("PICOFORGE_TEST_HSM_PIN").expect("Set a test User PIN locally");
    let so = std::env::var("PICOFORGE_TEST_HSM_SO_PIN").expect("Set a test SO PIN locally");
    let before = hsm::read_info().unwrap();
    assert!(before.files.iter().all(|f| matches!(*f, 0xC400 | 0xCC00)));
    if before.initialized == Some(false) {
        hsm::setup(pin.as_bytes(), so.as_bytes(), 1).expect("first-use Setup");
    } else {
        assert_eq!(
            std::env::var("PICOFORGE_TEST_RESET_EMPTY_HSM").as_deref(),
            Ok("1"),
            "Explicitly opt in to resetting an already initialized empty test HSM"
        );
        hsm::initialize(pin.as_bytes(), so.as_bytes(), 1)
            .expect("prepare empty HSM for repeat testing");
    }
    let result = std::panic::catch_unwind(|| {
        assert_eq!(hsm::read_info().unwrap().initialized, Some(true));
        hsm::dkek_share(pin.as_bytes(), &[0xA5; 32]).expect("test DKEK share");
        // A certificate-only slot and a metadata-only slot must both be skipped.
        hsm::write_object(pin.as_bytes(), 0xCE01, b"reserved certificate").unwrap();
        hsm::write_object(pin.as_bytes(), 0xC402, b"reserved metadata").unwrap();
        hsm::generate_auto(pin.as_bytes(), 5).expect("auto-generate AES-128");
        let generated = hsm::read_info().unwrap();
        assert!(
            generated.files.contains(&0xCC03),
            "Unexpected file list: {:04X?}",
            generated.files
        );
        assert!(!generated.files.contains(&0xCC01) && !generated.files.contains(&0xCC02));
        let plaintext = [0x42; 32];
        let encrypted = hsm::crypto(pin.as_bytes(), 3, 6, &plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        let wrapped = hsm::wrap_key(pin.as_bytes(), 3).expect("wrap generated key");
        hsm::unwrap_auto(pin.as_bytes(), &wrapped).expect("auto-import into next free slot");
        assert!(hsm::read_info().unwrap().files.contains(&0xCC04));
        assert_eq!(
            hsm::crypto(pin.as_bytes(), 4, 7, &encrypted).unwrap(),
            plaintext
        );
        assert_eq!(
            hsm::read_object(pin.as_bytes(), 0xCE01).unwrap(),
            b"reserved certificate"
        );
        assert_eq!(
            hsm::read_object(pin.as_bytes(), 0xC402).unwrap(),
            b"reserved metadata"
        );
        let object = vec![0x42; 600];
        hsm::write_object(pin.as_bytes(), 0xCF7E, &object).unwrap();
        assert_eq!(hsm::read_object(pin.as_bytes(), 0xCF7E).unwrap(), object);
        let populated = hsm::read_info().unwrap().files;
        assert!(
            hsm::setup(pin.as_bytes(), so.as_bytes(), 0).is_err(),
            "Setup must refuse an initialized card"
        );
        assert_eq!(hsm::read_info().unwrap().files, populated);
        assert_eq!(
            hsm::crypto(pin.as_bytes(), 3, 6, &plaintext).unwrap(),
            encrypted
        );
    });
    // Remove only files created on the initially empty test applet.
    let created = hsm::read_info().unwrap().files;
    for fid in created
        .iter()
        .filter(|f| **f >> 8 == 0xCC && **f as u8 != 0)
    {
        assert!(
            matches!(*fid, 0xCC03 | 0xCC04),
            "Unexpected key; refuse cleanup"
        );
        hsm::delete_key(pin.as_bytes(), *fid as u8).unwrap();
    }
    for fid in hsm::read_info().unwrap().files {
        if fid as u8 != 0 {
            assert!(
                matches!(
                    fid,
                    0xCE01 | 0xC402 | 0xC403 | 0xCE03 | 0xC404 | 0xCE04 | 0xCF7E
                ),
                "Unexpected object; refuse cleanup"
            );
            hsm::delete_object(pin.as_bytes(), fid).unwrap();
        }
    }
    assert!(
        hsm::read_info()
            .unwrap()
            .files
            .iter()
            .all(|f| matches!(*f, 0xC400 | 0xCC00))
    );
    // Restore the previously authorized empty test configuration without DKEK.
    hsm::initialize(pin.as_bytes(), so.as_bytes(), 0).expect("clear the test DKEK configuration");
    let after = hsm::read_info().unwrap();
    assert_eq!(after.initialized, Some(true));
    assert!(after.files.iter().all(|f| matches!(*f, 0xC400 | 0xCC00)));
    result.unwrap();
    println!(
        "HSM PASS: first Setup, metadata/certificate collision avoidance, auto generate/import, object I/O, repeat Setup refusal and cleanup"
    );
}

#[test]
#[ignore = "requires an empty FIDO applet, a new test PIN, physical unplug/replug and a reset touch; clears the test FIDO state"]
fn current_app_ea_then_reset() {
    select_target();
    let pin = std::env::var("PICOFORGE_TEST_FIDO_PIN").expect("Set a new test FIDO PIN locally");
    let before = DeviceRepo::get_fido_info_blocking().unwrap();
    assert_eq!(before.options.get("clientPin"), Some(&false));
    assert_eq!(before.options.get("ep"), Some(&false));
    let hsm_before = hsm::read_info().unwrap();
    let audit_before = DeviceRepo::audit_status_blocking().unwrap();
    DeviceRepo::change_fido_pin_blocking(None, pin.clone()).expect("set test FIDO PIN");
    assert!(
        DeviceRepo::get_credentials_blocking(pin.clone())
            .unwrap()
            .is_empty(),
        "Refuse to reset an applet containing credentials"
    );
    DeviceRepo::enable_enterprise_attestation_blocking(pin)
        .expect("enable EA with immediate readback");
    assert_eq!(
        DeviceRepo::get_fido_info_blocking()
            .unwrap()
            .options
            .get("ep"),
        Some(&true)
    );
    println!(
        "EA PASS: enabled state read back. Now unplug and reconnect the selected board, then press BOOTSEL for reset."
    );
    super::fido::wait_for_reset_reconnection().expect("same reconnect worker used by Passkeys UI");
    DeviceRepo::reset_device_blocking().expect("FIDO reset immediately after reconnect");
    let after = DeviceRepo::get_fido_info_blocking().unwrap();
    assert_eq!(after.options.get("clientPin"), Some(&false));
    assert_eq!(after.options.get("ep"), Some(&false));
    assert_eq!(DeviceRepo::audit_status_blocking().unwrap(), audit_before);
    let hsm_after = hsm::read_info().unwrap();
    assert_eq!(hsm_after.initialized, hsm_before.initialized);
    assert_eq!(hsm_after.files, hsm_before.files);
    println!(
        "RESET PASS: unplug/reconnect detection, PIN/EA cleanup, HSM and Audit state preserved"
    );
}

#[test]
#[ignore = "requires the specified Pico All and three BOOTSEL confirmations; restores status-light configuration"]
fn current_app_status_modes_roundtrip() {
    select_target();
    let state = DeviceRepo::read_device_state_blocking().unwrap();
    let original = state.led_status.clone().expect("LED settings");
    assert!(
        original.steady_modes.is_some(),
        "Install firmware with per-state modes first"
    );
    let mut updated = original.clone();
    updated.steady_modes = Some([true, false, true, false, true, false, false]);
    println!("LED STEP 1: press BOOTSEL to save independent modes (Ready steady)");
    DeviceRepo::write_all_config_blocking(
        state.status.method.clone(),
        None,
        Some(updated.clone()),
        None,
        None,
    )
    .unwrap();
    let result = std::panic::catch_unwind(|| {
        assert_eq!(
            io::read_led_config(state.status.method.clone()).unwrap(),
            updated
        );
        std::thread::sleep(std::time::Duration::from_secs(5));
        let mut updated = updated.clone();
        updated.steady_modes = Some([false, true, false, true, false, true, true]);
        println!("LED STEP 2: press BOOTSEL to save the opposite modes (Ready breathing)");
        DeviceRepo::write_all_config_blocking(
            state.status.method.clone(),
            None,
            Some(updated.clone()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            io::read_led_config(state.status.method.clone()).unwrap(),
            updated
        );
        std::thread::sleep(std::time::Duration::from_secs(5));
    });
    println!("LED RESTORE: press BOOTSEL to restore the original light settings");
    DeviceRepo::write_all_config_blocking(
        state.status.method.clone(),
        None,
        Some(original.clone()),
        None,
        None,
    )
    .expect("restore original LED settings");
    assert_eq!(io::read_led_config(state.status.method).unwrap(), original);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    println!("LED PASS: independent modes saved/read back, original settings restored");
}

#[test]
#[ignore = "requires the current HSM PIN; creates and deletes two temporary objects, preserving existing content"]
fn current_app_hsm_object_editor() {
    select_target();
    let pin = std::env::var("PICOFORGE_TEST_HSM_PIN").expect("Supply the current HSM PIN");
    let original = hsm::read_info().unwrap();
    let first =
        hsm::write_object_auto(pin.as_bytes(), 0xCD, "PicoForge 文字测试".as_bytes()).unwrap();
    let mut second = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(
            hsm::read_object(pin.as_bytes(), first).unwrap(),
            "PicoForge 文字测试".as_bytes()
        );
        let binary = vec![0xAB; hsm::MAX_OBJECT_BYTES];
        let id = hsm::write_object_auto(pin.as_bytes(), 0xCD, &binary).unwrap();
        second = Some(id);
        assert_ne!(id, first);
        assert!(!original.files.contains(&first) && !original.files.contains(&id));
        assert_eq!(hsm::read_object(pin.as_bytes(), id).unwrap(), binary);
        hsm::write_object(pin.as_bytes(), first, b"updated").unwrap();
        assert_eq!(hsm::read_object(pin.as_bytes(), first).unwrap(), b"updated");
        assert!(
            hsm::write_object_auto(pin.as_bytes(), 0xCD, &vec![0; hsm::MAX_OBJECT_BYTES + 1])
                .is_err()
        );
    }));
    if let Some(id) = second {
        hsm::delete_object(pin.as_bytes(), id).unwrap();
    }
    hsm::delete_object(pin.as_bytes(), first).unwrap();
    assert_eq!(hsm::read_info().unwrap().files, original.files);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    println!(
        "OBJECT PASS: UTF-8 text, 1,800-byte file, automatic IDs, replacement, size rejection and cleanup"
    );
}

#[test]
#[ignore = "resets only an EMPTY dedicated HSM to PicoForge defaults; clears its PINs and DKEK configuration"]
fn current_app_hsm_reset_defaults() {
    select_target();
    assert!(
        hsm::read_info()
            .unwrap()
            .files
            .iter()
            .all(|id| matches!(*id, 0xC400 | 0xCC00)),
        "This regression only resets an empty test HSM"
    );
    hsm::reset_defaults().unwrap();
    let info = hsm::read_info().unwrap();
    assert_eq!(info.initialized, Some(true));
    assert!(info.files.iter().all(|id| matches!(*id, 0xC400 | 0xCC00)));
    println!(
        "HSM RESET PASS: user PIN 123456, SO PIN 12345678, no DKEK shares; initialized and empty"
    );
}
