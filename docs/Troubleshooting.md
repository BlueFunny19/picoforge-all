# Troubleshooting

## Device not detected

Reconnect the key, select Refresh, and close other applications that may hold its smart-card connection. On Windows, make sure the Smart Card (`SCardSvr`) service is available. On Linux, install and run `pcscd` with a recent `libccid` and grant the user access to the device.

An **Online · FIDO** connection exposes only FIDO features. PIV, OpenPGP, HSM and hardware configuration need the PC/SC connection. Custom VID/PID values may need a local libccid configuration or udev rule; do not change the device identity just to imitate another vendor.

## Firmware tools unavailable

Install Raspberry Pi picotool 2.3.1 or later and put it on PATH, or set `PICOTOOL` to its full path before starting PicoForge All. Use one RP2350 ARM UF2 image for signing. A board with Secure Boot enabled requires its original trusted signing key.

## Button confirmation timed out

When the light flashes, press and release the device button (BOOTSEL). Matching Pico All firmware defaults to 60 seconds. Retry the operation after a timeout.

## Event time is unknown

Enable device clock synchronization under Software → Settings, then reconnect or refresh. Old events and events written before synchronization cannot be assigned a reliable calendar date. Select an IANA time zone to change how known dates are displayed.

## PIN or reset errors

FIDO has no factory PIN. Other applets only fill a default when the device confirms it is unchanged. A reset code must be set before it can unblock OpenPGP; the admin PIN is a separate recovery option. Avoid repeated guesses because they consume retry attempts.

Include your application version, firmware version and relevant console errors in an [issue](https://github.com/BlueFunny19/picoforge-all/issues). Do not attach secrets.
