#!/usr/bin/env python3
"""Local signing, verified UF2 updates, and staged RP2350 security."""
# SPDX-License-Identifier: AGPL-3.0-only
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

try:
    from rich.console import Console
    from rich.prompt import Confirm, Prompt
    from rich.table import Table
except ImportError:
    raise SystemExit("Install dependencies: python -m pip install -r requirements.txt")

console = Console(highlight=False)
error_console = Console(stderr=True, highlight=False)
ROOT = Path(__file__).resolve().parent
DEFAULT_KEY = ROOT / ".private" / "firmware-signing.pem"


class FirmwareError(Exception):
    pass


class PicotoolError(FirmwareError):
    def __init__(self, output: str):
        self.output = output
        lines = [line.strip() for line in output.splitlines() if line.strip()]
        super().__init__("\n".join(lines[-6:]) or "picotool failed.")


def tool_path(value: str | None) -> str:
    candidate = value or os.environ.get("PICOTOOL") or shutil.which("picotool")
    if not candidate:
        for name in ("picotool.exe", "picotool"):
            local = ROOT / name
            if local.is_file():
                candidate = str(local)
                break
    if not candidate:
        raise FirmwareError("picotool not found. Add it to PATH or use --picotool PATH.")
    resolved = shutil.which(candidate) or candidate
    if not Path(resolved).is_file():
        raise FirmwareError("picotool not found. Check --picotool or PICOTOOL.")
    return str(Path(resolved).resolve())


def run(tool: str, args: list[str], label: str, timeout: int = 120) -> str:
    with console.status(label):
        try:
            result = subprocess.run([tool, *args], capture_output=True, text=True,
                                    errors="replace", timeout=timeout, check=False,
                                    creationflags=0x08000000 if os.name == "nt" else 0)
        except subprocess.TimeoutExpired:
            raise FirmwareError("picotool timed out. Check the USB connection and retry the command.") from None
        except OSError:
            raise FirmwareError("Could not start picotool. Check its installation.") from None
    output = result.stdout + result.stderr
    if result.returncode:
        raise PicotoolError(output)
    return output


def uf2_file(value: str | Path) -> Path:
    path = Path(value).expanduser().resolve()
    if not path.is_file() or path.suffix.lower() != ".uf2":
        raise FirmwareError("Choose an existing .uf2 firmware file.")
    return path


def signing_key(path: Path, create: bool) -> None:
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    if create:
        if path.exists():
            raise FirmwareError("Key already exists. Remove --new-key to use it.")
        key = ec.generate_private_key(ec.SECP256K1())
        pem = key.private_bytes(serialization.Encoding.PEM,
                                serialization.PrivateFormat.TraditionalOpenSSL,
                                serialization.NoEncryption())
        path.parent.mkdir(parents=True, exist_ok=True)
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(pem)
        console.print("Created local signing key: " + str(path), style="green", markup=False)
        console.print("Keep this key private and back it up offline.", style="dim")
    if not path.is_file():
        raise FirmwareError("Key not found. Use --new-key to create one.")
    try:
        key = serialization.load_pem_private_key(path.read_bytes(), password=None)
    except (ValueError, TypeError):
        raise FirmwareError("Use an unencrypted secp256k1 private key in PEM format.") from None
    if not isinstance(key, ec.EllipticCurvePrivateKey) or not isinstance(key.curve, ec.SECP256K1):
        raise FirmwareError("Signing requires a secp256k1 private key.")


def approved(message: str, yes: bool) -> None:
    if yes:
        return
    if not sys.stdin.isatty():
        raise FirmwareError("Confirmation required. Review the command, then add --yes.")
    if not Confirm.ask(message, default=False):
        raise FirmwareError("Cancelled.")


def sign(tool: str, firmware: str, key_name: str, output: str | None,
         new_key: bool = False, yes: bool = False) -> Path:
    source = uf2_file(firmware)
    key = Path(key_name).expanduser().resolve()
    destination = Path(output).expanduser().resolve() if output else source.with_name(source.stem + ".signed.uf2")
    if destination.suffix.lower() != ".uf2":
        raise FirmwareError("Output must end in .uf2.")
    if source == destination or key in (source, destination):
        raise FirmwareError("Firmware, signed output, and key must use different paths.")
    if not destination.parent.is_dir():
        raise FirmwareError("Output folder does not exist.")
    if destination.exists():
        approved("Replace the existing signed firmware?", yes)
    run(tool, ["info", "-b", str(source)], "Checking firmware...")
    signing_key(key, new_key)
    # Publish only after picotool has verified the new signature.
    with tempfile.TemporaryDirectory(prefix=".firmware-sign-", dir=destination.parent) as directory:
        temporary = Path(directory) / "signed.uf2"
        run(tool, ["seal", "--sign", str(source), str(temporary), str(key)], "Signing firmware...")
        info = run(tool, ["info", "-b", str(temporary)], "Verifying signature...")
        if not re.search(r"signature:\s+verified\b", info, re.IGNORECASE):
            raise FirmwareError("Signature verification failed. Output was not replaced.")
        os.replace(temporary, destination)
    console.print("Signed and verified: " + str(destination), style="green", markup=False)
    return destination


def selection(serial: str | None) -> list[str]:
    return ["--ser", serial] if serial else []


def recovery_board(tool: str, serial: str | None) -> list[str]:
    # Interrupted flash may break image metadata before `info -d` reaches the
    # chip ID. These four public factory rows identify RP2350 without an image.
    output = run(tool, ["otp", "get", "-c", "1", "-e", "-n", *selection(serial),
                        "0x0", "0x1", "0x2", "0x3"], "Checking recovery device...", 10)
    blocks = re.split(r"(?m)^\s*ROW\s+", output)[1:]
    words = {}
    for block in blocks:
        row = re.match(r"(0x[0-9a-fA-F]+)(?::|\s)", block)
        values = re.findall(r"\bVALUE\s+(0x[0-9a-fA-F]+)\b", block)
        if not row or len(values) != 1:
            raise FirmwareError("Could not identify the recovery board.")
        index, value = int(row[1], 16), int(values[0], 16)
        if index in words or index not in range(4) or value > 0xffff:
            raise FirmwareError("Unexpected recovery board identity.")
        words[index] = value
    if len(words) != 4:
        raise FirmwareError("Incomplete recovery board identity.")
    found = "".join(f"{words[index]:04X}" for index in reversed(range(4)))
    if serial and found != serial:
        raise FirmwareError("Recovery serial does not match the selected board.")
    return [found]


def bootsel_boards(tool: str, serial: str | None = None) -> list[str]:
    try:
        output = run(tool, ["info", "-d", *selection(serial)], "Checking BOOTSEL mode...", 10)
    except PicotoolError as error:
        output = error.output
        if output.strip().startswith("ERROR: Block loop is not valid"):
            return recovery_board(tool, serial)
        # Only the plain no-device result means absence. Driver/access errors
        # carry extra diagnostics and must not trigger a mode change.
        if not re.fullmatch(r"\s*No accessible RP-series devices in BOOTSEL mode were found"
                            r"(?: with serial number [0-9a-fA-F]{16})?\.\s*", " ".join(output.split())):
            raise
        return []
    ids = re.findall(r"^\s*chipid:\s*(?:0x)?([0-9a-fA-F]{16})\s*$", output, re.MULTILINE)
    chips = re.findall(r"^\s*type:\s*(\S+)", output, re.MULTILINE)
    if not ids or len(ids) != len(chips) or any(chip != "RP2350" for chip in chips):
        raise FirmwareError("Could not identify the BOOTSEL board. Select an RP2350 with --serial.")
    ids = [value.upper() for value in ids]
    if serial and any(value != serial for value in ids):
        raise FirmwareError("BOOTSEL serial does not match the selected board.")
    return ids


def detect_board(tool: str, serial: str | None) -> tuple[str, str]:
    if serial is not None:
        serial = BootOtp(tool, serial).serial
    boot = bootsel_boards(tool, serial)
    # An explicit serial already identifies the target; no PC/SC is needed.
    if serial and boot == [serial]:
        return serial, "bootsel"
    normal = normal_boards()
    if serial:
        normal = [value for value in normal if value == serial]
    if len(boot) + len(normal) > 1:
        raise FirmwareError("Multiple boards found. Select one with --serial ID.")
    if boot:
        return boot[0], "bootsel"
    if normal:
        return normal[0], "normal"
    raise FirmwareError("Board not found. Connect Pico All and check --serial and USB drivers.")


def wait_for_mode(tool: str, serial: str, mode: str, timeout: float = 30) -> None:
    deadline = time.monotonic() + timeout
    console.print("Waiting for " + serial + " in " + mode + " mode...", style="dim")
    while time.monotonic() < deadline:
        boards = bootsel_boards(tool, serial) if mode == "bootsel" else normal_boards(waiting=True)
        if boards.count(serial) == 1:
            return
        time.sleep(0.5)
    raise FirmwareError(f"Board {serial} did not appear in {mode} mode. Check USB and retry; no further operation was started.")


def ensure_bootsel(tool: str, serial: str | None) -> str:
    serial, mode = detect_board(tool, serial)
    if mode != "bootsel":
        console.print("BOOTSEL is needed. When the LED flashes yellow, press and release BOOTSEL.", style="yellow")
        management(serial, [0x80, 0x1f, 1, 0, 0])
        wait_for_mode(tool, serial, "bootsel")
    return serial


def device_mode(tool: str, action: str, serial: str | None) -> None:
    if action == "bootsel":
        serial = ensure_bootsel(tool, serial)
        console.print(f"Board {serial} is ready in BOOTSEL mode.", style="green")
        return
    serial, mode = detect_board(tool, serial)
    if mode == "bootsel":
        run(tool, ["reboot", "-a", *selection(serial)], "Starting firmware...", 30)
    else:
        management(serial, [0x80, 0x1f, 0, 0, 0])
        # Firmware schedules a watchdog reboot after acknowledging the APDU.
        time.sleep(1)
    wait_for_mode(tool, serial, "normal")
    console.print(f"Board {serial} is running Pico All.", style="green")


def board_info(tool: str, serial: str | None) -> None:
    serial = ensure_bootsel(tool, serial)
    text = run(tool, ["info", "-b", "-l", "-d", *selection(serial)], "Reading board firmware...", 30)
    table = Table(title="Board firmware", show_header=False, box=None, padding=(0, 2))
    table.add_column(style="cyan")
    table.add_column(overflow="fold")
    labels = {"name": "Firmware", "version": "Version", "signature": "Signature",
              "pico_board": "Board", "sdk version": "SDK", "build date": "Built",
              "type": "Chip", "chipid": "Serial", "flash size": "Flash",
              "secure boot": "Secure boot"}
    seen = set()
    from rich.text import Text
    for line in text.splitlines():
        match = re.match(r"\s*([^:]+):\s+(\S.*)", line)
        if match:
            label, value = match.groups()
            label = label.strip().lower()
            if label in labels and label not in seen:
                table.add_row(Text(labels[label]), Text(value.strip()))
                seen.add(label)
    if not table.row_count:
        raise FirmwareError("No board information returned. Check BOOTSEL mode and --serial.")
    console.print(table)
    console.print(f"To start firmware: python firmware.py device reboot -s {serial}", style="dim")


def flash(tool: str, firmware: str, serial: str | None, yes: bool = False) -> None:
    source = uf2_file(firmware)
    run(tool, ["info", "-b", str(source)], "Checking firmware...")
    console.print("Firmware: " + str(source), markup=False)
    approved("Flash this firmware and restart the board?", yes)
    serial = ensure_bootsel(tool, serial)
    # picotool refuses ambiguous targets. Keep its partition checks and verify
    # every write; never use erase, ignore-partitions, or OTP commands.
    run(tool, ["load", "-v", "-x", str(source), *selection(serial)], "Flashing and verifying...", 180)
    console.print("Flashed and verified. Board restarted.", style="green")


# RP2350 predefined OTP layout (datasheet 13.10). Only public boot rows are
# accessible through this host interface; device-root pages are never dumped.
CRIT_ROWS = tuple(range(0x40, 0x48))
FLAGS_ROWS = (0x4b, 0x4c, 0x4d)
BOOT_LOCKS = (0xf83, 0xf85)
HARDEN_BITS = 0x74  # debug disabled, glitch detector enabled, sensitivity 3
BOOT_LOCK = 0x151515  # Secure/Non-secure/BOOTSEL read-only, three copies


def vote(values: list[int], threshold: int) -> int:
    return sum(1 << bit for bit in range(24)
               if sum(bool(value & (1 << bit)) for value in values) >= threshold)


class BootOtp:
    """Small picotool transport. Simulation replaces this boundary, not policy."""
    def __init__(self, tool: str, serial: str):
        if not re.fullmatch(r"[0-9a-fA-F]{16}", serial or ""):
            raise FirmwareError("Enter the board's 16-digit serial number.")
        self.tool, self.serial = tool, serial.upper()

    def read(self, row: int, ecc: bool = False) -> int:
        text = run(self.tool, ["otp", "get", "-c", "1", "-e" if ecc else "-r", "-n",
                              *selection(self.serial), hex(row)], "Reading security state...", 30)
        found = re.findall(r"\bVALUE\s+(0x[0-9a-fA-F]+)\b", text)
        rows = re.findall(r"^\s*ROW\s+(0x[0-9a-fA-F]+)(?::|\s*$)", text, re.MULTILINE)
        if len(found) != 1 or len(rows) != 1 or int(rows[0], 16) != row:
            raise FirmwareError(f"Could not read OTP row {row:#x}. No changes made by this read.")
        value = int(found[0], 16)
        if value > (0xffff if ecc else 0xffffff):
            raise FirmwareError("Unexpected OTP value. Check the picotool version.")
        return value

    def write(self, row: int, value: int, ecc: bool = False) -> None:
        # Policy calls only boot-key words, CRIT1, BOOT_FLAGS1 and their locks.
        allowed = (ecc and 0x80 <= row < 0xc0) or (not ecc and row in (*CRIT_ROWS, *FLAGS_ROWS, *BOOT_LOCKS))
        if not allowed:
            raise FirmwareError("Unsupported OTP write.")
        run(self.tool, ["otp", "set", "-c", "1", "-e" if ecc else "-r", hex(row), hex(value),
                        *selection(self.serial)], "Programming security settings...", 30)
        if self.read(row, ecc) != value:
            raise FirmwareError(f"OTP verification failed at {row:#x}. Stop and read status before retrying.")

    def board(self) -> str:
        text = run(self.tool, ["info", "-d", *selection(self.serial)], "Checking target board...", 30)
        if "RP2350" not in text or "Multiple RP-series" in text:
            raise FirmwareError("Connect exactly the selected RP2350 in BOOTSEL mode.")
        return text


def boot_state(otp: BootOtp) -> dict:
    critical = [otp.read(row) for row in CRIT_ROWS]
    flags = [otp.read(row) for row in FLAGS_ROWS]
    locks = [otp.read(row) for row in BOOT_LOCKS]
    # Lock0 can gate access with hardware keys; this workflow doesn't own them.
    key_locks = [otp.read(0xf82), otp.read(0xf84)]
    fps = []
    for slot in range(4):
        try:
            words = [otp.read(0x80 + 16 * slot + i, True) for i in range(16)]
            fps.append(b"".join(word.to_bytes(2, "little") for word in words))
        except FirmwareError:
            # Keep an unreadable slot distinct from a blank slot. A torn unused
            # slot must not prevent provisioning a different, readable slot.
            fps.append(None)
    flag = vote(flags, 2)
    return dict(critical=critical, flags=flags, locks=locks, key_locks=key_locks,
                crit=vote(critical, 3), valid=flag & 15, revoked=(flag >> 8) & 15, fps=fps)


def show_security(state: dict) -> None:
    table = Table(title="RP2350 security", show_header=False, box=None)
    crit = state["crit"]
    table.add_row("Secure Boot fuse", "Enabled" if crit & 1 else "Disabled")
    table.add_row("Debug fuse", "Disabled" if crit & 4 else "Enabled")
    table.add_row("Glitch detector", f"Enabled, sensitivity {(crit >> 5) & 3}" if crit & 16 else "Disabled")
    for i, fp in enumerate(state["fps"]):
        label = "Revoked" if state["revoked"] & (1 << i) else ("Trusted" if state["valid"] & (1 << i) else "Unused")
        table.add_row(f"Boot key {i}", "Unreadable / " + label if fp is None else
                      label + (" / " + fp.hex() if any(fp) else ""))
    table.add_row("Boot page locks", ", ".join(f"{lock:06x}" for lock in state["locks"]))
    console.print(table)
    console.print("Fuse settings take effect after a full power cycle.", style="dim")


def firmware_fingerprint(tool: str, path: Path) -> bytes:
    text = run(tool, ["info", "-m", str(path)], "Verifying signed firmware...")
    blocks = re.split(r"Metadata Block \d+", text)
    signed = [block for block in blocks if re.search(r"signature:\s+verified\b", block)]
    # Reject ambiguous multi-image inputs. The tool manages one RP2350 Arm image.
    if len(signed) != 1 or not re.search(r"target chip:\s+RP2350\b", signed[0]) or not re.search(r"image type:\s+ARM Secure\b", signed[0]):
        raise FirmwareError("Choose one verified, signed RP2350 Arm firmware image.")
    match = re.search(r"public key:\s+([0-9a-fA-F]{128})\b", signed[0])
    if not match:
        raise FirmwareError("Could not read the firmware signing public key.")
    # picotool hashes the 64-byte big-endian X || Y public key, without SEC1 prefix.
    return hashlib.sha256(bytes.fromhex(match[1])).digest()


def security_plan(otp: BootOtp, state: dict, action: str, fp: bytes, slot: int) -> list[tuple[int, int, bool]]:
    if slot not in range(4) or len(fp) != 32 or not any(fp):
        raise FirmwareError("Invalid boot key or slot.")
    if any(state["key_locks"]):
        raise FirmwareError("Boot pages use an existing hardware access-key policy.")
    writes = []
    def raw(row: int, old: int, bits: int):
        if old | bits != old:
            writes.append((row, old | bits, False))
    trusted = state["valid"] & ~state["revoked"] & 15
    matches = [i for i in range(4) if state["fps"][i] == fp and trusted & (1 << i)]
    if action == "load-key":
        if any(value & (1 << (slot + 8)) for value in state["flags"]):
            raise FirmwareError("This key slot is revoked. Choose an unused slot.")
        for i in range(16):
            row = 0x80 + 16 * slot + i
            wanted = int.from_bytes(fp[2*i:2*i+2], "little")
            if otp.read(row):
                if otp.read(row, True) != wanted:
                    raise FirmwareError("This key slot contains different or incomplete data. Choose another slot.")
            elif wanted:
                writes.append((row, wanted, True))
        # KEY_VALID is programmed only AFTER all fingerprint words read back.
        for row, old in zip(FLAGS_ROWS, state["flags"]):
            raw(row, old, 1 << slot)
    else:
        if len(matches) != 1:
            raise FirmwareError("The firmware signing key must match exactly one trusted OTP slot.")
        if action == "harden":
            for row, old in zip(CRIT_ROWS, state["critical"]):
                raw(row, old, HARDEN_BITS)
        elif action == "enable":
            if any(value & HARDEN_BITS != HARDEN_BITS for value in state["critical"]):
                raise FirmwareError("Run Harden and verify a power-cycle boot first.")
            if any(not value & (1 << matches[0]) for value in state["flags"]):
                raise FirmwareError("Finish Load key before enabling Secure Boot.")
            for row, old in zip(CRIT_ROWS, state["critical"]):
                raw(row, old, 1)
        elif action == "lock":
            if any(value & 0x75 != 0x75 for value in state["critical"]):
                raise FirmwareError("Enable Secure Boot and verify a power-cycle boot first.")
            if trusted != 1 << matches[0]:
                raise FirmwareError("More than one key is trusted. This lock workflow requires a single signing key.")
            revoke = (15 ^ trusted) << 8
            for row, old in zip(FLAGS_ROWS, state["flags"]):
                raw(row, old, revoke)
            for row, old in zip(BOOT_LOCKS, state["locks"]):
                if old & ~BOOT_LOCK:
                    raise FirmwareError("Boot pages have incompatible existing locks.")
                effective = vote([(old >> shift) & 255 for shift in (0, 8, 16)], 2)
                if effective != 0x15:
                    raw(row, old, BOOT_LOCK)
        else:
            raise FirmwareError("Unknown security action.")
    for row, _, _ in writes:
        lock_index = 1 if 0x80 <= row < 0xc0 or row == 0xf85 else 0
        if vote([(state["locks"][lock_index] >> shift) & 255 for shift in (0, 8, 16)], 2) & 0x30:
            raise FirmwareError("BOOTSEL cannot modify this locked page. Read status before continuing.")
    return writes


def boot_proof_path(serial: str) -> Path:
    return ROOT / ".private" / ("boot-check-" + serial.upper() + ".json")


MANAGEMENT_AID = [0, 0xa4, 4, 0, 8, 0xa0, 0x58, 0x3f, 0xc1, 0x9b, 0x7e, 0x4f, 0x21]


def management_access(serial: str | None = None, command: list[int] | None = None,
                      waiting: bool = False) -> bytes | list[str]:
    from smartcard.System import readers
    from smartcard.Exceptions import (CardConnectionException, NoCardException,
                                      NoReadersException, SmartcardException)
    if serial is not None:
        serial = BootOtp("", serial).serial
    connections, matches = [], []
    try:
        try:
            available = readers()
        except NoReadersException:
            available = []
        for reader in available:
            if "Pico All" not in str(reader):
                continue
            connection = reader.createConnection()
            connections.append(connection)
            try:
                connection.connect()
                data, sw1, sw2 = connection.transmit(MANAGEMENT_AID)
            except NoCardException:
                continue
            except CardConnectionException:
                if waiting:
                    continue  # A reader may still be disappearing after reboot.
                raise
            if (sw1, sw2) == (0x90, 0) and len(data) == 12:
                found = bytes(data[4:]).hex().upper()
                if serial is None or found == serial:
                    matches.append((found, connection))
        if command is None:
            return [found for found, _ in matches]
        if len(matches) != 1:
            raise FirmwareError("Connect exactly the selected board in normal mode. To leave BOOTSEL, use device reboot -s ID.")
        try:
            data, sw1, sw2 = matches[0][1].transmit(command)
        except CardConnectionException:
            # Reboot may remove CCID before Windows receives the response.
            # The caller must verify the same board in the destination mode.
            if command in ([0x80, 0x1f, 0, 0, 0], [0x80, 0x1f, 1, 0, 0]):
                return b""
            raise
        if (sw1, sw2) != (0x90, 0):
            raise FirmwareError(f"Board declined the operation ({sw1:02X}{sw2:02X}). Check its state or button confirmation.")
        return bytes(data)
    except SmartcardException as error:
        # Windows may briefly stop PC/SC while the last CCID device reboots.
        if waiting and (error.hresult & 0xffffffff) in (0x8010001d, 0x8010001e):
            return []
        raise FirmwareError("Could not communicate through PC/SC. Check the smart-card service and USB connection. " + str(error)) from None
    finally:
        for connection in connections:
            try:
                connection.disconnect()
            except SmartcardException:
                pass  # Removal during reboot must not replace the real result.


def normal_boards(waiting: bool = False) -> list[str]:
    return management_access(waiting=waiting)


def management(serial: str, command: list[int]) -> bytes:
    return management_access(serial, command)


def prepare_storage(serial: str, apply: bool) -> None:
    data = management(serial, [0x80, 0x1e, 6, 0, 0])
    if len(data) != 7 or data[0] != 1 or data[1] != 0 or int.from_bytes(data[3:], "big") & 1:
        raise FirmwareError("Preparation is allowed only before OTP root initialization and Secure Boot.")
    console.print("Erase all application credentials, PINs and settings, then return to BOOTSEL.", style="yellow")
    console.print("Firmware and OTP are retained. Do not reconnect normally before Enable.", style="dim")
    if not apply:
        console.print("Preview only. Add --apply to confirm preparation.", style="cyan")
        return
    phrase = "ERASE " + serial.upper()
    if not sys.stdin.isatty() or Prompt.ask("Type " + phrase) != phrase:
        raise FirmwareError("Cancelled.")
    console.print("Press and release BOOTSEL when the LED flashes yellow.", style="yellow")
    management(serial, [0x80, 0x1d, 0, 2, 0])
    console.print("Preparation requested. Wait for the BOOTSEL drive; Enable will verify that storage is empty.", style="green")


def require_empty_storage(tool: str, serial: str, source: Path) -> None:
    metadata = run(tool, ["info", "-m", str(source)], "Checking storage layout...")
    match = re.search(r"partition 1 \([^\n]*?\):\s+([0-9a-fA-F]{8})->([0-9a-fA-F]{8})", metadata)
    if not match:
        raise FirmwareError("Could not identify the Pico All data partition.")
    start, end = [int(part, 16) for part in match.groups()]
    # This workflow is intentionally bounded to the RP2350-One's 4 MiB flash.
    if start < 0x102000 or end != 0x400000 or start >= end - 0x2000 or start % 4096:
        raise FirmwareError("Unsupported credential storage layout. No OTP changes made.")
    start += 0x10000000
    end += 0x10000000 - 0x2000  # SDK reserves the last two sectors.
    private = ROOT / ".private"
    private.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="storage-check-", dir=private) as directory:
        target = Path(directory) / "storage.bin"
        run(tool, ["save", "-r", hex(start), hex(end), str(target), "-t", "bin", *selection(serial)],
            "Checking empty credential storage...", 60)
        if target.stat().st_size != end - start:
            raise FirmwareError("Could not read the complete credential area.")
        with target.open("rb") as handle:
            while chunk := handle.read(65536):
                if chunk != b"\xff" * len(chunk):
                    raise FirmwareError("Credential storage is not empty. Run security prepare before Enable.")


def prove_boot(tool: str, serial: str, firmware: str) -> None:
    # Read-only APDUs. The saved record is a local workflow check, not attestation.
    source = uf2_file(firmware)
    firmware_fingerprint(tool, source)
    BootOtp(tool, serial)  # Validate serial before constructing a local path.
    data = management(serial, [0x80, 0x1e, 6, 0, 0])
    if len(data) != 7 or data[0] != 1:
        raise FirmwareError("Update to firmware with OTP status support first.")
    critical = int.from_bytes(data[3:], "big")
    if data[1] != 1 or data[2] not in range(56, 59) or critical & 0x75 != 0x75:
        raise FirmwareError("Signed boot, hardening and OTP root are not all active.")
    matched = dict(serial=serial.upper(), sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                   critical=critical, root_page=data[2])
    path = boot_proof_path(serial)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(matched, indent=2) + "\n", encoding="utf-8")
    console.print("Secure boot and OTP root are active. Boot check saved.", style="green")


def security(tool: str, action: str, serial: str, firmware: str | None = None,
             slot: int = 0, apply: bool = False, gui_confirm=None) -> None:
    if action == "prepare":
        prepare_storage(serial, apply)
        return
    if action == "prove":
        if not firmware:
            raise FirmwareError("Pass the signed firmware as FILE after the security command.")
        prove_boot(tool, serial, firmware)
        return
    otp = BootOtp(tool, serial)
    ensure_bootsel(tool, otp.serial)
    board = otp.board()
    state = boot_state(otp)
    show_security(state)
    if action == "status":
        return
    if not firmware:
        raise FirmwareError("Pass the signed firmware as FILE after the security command.")
    source = uf2_file(firmware)
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    fp = firmware_fingerprint(tool, source)
    plan = security_plan(otp, state, action, fp, slot)
    console.print("Signing key: " + fp.hex(), markup=False)
    for row, value, ecc in plan:
        console.print(f"  {'ECC' if ecc else 'RAW'}  {row:#05x}  ->  {value:#08x}", style="dim")
    if not plan:
        console.print("This stage is already complete.", style="green")
        return
    effects = {
        "load-key": "Permanently register this signing key. Secure Boot remains unchanged.",
        "harden": "Permanently disable debug and enable maximum glitch sensitivity. Power-cycle and test before Enable.",
        "enable": "Permanently require signed firmware. OTP roots initialize on a protected boot with EMPTY credential storage.",
        "lock": "Permanently revoke every other signing-key slot and make boot configuration read-only. Key rotation ends here.",
    }
    console.print(effects[action], style="yellow")
    console.print("Signed BOOTSEL updates remain available. Keep your signing key backed up offline.", style="dim")
    if not apply:
        console.print("Preview only. Add --apply to review and confirm this stage.", style="cyan")
        return
    if action == "enable" and not state["crit"] & 1:
        require_empty_storage(tool, serial, source)
    # Verify the exact signed file is installed before any irreversible stage.
    run(tool, ["verify", str(source), *selection(serial)], "Checking installed firmware...", 60)
    if action == "lock":
        try:
            proof = json.loads(boot_proof_path(serial).read_text(encoding="utf-8"))
        except (OSError, ValueError):
            raise FirmwareError("Run the normal-mode boot check (security prove) before Lock.") from None
        if proof.get("serial") != serial.upper() or proof.get("sha256") != hashlib.sha256(source.read_bytes()).hexdigest():
            raise FirmwareError("Boot check does not match this board and firmware. Run security prove again.")
        if not re.search(r"secure boot:\s+1\b", board, re.I):
            raise FirmwareError("Secure Boot is not latched. Power-cycle and repeat the boot check.")
    if gui_confirm is not None:
        gui_confirm(action, serial, state, source_hash)
    else:
        if not sys.stdin.isatty():
            raise FirmwareError("Security writes require an interactive terminal. No --yes override.")
        if action in ("harden", "enable") and not Confirm.ask("Has this signed firmware booted and worked after the previous stage's power cycle?", default=False):
            raise FirmwareError("Cancelled. Test the current stage before continuing.")
        phrase = action.upper() + " " + serial.upper()
        if Prompt.ask("Type " + phrase) != phrase:
            raise FirmwareError("Cancelled.")
    # Re-read after the user reviews the plan; do not burn a stale plan.
    if boot_state(otp) != state:
        raise FirmwareError("Security state changed during confirmation. Read status and try again.")
    if hashlib.sha256(source.read_bytes()).hexdigest() != source_hash:
        raise FirmwareError("Firmware file changed during confirmation. Start this stage again.")
    run(tool, ["verify", str(source), *selection(serial)], "Rechecking installed firmware...", 60)
    for row, value, ecc in plan:
        otp.write(row, value, ecc)
    console.print("Stage programmed and read back. Fully unplug before testing the next stage.", style="green")


class HelpAction(argparse.Action):
    def __init__(self, option_strings, dest=argparse.SUPPRESS, **kwargs):
        super().__init__(option_strings, dest, nargs=0, default=argparse.SUPPRESS, **kwargs)

    def __call__(self, parser, namespace, values, option_string=None):
        parser.print_help(detailed=option_string == "--help")
        parser.exit()


class CliParser(argparse.ArgumentParser):
    def __init__(self, *args, details="", examples="", **kwargs):
        kwargs.update(add_help=False, allow_abbrev=False,
                      formatter_class=argparse.RawDescriptionHelpFormatter)
        super().__init__(*args, **kwargs)
        self.details, self.examples = details, examples
        help_group = self.add_argument_group("Help")
        help_group.add_argument("-h", "--help", action=HelpAction,
                               help="Short help (-h) or detailed help (--help)")

    def format_help(self, detailed=False):
        order = ["positional arguments", "Commands", "Security commands", "Signing options",
                 "Key options", "Output options", "Connection options", "Confirmation options", "Help"]
        self._action_groups.sort(key=lambda group: order.index(group.title) if group.title in order else -1)
        description, epilog = self.description, self.epilog
        if detailed:
            self.description = "\n\n".join(x for x in (description, self.details) if x)
            self.epilog = "Examples:\n" + self.examples if self.examples else None
        else:
            self.epilog = "Use --help for details and examples."
        try:
            return super().format_help()
        finally:
            self.description, self.epilog = description, epilog

    def print_help(self, file=None, detailed=False):
        output = Console(file=file or sys.stdout, highlight=False)
        for line in self.format_help(detailed).splitlines():
            output.print(line, style="bold cyan" if line.endswith(":") else None, markup=False)

    def error(self, message):
        self.print_usage(sys.stderr)
        self.exit(2, f"{self.prog}: error: {message}\nTry '{self.prog} --help' for details.\n")


def parser() -> argparse.ArgumentParser:
    top = CliParser(prog="firmware.py", description="Manage Pico All firmware and board security.",
                    details="Commands request BOOTSEL when needed: press and release the button when the LED flashes yellow.\nSecurity Prepare and Prove use normal mode; device reboot returns there without flashing.",
                    examples="  python firmware.py info\n"
                             "  python firmware.py sign firmware.uf2 -k .private/key.pem\n"
                             "  python firmware.py flash firmware.signed.uf2\n"
                             "  python firmware.py security -h")
    top.set_defaults(picotool=None, serial=None, action=None)

    def connection(command, device=False):
        group = command.add_argument_group("Connection options")
        group.add_argument("--picotool", metavar="PATH", default=argparse.SUPPRESS,
                           help="picotool executable (or PICOTOOL / PATH)")
        if device:
            group.add_argument("-s", "--serial", metavar="ID", default=argparse.SUPPRESS,
                               help="Target board serial (required)" if device == "required" else "Target board serial (auto if omitted)")

    def child(parent, name, summary, details="", examples="", device=False):
        command = parent.add_parser(name, help=summary, description=summary,
                                    details=details, examples=examples)
        connection(command, device)
        return command

    connection(top)
    commands = top.add_subparsers(dest="command", title="Commands", metavar="COMMAND")
    child(commands, "info", "Read board firmware information",
          "Switches to BOOTSEL if needed and stays there. Use device reboot to return to firmware.\nUse --serial when more than one board is connected.",
          "  python firmware.py info -s 0011223344556677", device=True)
    signer = child(commands, "sign", "Sign a UF2 with a local key",
                   "Use a secp256k1 PEM private key. --new-key creates a key and never overwrites one.\n"
                   "The output is verified before it replaces a file. Without -o, the output is FILE.signed.uf2.\n"
                   "Default key: .private/firmware-signing.pem beside this script. Keep its backup offline.",
                   "  python firmware.py sign firmware.uf2 -k .private/key.pem --new-key\n"
                   "  python firmware.py sign firmware.uf2 -k .private/key.pem -o signed.uf2")
    signer.add_argument("firmware", metavar="FILE", help="Input UF2")
    signing = signer.add_argument_group("Signing options")
    signing.add_argument("-k", "--key", metavar="PEM", default=str(DEFAULT_KEY), help="Local secp256k1 private key")
    signing.add_argument("--new-key", action="store_true", help="Create the signing key")
    output = signer.add_argument_group("Output options")
    output.add_argument("-o", "--output", metavar="FILE", help="Signed UF2 destination")
    output.add_argument("-y", "--yes", action="store_true", help="Allow replacing the output without a prompt")
    updater = child(commands, "flash", "Flash a UF2, verify it and restart",
                    "Requests BOOTSEL with board-button confirmation if needed. Writes are read back before restart.\n"
                    "--yes skips the flash prompt, but not the board button or security confirmations.",
                    "  python firmware.py flash signed.uf2 -s 0011223344556677", device=True)
    updater.add_argument("firmware", metavar="FILE", help="Firmware UF2")
    updater.add_argument_group("Confirmation options").add_argument(
        "-y", "--yes", action="store_true", help="Skip the flash confirmation")
    device = child(commands, "device", "Switch board modes",
                   "BOOTSEL entry requires a button press in normal firmware. Reboot starts installed firmware.\n"
                   "No firmware is flashed. Starting firmware performs its usual boot initialization.",
                   "  python firmware.py device bootsel\n  python firmware.py device reboot", device=True)
    modes = device.add_subparsers(dest="action", title="Device commands", metavar="COMMAND")
    for name, summary, detail in [
        ("bootsel", "Enter firmware update mode", "Requests board-button confirmation if needed, then waits for the same serial in BOOTSEL."),
        ("reboot", "Start or restart normal firmware", "Leaves idle BOOTSEL without flashing or pressing RESET. Waits for Pico All to reconnect.\n"
         "Do not run between Security Prepare and Enable: starting firmware can recreate application data."),
    ]:
        mode = child(modes, name, summary, detail,
                     f"  python firmware.py device {name} -s 0011223344556677", device=True)
        mode.set_defaults(selected_parser=mode)
    device.set_defaults(selected_parser=device)
    secure = child(commands, "security", "Inspect and configure RP2350 security",
                   "Stages: load-key -> harden -> prepare -> enable -> prove -> lock.\n"
                   "Power-cycle and test between irreversible stages. Prepare and Prove use normal mode;\n"
                   "the other stages request BOOTSEL if needed. Every stage requires an exact --serial.\n"
                   "Writes default to a preview. --apply enables interactive confirmation, never bypasses it.",
                   "  python firmware.py security status -s 0011223344556677\n"
                   "  python firmware.py security enable --help\n"
                   "  python firmware.py security load-key signed.uf2 -s 0011223344556677", device="required")
    actions = secure.add_subparsers(dest="action", title="Security commands", metavar="COMMAND")
    stages = [
        ("status", "Read security fuse settings", "Read-only. Settings stored in fuses may require a power cycle to take effect."),
        ("load-key", "Register a firmware signing key", "Register the public-key fingerprint from a verified signed UF2.\nThis permanently uses an OTP key slot. It does not enable Secure Boot."),
        ("harden", "Disable debug and enable glitch detection", "Permanently disable debug and enable maximum glitch sensitivity.\nVerify a power-cycle boot before continuing to Enable."),
        ("prepare", "Clear application data before OTP setup", "Normal mode. Deletes application credentials, PINs and settings after typed and board-button confirmation.\nThe board returns to BOOTSEL. Continue to Enable without starting the application again."),
        ("enable", "Enable signed boot and automatic OTP setup", "Permanently require signed firmware. The installed image must match FILE.\nFirst-time setup requires empty credential storage and completed hardening.\nAfter a protected boot, firmware initializes the device roots automatically."),
        ("prove", "Check signed boot and OTP root activation", "Read-only board check in normal mode. Saves a local boot-check record for FILE and this board.\nLock requires this record and verifies that the same firmware is still installed."),
        ("lock", "Finalize boot protection and trusted keys", "Permanently revoke every other signing-key slot and make boot configuration read-only.\nKey rotation ends here. Keep the signing key backed up offline; signed BOOTSEL updates remain available."),
    ]
    for name, summary, detail in stages:
        needs_file = name not in ("status", "prepare")
        writes = name not in ("status", "prove")
        example = f"  python firmware.py security {name}" + (" signed.uf2" if needs_file else "") + " -s 0011223344556677"
        if writes:
            detail += "\nDefault: preview only. Add --apply to review and confirm; an interactive terminal is required."
            example += "\n" + example + " --apply"
        action = child(actions, name, summary, detail, example, device="required")
        if needs_file:
            action.add_argument("firmware", metavar="FILE", help="Signed UF2 installed on the board")
        if name == "load-key":
            action.add_argument_group("Key options").add_argument(
                "--slot", type=int, choices=range(4), default=0, help="Boot key slot (default: 0)")
        if writes:
            action.add_argument_group("Confirmation options").add_argument(
                "--apply", action="store_true", help="Review and confirm this stage (default: preview)")
        action.set_defaults(selected_parser=action)
    secure.set_defaults(selected_parser=secure)
    return top


def main(argv: list[str] | None = None) -> int:
    cli = parser()
    arguments = cli.parse_args(argv)
    if arguments.command is None:
        cli.print_help()
        return 0
    if arguments.command in ("security", "device"):
        if arguments.action is None:
            arguments.selected_parser.print_help()
            return 0
        if arguments.command == "security" and not arguments.serial:
            arguments.selected_parser.error("the following argument is required: -s/--serial")
    try:
        # Normal-mode APDUs do not need the picotool executable.
        if arguments.command == "security" and arguments.action == "prepare":
            prepare_storage(arguments.serial, arguments.apply)
            return 0
        tool = tool_path(arguments.picotool)
        if arguments.command == "info":
            board_info(tool, arguments.serial)
        elif arguments.command == "sign":
            sign(tool, arguments.firmware, arguments.key, arguments.output, arguments.new_key, arguments.yes)
        elif arguments.command == "flash":
            flash(tool, arguments.firmware, arguments.serial, arguments.yes)
        elif arguments.command == "device":
            device_mode(tool, arguments.action, arguments.serial)
        elif arguments.command == "security":
            security(tool, arguments.action, arguments.serial, getattr(arguments, "firmware", None),
                     getattr(arguments, "slot", 0), getattr(arguments, "apply", False))
        return 0
    except KeyboardInterrupt:
        error_console.print("Interrupted. Check the board before retrying. Use device reboot to leave idle BOOTSEL.", style="yellow")
        return 130
    except (FirmwareError, OSError, EOFError) as error:
        error_console.print(str(error) or "Input closed.", style="red", markup=False)
        return 1
    except ImportError:
        error_console.print("Install dependencies: python -m pip install -r requirements.txt", style="red")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
