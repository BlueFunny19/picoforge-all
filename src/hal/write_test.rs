//! Opt-in hardware round trips. Never part of the default test run.
//! Run one test at a time with PICOFORGE_TEST_SERIAL set to the test board.
//! Public test vectors and test PINs below must never be used in production.
use super::{
    applets::{hsm, oath, otp, piv},
    io,
};
fn board() {
    let expected =
        std::env::var("PICOFORGE_TEST_SERIAL").expect("explicit test-board serial required");
    assert!(
        expected.len() == 16 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "use the designated test board's complete serial"
    );
    let expected = expected.to_uppercase();
    let info = io::read_device_details().expect("device");
    assert_eq!(info.firmware_type, super::types::FirmwareType::PicoAll);
    assert_eq!(info.info.serial, expected);
}
#[test]
#[ignore = "mutates the designated test board; restores temporary OATH data"]
fn write_oath_roundtrip() {
    board();
    assert!(!io::oath_password_required().unwrap());
    let name = "PicoForge-All-hardware-test";
    let renamed = "PicoForge-All-hardware-test-renamed";
    assert!(
        !io::oath_list_accounts(None)
            .unwrap()
            .iter()
            .any(|a| a.id == name || a.id == renamed)
    );
    let result = std::panic::catch_unwind(|| {
        io::oath_add(
            None,
            oath::NewCredential {
                issuer: None,
                account: name.into(),
                secret: b"12345678901234567890".to_vec(),
                oath_type: oath::OathType::Hotp,
                algorithm: oath::HashAlgo::Sha1,
                digits: 6,
                period: 30,
                counter: 0,
                touch: false,
            },
        )
        .expect("create HOTP");
        assert_eq!(io::oath_calculate(None, name.into(), 30).unwrap(), "755224");
        io::oath_rename(None, name.into(), renamed.into()).expect("rename");
        assert_eq!(
            io::oath_calculate(None, renamed.into(), 30).unwrap(),
            "287082"
        );
        io::oath_set_password(None, Some("public-test-password".into())).expect("set password");
        assert!(io::oath_password_required().unwrap());
        assert!(
            io::oath_list_accounts(Some("public-test-password".into()))
                .unwrap()
                .iter()
                .any(|a| a.id == renamed)
        );
    });
    if io::oath_password_required().unwrap() {
        io::oath_set_password(Some("public-test-password".into()), None)
            .expect("clear test password");
    }
    for a in io::oath_list_accounts(None).unwrap() {
        if a.id == name || a.id == renamed {
            io::oath_delete(None, a.id).expect("delete test account");
        }
    }
    result.unwrap();
    println!("OATH: HOTP golden vectors, rename, password, cleanup passed");
}
#[test]
#[ignore = "mutates empty OTP slot 4 and removes it afterwards"]
fn write_otp_roundtrip() {
    board();
    assert_eq!(io::otp_read_info().unwrap()[3].kind, otp::SlotType::Empty);
    let result = std::panic::catch_unwind(|| {
        let secret = b"12345678901234567890";
        io::otp_program_chalresp(4, secret.to_vec(), false, [0; 6], [0; 6])
            .expect("program slot 4");
        assert_eq!(
            io::otp_read_info().unwrap()[3].kind,
            otp::SlotType::ChallengeResponse
        );
        let challenge = vec![0x42; 63];
        let expected = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret),
            &challenge,
        );
        assert_eq!(io::otp_calculate(4, challenge).unwrap(), expected.as_ref());
    });
    io::otp_delete(4, [0; 6]).expect("clear slot 4");
    assert_eq!(io::otp_read_info().unwrap()[3].kind, otp::SlotType::Empty);
    result.unwrap();
    println!("OTP: slot 4 program, HMAC verification, cleanup passed");
}
fn mgm() -> io::MgmAuth {
    io::MgmAuth::Key {
        key: piv::DEFAULT_MGM_KEY.to_vec(),
        algo: piv::ALGO_AES192,
    }
}
#[test]
#[ignore = "generates a temporary PIV key in retired slot 95 and deletes it"]
fn write_piv_roundtrip() {
    board();
    let info = io::piv_read_info().unwrap();
    assert!(info.mgm_default && info.mgm_algo == piv::ALGO_AES192);
    assert!(
        info.slots
            .iter()
            .any(|s| s.slot == 0x95 && s.meta.is_none() && !s.has_cert)
    );
    let result = std::panic::catch_unwind(|| {
        let public = io::piv_generate(
            0x95,
            piv::ALGO_ECCP256,
            piv::PIN_POLICY_NEVER,
            piv::TOUCH_POLICY_NEVER,
            mgm(),
        )
        .expect("generate P256");
        assert!(!public.is_empty());
        let cert = io::piv_attest(0x95).expect("attest generated key");
        assert_eq!(cert[0], 0x30);
        io::piv_import_cert(0x95, cert.clone(), mgm()).expect("import certificate");
        assert_eq!(io::piv_export_cert(0x95).unwrap(), cert);
    });
    io::piv_delete_cert(0x95, mgm()).expect("delete test certificate");
    io::piv_delete_key(0x95, mgm()).expect("delete test key");
    assert!(
        io::piv_read_info()
            .unwrap()
            .slots
            .iter()
            .any(|s| s.slot == 0x95 && s.meta.is_none() && !s.has_cert)
    );
    result.unwrap();
    println!("PIV: retired slot generation, attestation, certificate round trip, cleanup passed");
}
#[test]
#[ignore = "writes and restores OpenPGP cardholder fields"]
fn write_openpgp_roundtrip() {
    board();
    let before = io::openpgp_read_info().unwrap();
    let result = std::panic::catch_unwind(|| {
        io::openpgp_set_cardholder(
            "12345678".into(),
            "PicoForge All Test".into(),
            "test".into(),
            "https://example.invalid".into(),
            "en".into(),
            0x39,
        )
        .expect("write cardholder");
        let after = io::openpgp_read_info().unwrap();
        assert_eq!(after.name, "PicoForge All Test");
        assert_eq!(after.login, "test");
        assert_eq!(after.url, "https://example.invalid");
        assert_eq!(after.lang, "en");
        assert_eq!(after.sex, 0x39);
    });
    io::openpgp_set_cardholder(
        "12345678".into(),
        before.name.clone(),
        before.login.clone(),
        before.url.clone(),
        before.lang.clone(),
        before.sex,
    )
    .expect("restore cardholder");
    let after = io::openpgp_read_info().unwrap();
    assert_eq!(
        (after.name, after.login, after.url, after.lang, after.sex),
        (
            before.name,
            before.login,
            before.url,
            before.lang,
            before.sex
        )
    );
    result.unwrap();
    println!("OpenPGP: cardholder metadata write/read/restore passed");
}
#[test]
#[ignore = "initializes empty HSM, exercises temporary keys/objects, clears test state"]
fn write_hsm_roundtrip() {
    board();
    let before = hsm::read_info().unwrap();

    assert!(before.files.iter().all(|f| matches!(*f, 0xC400 | 0xCC00)));
    let result = std::panic::catch_unwind(|| {
        hsm::initialize(b"123456", b"12345678", 1).expect("initialize");
        hsm::dkek_share(b"123456", &[0xA5; 32]).expect("import test DKEK share");
        hsm::generate(b"123456", 0x7E, 5).expect("generate AES128");
        let plaintext = [0x42; 32];
        let encrypted = hsm::crypto(b"123456", 0x7E, 6, &plaintext).expect("encrypt");
        assert_ne!(encrypted, plaintext);
        assert_eq!(
            hsm::crypto(b"123456", 0x7E, 7, &encrypted).unwrap(),
            plaintext
        );
        let wrapped = hsm::wrap_key(b"123456", 0x7E).expect("wrap key");
        hsm::unwrap_key(b"123456", 0x7D, &wrapped).expect("unwrap key");
        assert_eq!(
            hsm::crypto(b"123456", 0x7D, 6, &plaintext).unwrap(),
            encrypted
        );
        hsm::delete_key(b"123456", 0x7D).expect("delete imported key");
        hsm::delete_key(b"123456", 0x7E).expect("delete generated AES key");
        hsm::generate(b"123456", 0x7C, 0).expect("generate P256");
        let sig = hsm::crypto(b"123456", 0x7C, 0, b"PicoForge All test").expect("sign");
        assert!(!sig.is_empty());
        hsm::write_object(b"123456", 0xCA7E, &vec![0x42; 600]).expect("write multi-page object");
        assert_eq!(
            hsm::read_object(b"123456", 0xCA7E).unwrap(),
            vec![0x42; 600]
        );
        hsm::change_pin(b"123456", b"654321", false).expect("change PIN");
        hsm::crypto(b"654321", 0x7C, 0, b"new PIN test").expect("new PIN authentication");
        hsm::change_pin(b"654321", b"123456", false).expect("restore PIN");
    });
    // User-authorized final state: empty initialized HSM with public test PINs.
    hsm::initialize(b"123456", b"12345678", 0).expect("clear HSM test keys and objects");
    let after = hsm::read_info().unwrap();
    assert!(
        !after
            .files
            .iter()
            .any(|f| matches!(*f, 0xCC7D | 0xCC7E | 0xCC7C | 0xCA7E))
    );
    result.unwrap();
    println!("HSM: initialization, DKEK, AES, key wrap, ECC, objects, PIN, cleanup passed");
}

#[test]
#[ignore = "requires user touch; leaves a test FIDO PIN until the separate reset step"]
fn write_fido_credential_roundtrip() {
    use super::fido::{constants::PinUvAuthTokenPermissions, ops::FidoOperations};
    use super::transport::fido::HidTransport;
    use serde_cbor_2::{Value, to_vec};
    use std::collections::BTreeMap;
    board();
    assert!(
        !io::get_fido_info()
            .unwrap()
            .options
            .get("clientPin")
            .copied()
            .unwrap_or(false)
    );
    io::change_fido_pin(None, "87654321".into()).expect("set test PIN");
    io::change_fido_pin(Some("87654321".into()), "123456".into()).expect("change test PIN");
    let pin = "123456";
    let rp = "picoforge-all.test";
    assert!(io::get_credentials(pin.into()).unwrap().is_empty());
    let result = std::panic::catch_unwind(|| {
        let t = HidTransport::open().unwrap();
        let token = t
            .get_pin_token_with_permission(
                pin,
                PinUvAuthTokenPermissions::MAKE_CREDENTIAL,
                Some(rp.into()),
            )
            .expect("scoped PIN token");
        let client_hash = [0x42; 32];
        let auth = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &token),
            &client_hash,
        );
        let text_map = |pairs: Vec<(&str, Value)>| {
            Value::Map(
                pairs
                    .into_iter()
                    .map(|(k, v)| (Value::Text(k.into()), v))
                    .collect(),
            )
        };
        let map: BTreeMap<_, _> = [
            (Value::Integer(1), Value::Bytes(client_hash.to_vec())),
            (
                Value::Integer(2),
                text_map(vec![
                    ("id", Value::Text(rp.into())),
                    ("name", Value::Text("PicoForge All test".into())),
                ]),
            ),
            (
                Value::Integer(3),
                text_map(vec![
                    ("id", Value::Bytes(b"temporary-test-user".to_vec())),
                    ("name", Value::Text("Temporary test".into())),
                ]),
            ),
            (
                Value::Integer(4),
                Value::Array(vec![text_map(vec![
                    ("type", Value::Text("public-key".into())),
                    ("alg", Value::Integer(-7)),
                ])]),
            ),
            (Value::Integer(7), text_map(vec![("rk", Value::Bool(true))])),
            (
                Value::Integer(8),
                Value::Bytes(auth.as_ref()[..16].to_vec()),
            ),
            (Value::Integer(9), Value::Integer(1)),
        ]
        .into_iter()
        .collect();
        let mut command = vec![1];
        command.extend(to_vec(&Value::Map(map)).unwrap());
        println!("TOUCH NOW: create the temporary PicoForge All passkey");
        let response = t
            .send_cbor_with_timeout(0x90, &command, 60_000)
            .expect("MakeCredential");
        assert!(!response.is_empty());
        drop(t);
        let creds = io::get_credentials(pin.into()).expect("enumerate created passkey");
        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].rp_id, rp);
        io::delete_credential(pin.into(), creds[0].credential_id.clone())
            .expect("delete temporary passkey");
        assert!(io::get_credentials(pin.into()).unwrap().is_empty());
    });
    // Clean up a credential even when a later assertion fails. PIN is removed
    // by write_fido_reset_after_replug when the user is ready for the ceremony.
    for cred in io::get_credentials(pin.into()).unwrap() {
        if cred.rp_id == rp {
            io::delete_credential(pin.into(), cred.credential_id).expect("cleanup passkey");
        }
    }
    result.unwrap();
    println!(
        "FIDO: set/change PIN and passkey creation/enumeration/deletion passed; reset still required"
    );
}

#[test]
#[ignore = "requires replug and touch; erases FIDO test PIN and checks applet isolation"]
fn write_fido_reset_after_replug() {
    board();
    assert!(
        io::get_credentials("123456".into()).unwrap().is_empty(),
        "never reset a board with retained credentials"
    );
    let hsm_before = hsm::read_info().unwrap();
    println!("TOUCH NOW: confirm FIDO reset");
    io::reset_device().expect("reset after replug");
    assert!(
        !io::get_fido_info()
            .unwrap()
            .options
            .get("clientPin")
            .copied()
            .unwrap_or(false)
    );
    let hsm_after = hsm::read_info().unwrap();
    assert_eq!(hsm_after.files, hsm_before.files);
    assert_eq!(hsm_after.pin, hsm_before.pin);
    println!("FIDO: test PIN erased; HSM state retained");
}

#[test]
#[ignore = "changes LED brightness and OATH application enablement, then restores both"]
fn write_configuration_roundtrip() {
    use super::types::{AppConfigInput, DeviceMethod};
    board();
    let before = super::rescue::read_device_details().unwrap();
    let apps = io::read_management_config(DeviceMethod::Rescue).unwrap();
    let original = before
        .config
        .led_brightness
        .expect("explicit brightness can be restored");
    let next = if original == 1 { 2 } else { 1 };
    let patch = |value: u8| -> AppConfigInput {
        serde_json::from_value(serde_json::json!({"ledBrightness":value})).unwrap()
    };
    let result = std::panic::catch_unwind(|| {
        println!("TOUCH NOW: confirm temporary brightness change");
        io::write_config(patch(next), DeviceMethod::Rescue, None).expect("brightness write");
        let changed = super::rescue::read_device_details().unwrap();
        assert_eq!(changed.config.led_brightness, Some(next));
        let mut expected = before.config.clone();
        expected.led_brightness = Some(next);
        assert_eq!(
            changed.config, expected,
            "partial writes preserve unrelated configuration"
        );
        io::write_management_config(DeviceMethod::Rescue, apps.usb_enabled & !0x20, None)
            .expect("disable OATH");
        assert_eq!(
            io::read_management_config(DeviceMethod::Rescue)
                .unwrap()
                .usb_enabled,
            apps.usb_enabled & !0x20
        );
        assert!(
            io::oath_password_required().is_err(),
            "disabled OATH must reject SELECT"
        );
    });
    io::write_management_config(DeviceMethod::Rescue, apps.usb_enabled, None)
        .expect("restore app mask");
    if super::rescue::read_device_details()
        .unwrap()
        .config
        .led_brightness
        != Some(original)
    {
        println!("TOUCH NOW: restore original brightness");
        io::write_config(patch(original), DeviceMethod::Rescue, None).expect("restore brightness");
    }
    assert_eq!(
        super::rescue::read_device_details().unwrap().config,
        before.config
    );
    assert_eq!(
        io::read_management_config(DeviceMethod::Rescue)
            .unwrap()
            .usb_enabled,
        apps.usb_enabled
    );
    io::oath_password_required().expect("OATH available again");
    result.unwrap();
    println!(
        "Configuration: brightness write/read/restore, unrelated field preservation, OATH disable/enable passed"
    );
}

fn nested_tlv(data: &[u8], tag: u32, depth: u8) -> Option<&[u8]> {
    if depth == 0 {
        return None;
    }
    for (t, v) in super::apdu::tlv::TlvIter::new(data) {
        if t == tag {
            return Some(v);
        }
        if matches!(t, 0x67 | 0x7F21 | 0x7F4E) {
            if let Some(found) = nested_tlv(v, tag, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}
#[test]
#[ignore = "generates temporary HSM signing keys; verifies signatures; deletes test objects"]
fn write_hsm_signatures_and_pin_recovery() {
    use super::apdu::tlv;
    use ring::signature;
    board();
    let before = hsm::read_info().unwrap();
    assert!(
        !before
            .files
            .iter()
            .any(|f| matches!(*f, 0xCC7A | 0xCC7B | 0xCA7A))
    );
    let result = std::panic::catch_unwind(|| {
        let message = b"PicoForge All independent signature verification";
        let ec = hsm::generate(b"123456", 0x7A, 0).expect("generate EC key");
        let ec_public = nested_tlv(&ec, 0x7F49, 6).expect("CVC public key");
        let point = tlv::find(ec_public, 0x86).expect("EC point");
        let sig = hsm::crypto(b"123456", 0x7A, 0, message).expect("ECDSA signing");
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, point)
            .verify(message, &sig)
            .expect("independent ECDSA verification");
        let rsa = hsm::generate(b"123456", 0x7B, 2).expect("generate RSA2048 key");
        let public = nested_tlv(&rsa, 0x7F49, 6).expect("RSA public key");
        let key = signature::RsaPublicKeyComponents {
            n: tlv::find(public, 0x81).unwrap(),
            e: tlv::find(public, 0x82).unwrap(),
        };
        for (op, algorithm) in [
            (2, &signature::RSA_PKCS1_2048_8192_SHA256),
            (3, &signature::RSA_PSS_2048_8192_SHA256),
        ] {
            let sig = hsm::crypto(b"123456", 0x7B, op, message).expect("RSA signing");
            key.verify(algorithm, message, &sig)
                .expect("independent RSA verification");
        }
        hsm::write_object(b"123456", 0xCA7A, &[0x42; 600]).unwrap();
        hsm::write_object(b"123456", 0xCA7A, b"shorter").unwrap();
        assert_eq!(hsm::read_object(b"123456", 0xCA7A).unwrap(), b"shorter");
        hsm::delete_object(b"123456", 0xCA7A).expect("delete object");
        hsm::change_pin(b"12345678", b"87654321", true).expect("change SO PIN");
        hsm::unblock_pin(b"87654321", b"654321").expect("SO recovery");
        hsm::crypto(b"654321", 0x7A, 0, message).expect("recovered PIN");
        hsm::change_pin(b"654321", b"123456", false).expect("restore user PIN");
        hsm::change_pin(b"87654321", b"12345678", true).expect("restore SO PIN");
    });
    hsm::initialize(b"123456", b"12345678", 0).expect("cleanup temporary signing keys");
    assert!(
        !hsm::read_info()
            .unwrap()
            .files
            .iter()
            .any(|f| matches!(*f, 0xCC7A | 0xCC7B | 0xCA7A))
    );
    result.unwrap();
    println!(
        "HSM: independent ECC/RSA PKCS1/PSS verification, short overwrite/delete, SO PIN recovery passed"
    );
}

#[test]
#[ignore = "requires two button confirmations; installs then clears a temporary organization attestation"]
fn write_attestation_roundtrip() {
    board();
    assert!(
        !io::att_status().unwrap().installed,
        "preserve any existing attestation"
    );
    let key =
        std::fs::read(".codex/tmp/attestation-test/key.pem").expect("temporary test key fixture");
    let certificate = std::fs::read(".codex/tmp/attestation-test/certificate.pem")
        .expect("temporary certificate fixture");
    let result = std::panic::catch_unwind(|| {
        println!("TOUCH NOW: install temporary organization attestation");
        io::att_import(Some("123456".into()), key, certificate).expect("attestation import");
        let status = io::att_status().unwrap();
        assert!(status.installed);
        assert_eq!(status.chain_hash.as_ref().map(String::len), Some(64));
    });
    if io::att_status().unwrap().installed {
        println!("TOUCH NOW: clear temporary organization attestation");
        io::att_clear(Some("123456".into())).expect("attestation cleanup");
    }
    assert!(!io::att_status().unwrap().installed);
    result.unwrap();
    println!("Attestation: secure import, installed status/hash, physical clear passed");
}

#[test]
#[ignore = "tests empty OpenPGP key generation/signing/PIN/touch; resets only the test applet"]
fn write_openpgp_key_and_pin_roundtrip() {
    use super::apdu::{Apdu, tlv};
    use super::applets::openpgp as pgp;
    use ring::{digest, signature};
    board();
    let before = io::openpgp_read_info().unwrap();
    assert!(before.keys.iter().all(|k| !k.present));
    assert!(
        before.name.is_empty()
            && before.login.is_empty()
            && before.url.is_empty()
            && before.lang.is_empty()
    );
    let hsm_before = hsm::read_info().unwrap();
    let result = std::panic::catch_unwind(|| {
        io::openpgp_generate("12345678".into(), pgp::PgpSlot::Sig, 3)
            .expect("generate OpenPGP P256");
        let info = io::openpgp_read_info().unwrap();
        assert!(info.keys[0].present);
        assert_eq!(info.keys[0].algo, "ECC P-256");
        {
            let session = pgp::open().unwrap();
            let public = session
                .transceive_full(&Apdu::read(0, 0x47, 0x81, 0, &[0xB6, 0]))
                .expect("read public key");
            let public = tlv::find(&public, 0x7F49).unwrap();
            let point = tlv::find(public, 0x86).unwrap();
            pgp::verify_pin(&session, pgp::PW1, "123456").unwrap();
            let message = b"PicoForge All OpenPGP verification";
            let hash = digest::digest(&digest::SHA256, message);
            let sig = session
                .transceive_full(&Apdu::read(0, 0x2A, 0x9E, 0x9A, hash.as_ref()))
                .expect("OpenPGP signature");
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point)
                .verify(message, &sig)
                .expect("independent OpenPGP signature verification");
        }
        io::openpgp_set_touch("12345678".into(), pgp::PgpSlot::Sig, true).unwrap();
        assert!(io::openpgp_read_info().unwrap().keys[0].touch);
        io::openpgp_set_touch("12345678".into(), pgp::PgpSlot::Sig, false).unwrap();
        io::openpgp_change_user_pin("123456".into(), "654321".into()).unwrap();
        io::openpgp_change_user_pin("654321".into(), "123456".into()).unwrap();
        io::openpgp_set_reset_code("12345678".into(), "87654321".into()).unwrap();
        io::openpgp_unblock_with_code("87654321".into(), "654321".into()).unwrap();
        io::openpgp_unblock_with_admin("12345678".into(), "123456".into()).unwrap();
        io::openpgp_set_reset_code("12345678".into(), String::new()).unwrap();
    });
    io::openpgp_reset().expect("clear temporary OpenPGP key and test PIN state");
    let after = io::openpgp_read_info().unwrap();
    assert!(after.keys.iter().all(|k| !k.present && !k.touch));
    assert_eq!(
        (after.pw1_retries, after.rc_retries, after.pw3_retries),
        (3, 0, 3)
    );
    assert_eq!(hsm::read_info().unwrap().files, hsm_before.files);
    result.unwrap();
    println!(
        "OpenPGP: ECC key/signature verification, touch, PIN/reset-code operations, reset isolation passed"
    );
}
#[test]
#[ignore = "temporarily changes PIV PIN, PUK and management key; restores defaults"]
fn write_piv_pin_and_management_roundtrip() {
    board();
    let before = io::piv_read_info().unwrap();
    assert!(before.pin.unwrap().is_default && before.puk.unwrap().is_default && before.mgm_default);
    io::piv_change_pin("123456".into(), "654321".into()).expect("change PIV PIN");
    io::piv_change_pin("654321".into(), "123456".into()).expect("restore PIV PIN");
    io::piv_change_puk("12345678".into(), "87654321".into()).expect("change PUK");
    io::piv_unblock_pin("87654321".into(), "654321".into()).expect("PUK recovery");
    io::piv_change_pin("654321".into(), "123456".into()).expect("restore recovered PIN");
    io::piv_change_puk("87654321".into(), "12345678".into()).expect("restore PUK");
    io::piv_set_mgm(mgm(), piv::ALGO_AES256, vec![0x42; 32], false)
        .expect("set AES256 management key");
    let result = std::panic::catch_unwind(|| {
        let info = io::piv_read_info().unwrap();
        assert_eq!(info.mgm_algo, piv::ALGO_AES256);
        assert!(!info.mgm_default);
    });
    io::piv_set_mgm(
        io::MgmAuth::Key {
            key: vec![0x42; 32],
            algo: piv::ALGO_AES256,
        },
        piv::ALGO_AES192,
        piv::DEFAULT_MGM_KEY.to_vec(),
        false,
    )
    .expect("restore management key");
    let after = io::piv_read_info().unwrap();
    assert!(after.pin.unwrap().is_default && after.puk.unwrap().is_default && after.mgm_default);
    result.unwrap();
    println!(
        "PIV: PIN, PUK recovery, AES256 management authentication and default restoration passed"
    );
}
