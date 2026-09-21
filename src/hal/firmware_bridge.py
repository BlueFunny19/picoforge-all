# SPDX-License-Identifier: AGPL-3.0-only
"""Private JSON bridge for the bundled firmware engine; no shell command parsing."""
import contextlib
import io
import json
from pathlib import Path
import sys
from types import SimpleNamespace

def main(request):
    namespace = {"__name__": "picoforge_firmware", "__file__": str(Path(request["state_dir"]) / "firmware.py")}
    exec(compile(request["engine"], "<Pico All firmware engine>", "exec"), namespace)
    return execute(SimpleNamespace(**namespace), request)


def execute(ns, request):
    action = request["action"]
    serial = request.get("serial", "").upper()
    source = request.get("firmware", "")
    key = request.get("key", "")
    output = request.get("output", "")
    if action in ("load-key", "harden", "enable", "lock") and request.get("slot", 0) not in range(4):
        raise ns.FirmwareError("Choose a boot key slot from 0 to 3.")
    tool = None if action == "new-key" else ns.tool_path(request.get("picotool") or None)
    if action not in ("new-key", "sign", "inspect"):
        ns.BootOtp(tool, serial)  # Always bind device work to an explicit serial.
    if action == "new-key":
        ns.signing_key(Path(key).expanduser().resolve(), True)
    elif action == "sign":
        # GUI never silently replaces a file.
        if Path(output).exists():
            raise ns.FirmwareError("Output already exists. Choose a new output file.")
        ns.sign(tool, source, key, output, yes=False)
    elif action == "inspect":
        print(ns.run(tool, ["info", "-b", "-l", "-m", str(ns.uf2_file(source))], "Reading image..."))
    elif action == "info":
        ns.board_info(tool, serial)
    elif action in ("bootsel", "reboot"):
        ns.device_mode(tool, action, serial)
    elif action == "flash":
        if request.get("phrase") != "FLASH " + serial:
            raise ns.FirmwareError("Confirm the selected device before flashing.")
        ns.flash(tool, source, serial, yes=True)
    elif action == "prepare":
        if request.get("phrase") != "ERASE " + serial:
            raise ns.FirmwareError("Confirm erasing application storage before preparing.")
        ns.prepare_storage(serial, True)
    elif action in ("status", "prove"):
        ns.security(tool, action, serial, source)
    elif action in ("load-key", "harden", "enable", "lock"):
        class ReviewRequired(Exception):
            pass
        review = {}
        def confirm(stage, board, state, image_hash):
            # JSON normalizes bytes without exposing private signing material.
            current = json.loads(json.dumps({
                "action": stage, "serial": board, "state": state, "image_hash": image_hash
            }, default=lambda b: b.hex()))
            review.update(current)
            prior = request.get("review")
            if prior is None:
                raise ReviewRequired()
            if prior != current:
                raise ns.FirmwareError("Device or firmware changed. Review this stage again.")
            if request.get("phrase") != stage.upper() + " " + board:
                raise ns.FirmwareError("The confirmation phrase does not match this stage and board.")
            if stage in ("harden", "enable") and not request.get("boot_tested"):
                raise ns.FirmwareError("Power-cycle and test the previous stage before continuing.")
        try:
            ns.security(tool, action, serial, source, int(request.get("slot", 0)),
                        apply=True, gui_confirm=confirm)
        except ReviewRequired:
            return review
    else:
        raise ns.FirmwareError("Unknown firmware operation.")
    return None

if __name__ == "__main__":
    capture = io.StringIO()
    try:
        request = json.load(sys.stdin)
        with contextlib.redirect_stdout(capture), contextlib.redirect_stderr(capture):
            review = main(request)
        result = {"ok": True, "log": capture.getvalue()[-24000:], "review": review}
    except BaseException as error:
        result = {"ok": False, "log": capture.getvalue()[-16000:], "error": str(error)}
    print(json.dumps(result))
