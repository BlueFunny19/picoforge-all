"""GUI confirmation regression tests. All device transports are mocked."""
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch, MagicMock
from rich.console import Console

ROOT = Path(__file__).resolve().parents[1]
def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

engine = load("firmware_engine", ROOT / "vendor/pico_all/firmware.py")
bridge = load("firmware_bridge", ROOT / "src/hal/firmware_bridge.py")
SERIAL = "0011223344556677"

class ConfirmationTest(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.image = Path(folder.name) / "signed.uf2"
        self.image.write_bytes(b"test signed image")
        self.otp = MagicMock()
        self.otp.serial = SERIAL
        self.otp.board.return_value = "RP2350 secure boot: 1"
        self.state = {"crit": 0, "flags": [0, 0, 0], "critical": [0] * 8}
        for name, value in [
            ("tool_path", lambda _: "mock-picotool"),
            ("BootOtp", MagicMock(return_value=self.otp)),
            ("ensure_bootsel", MagicMock(return_value=SERIAL)),
            ("boot_state", MagicMock(side_effect=lambda _: dict(self.state))),
            ("show_security", MagicMock()),
            ("firmware_fingerprint", MagicMock(return_value=bytes(range(32)))),
            ("security_plan", MagicMock(return_value=[(0x40, 0x74, False)])),
            ("run", MagicMock(return_value="")),
            ("console", Console(file=io.StringIO())),
        ]:
            mock = patch.object(engine, name, value)
            mock.start()
            self.addCleanup(mock.stop)
        self.request = dict(action="harden", serial=SERIAL, firmware=str(self.image), slot=0)
    def review(self):
        review = bridge.execute(engine, self.request)
        self.otp.write.assert_not_called()
        return review
    def test_review_does_not_program_and_confirmed_stage_rechecks_before_writing(self):
        review = self.review()
        self.request.update(review=review, phrase="HARDEN " + SERIAL, boot_tested=True)
        self.assertIsNone(bridge.execute(engine, self.request))
        self.otp.write.assert_called_once_with(0x40, 0x74, False)
        self.assertGreaterEqual(engine.boot_state.call_count, 3)
        self.assertEqual(sum(call.args[1][0] == "verify" for call in engine.run.call_args_list), 3)
    def test_confirmation_requires_exact_phrase_and_power_cycle_acknowledgment(self):
        review = self.review()
        for phrase, boot in [("", True), ("HARDEN 1122334455667788", True), ("HARDEN " + SERIAL, False)]:
            with self.subTest(phrase=phrase, boot=boot):
                with self.assertRaises(engine.FirmwareError):
                    bridge.execute(engine, dict(self.request, review=review, phrase=phrase, boot_tested=boot))
                self.otp.write.assert_not_called()
    def test_changed_device_state_invalidates_review(self):
        review = self.review()
        self.state["crit"] = 1
        with self.assertRaisesRegex(engine.FirmwareError, "changed"):
            bridge.execute(engine, dict(self.request, review=review, phrase="HARDEN " + SERIAL, boot_tested=True))
        self.otp.write.assert_not_called()
    def test_changed_firmware_invalidates_review(self):
        review = self.review()
        self.image.write_bytes(b"different firmware")
        with self.assertRaisesRegex(engine.FirmwareError, "changed"):
            bridge.execute(engine, dict(self.request, review=review, phrase="HARDEN " + SERIAL, boot_tested=True))
        self.otp.write.assert_not_called()
    def test_flash_and_erase_do_not_run_without_target_confirmation(self):
        with patch.object(engine, "flash") as flash, patch.object(engine, "prepare_storage") as erase:
            for action in ["flash", "prepare"]:
                with self.assertRaises(engine.FirmwareError):
                    bridge.execute(engine, dict(self.request, action=action))
            flash.assert_not_called()
            erase.assert_not_called()
    def test_invalid_key_slot_rejected_before_device_access(self):
        with self.assertRaisesRegex(engine.FirmwareError, "slot"):
            bridge.execute(engine, dict(self.request, slot=255))
        engine.ensure_bootsel.assert_not_called()
        self.otp.write.assert_not_called()
    def test_unknown_action_rejected(self):
        with self.assertRaisesRegex(engine.FirmwareError, "Unknown"):
            bridge.execute(engine, dict(self.request, action="erase-chip"))
        self.otp.write.assert_not_called()

if __name__ == "__main__":
    unittest.main()
