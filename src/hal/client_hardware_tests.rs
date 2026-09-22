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
