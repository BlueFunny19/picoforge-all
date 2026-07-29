#!/bin/sh

set -eu

REPO="librekeys/picoforge"
APP_NAME="picoforge"
APP_ID="in.suyogtandel.picoforge"
MIN_GLIBC="2.28"
INSTALL_DIR="${HOME}/.local/bin"
ICON_BASE="${HOME}/.local/share/icons/hicolor"
DESKTOP_DIR="${HOME}/.local/share/applications"

if [ -t 1 ]; then
	BOLD=$(printf '\033[1m')
	GREEN=$(printf '\033[0;32m')
	RED=$(printf '\033[0;31m')
	RESET=$(printf '\033[0m')
fi

log()   { printf '%s\n' "$*"; }
warn()  { printf '%swarning:%s %s\n' "${BOLD:-}" "${RESET:-}" "$*" >&2; }
err()   { printf '%serror:%s %s\n' "${RED:-}" "${RESET:-}" "$*" >&2; exit 1; }
success() { printf '%s%s%s\n' "${GREEN:-}" "$*" "${RESET:-}"; }

require() {
	command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"
}

_fetch() {
	url="$1"
	case "$FETCH" in
		curl) command curl -fSL "$url" ;;
		wget) wget -O- "$url" ;;
	esac
}

_resolve_url() {
	url="$1"
	case "$FETCH" in
		curl) curl -fsSL -o /dev/null -w '%{url_effective}' "$url" ;;
		wget)
			_ru=$(wget -q -O /dev/null --server-response --max-redirect=30 "$url" 2>&1)
			_rl=$(printf '%s\n' "$_ru" | grep -i '^[[:space:]]*Location:' | tail -1 | sed 's/^[[:space:]]*[Ll]ocation:[[:space:]]*//')
			[ -n "$_rl" ] || return 1
			printf '%s\n' "$_rl"
			;;
	esac
}

detect_arch() {
	case "$(uname -m)" in
		x86_64) echo "x86-64" ;;
		aarch64|arm64) echo "aarch64" ;;
		*) err "unsupported architecture: $(uname -m)" ;;
	esac
}

detect_libc() {
	if command -v ldd >/dev/null 2>&1; then
		_ldd=$(ldd --version 2>&1 || true)
		case "$_ldd" in
			*musl*) echo "musl"; return 0 ;;
			*GNU*|*GLIBC*) echo "glibc"; return 0 ;;
		esac
	fi
	if command -v getconf >/dev/null 2>&1 && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
		echo "glibc"; return 0
	fi
	for _f in /lib/ld-musl-*.so.1 /lib64/ld-musl-*.so.1 /usr/lib/ld-musl-*.so.1; do
		[ -e "$_f" ] && { echo "musl"; return 0; }
	done
	err "unable to determine libc implementation"
}

check_glibc_version() {
	[ "$1" = "glibc" ] || return 0
	command -v ldd >/dev/null 2>&1 || return 0
	_ver=$(ldd --version 2>&1 | head -n1 | grep -oE '[0-9]+\.[0-9]+$' || true)
	[ -n "$_ver" ] || return 0
	command -v sort >/dev/null 2>&1 || return 0
	_low=$(printf '%s\n%s\n' "$_ver" "$MIN_GLIBC" | sort -V | head -n1)
	[ "$_low" = "$MIN_GLIBC" ] || err "glibc ${_ver} detected, ${APP_NAME} requires >= ${MIN_GLIBC}"
}

print_path_help() {
	case "${SHELL:-}" in
		*zsh)
			log "  export PATH=\"$1:\$PATH\" >> ~/.zshrc"
			log "  source ~/.zshrc"
			;;
		*fish)
			log "  fish_add_path -U \"$1\""
			;;
		*)
			log "  export PATH=\"$1:\$PATH\" >> ~/.bashrc"
			log "  source ~/.bashrc"
			;;
	esac
}

print_help() {
	cat <<-EOF
	Usage: sh install.sh [OPTION]

	Install or remove ${APP_NAME}.

	Options:
	  --help, -h      Show this help message
	  --uninstall     Remove ${APP_NAME} and all installed files

	Without any options, installs the latest version of ${APP_NAME}.
	EOF
}

uninstall() {
	_found=0

	if [ -f "${INSTALL_DIR}/${APP_NAME}.AppImage" ]; then
		rm -f "${INSTALL_DIR}/${APP_NAME}.AppImage"
		_found=1
	fi

	if [ -f "${DESKTOP_DIR}/${APP_ID}.desktop" ]; then
		rm -f "${DESKTOP_DIR}/${APP_ID}.desktop"
		_found=1
	fi

	for _icon in "${ICON_BASE}/scalable/apps/${APP_ID}.svg" \
	             "${ICON_BASE}/256x256/apps/${APP_ID}.png"; do
		if [ -f "$_icon" ]; then
			rm -f "$_icon"
			_found=1
		fi
	done

	if command -v update-desktop-database >/dev/null 2>&1; then
		update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
	fi

	if [ "$_found" = "1" ]; then
		success "uninstalled ${APP_NAME}"
	else
		warn "${APP_NAME} is not installed"
	fi
}

main() {
	case "${1:-}" in
		--help|-h)
			print_help
			exit 0
			;;
		--uninstall)
			uninstall
			exit 0
			;;
		--*)
			err "unknown option: $1 (use --help for usage)"
			;;
	esac

	FETCH=""
	if command -v curl >/dev/null 2>&1; then
		FETCH="curl"
	elif command -v wget >/dev/null 2>&1; then
		FETCH="wget"
	else
		err "required: curl or wget (neither found)"
	fi

	require sed
	require grep
	require chmod
	require mkdir
	require find

	_tmp_base="${TMPDIR:-/tmp}"
	TMP_DIR=$(mktemp -d "${_tmp_base}/${APP_NAME}-install-XXXXXX")

	cleanup() {
		_ec=$?
		[ -n "${TMP_DIR:-}" ] && rm -rf "$TMP_DIR"
		exit "$_ec"
	}
	trap cleanup EXIT

	ARCH=$(detect_arch)
	LIBC=$(detect_libc)
	check_glibc_version "$LIBC"

	[ "$LIBC" = "musl" ] && LIBC_TAG="musl" || LIBC_TAG="glibc-${MIN_GLIBC}"

	_latest=$(_resolve_url "https://github.com/${REPO}/releases/latest") || err "could not reach github, check network connection"
	VERSION_TAG="${_latest##*/}"
	[ -n "$VERSION_TAG" ] || err "could not determine latest release"
	VERSION="${VERSION_TAG#v}"

	ASSET_NAME="${APP_NAME}_${VERSION}_${LIBC_TAG}_${ARCH}.AppImage"
	DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION_TAG}/${ASSET_NAME}"
	CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION_TAG}/SHA256SUMS"

	log "detected: ${ARCH} / ${LIBC_TAG}"
	log "installing ${APP_NAME} ${VERSION}"

	_fetch "$DOWNLOAD_URL" > "${TMP_DIR}/${ASSET_NAME}" || err "failed to download ${DOWNLOAD_URL}"

	if _fetch "$CHECKSUMS_URL" > "${TMP_DIR}/SHA256SUMS" 2>/dev/null; then
		_expected=$(grep "$ASSET_NAME" "${TMP_DIR}/SHA256SUMS" | sed 's/ .*//' || true)
		if [ -n "$_expected" ]; then
			_actual=""
			if command -v sha256sum >/dev/null 2>&1; then
				_actual=$(sha256sum "${TMP_DIR}/${ASSET_NAME}" | sed 's/ .*//')
			elif command -v shasum >/dev/null 2>&1; then
				_actual=$(shasum -a 256 "${TMP_DIR}/${ASSET_NAME}" | sed 's/ .*//')
			fi
			if [ -n "$_actual" ]; then
				[ "$_expected" = "$_actual" ] || err "checksum mismatch for ${ASSET_NAME}"
				log "checksum verified"
			else
				warn "no sha256 utility found, skipping checksum verification"
			fi
		else
			warn "${ASSET_NAME} not listed in SHA256SUMS, skipping checksum verification"
		fi
	else
		warn "SHA256SUMS not found, skipping checksum verification"
	fi

	HAS_FUSE=0
	if [ -e /dev/fuse ] && { command -v fusermount >/dev/null 2>&1 || command -v fusermount3 >/dev/null 2>&1; }; then
		HAS_FUSE=1
	else
		warn "fuse not detected, ${APP_NAME} will launch via --appimage-extract-and-run"
	fi

	chmod +x "${TMP_DIR}/${ASSET_NAME}"

	(
		cd "$TMP_DIR"
		"./${ASSET_NAME}" --appimage-extract >/dev/null 2>&1
	) || err "failed to extract appimage contents"

	EXTRACT_DIR="${TMP_DIR}/squashfs-root"
	[ -d "$EXTRACT_DIR" ] || err "extraction directory missing after extract"

	DESKTOP_SRC=$(find "$EXTRACT_DIR" -maxdepth 1 -name '*.desktop' | head -n1)
	[ -n "$DESKTOP_SRC" ] || err "no .desktop file found in appimage"

	ICON_SRC=$(find "$EXTRACT_DIR" -maxdepth 1 \( -name '*.png' -o -name '*.svg' \) | head -n1)
	[ -n "$ICON_SRC" ] || err "no icon file found in appimage"

	ICON_EXT="${ICON_SRC##*.}"

	case "$ICON_EXT" in
		svg) _ICON_DIR="${ICON_BASE}/scalable/apps" ;;
		*)   _ICON_DIR="${ICON_BASE}/256x256/apps" ;;
	esac

	for _d in "$INSTALL_DIR" "$_ICON_DIR" "$DESKTOP_DIR"; do
		mkdir -p "$_d" 2>/dev/null || err "cannot create directory: $_d (check permissions)"
	done

	cp "${TMP_DIR}/${ASSET_NAME}" "${INSTALL_DIR}/${APP_NAME}.AppImage"
	chmod +x "${INSTALL_DIR}/${APP_NAME}.AppImage"

	cp "$ICON_SRC" "${_ICON_DIR}/${APP_ID}.${ICON_EXT}"

	EXEC_LINE="${INSTALL_DIR}/${APP_NAME}.AppImage"
	[ "$HAS_FUSE" = "1" ] || EXEC_LINE="${EXEC_LINE} --appimage-extract-and-run"

	sed \
		-e "s|^Exec=.*|Exec=${EXEC_LINE}|" \
		-e "s|^Icon=.*|Icon=${_ICON_DIR}/${APP_ID}.${ICON_EXT}|" \
		"$DESKTOP_SRC" > "${DESKTOP_DIR}/${APP_ID}.desktop"

	chmod +x "${DESKTOP_DIR}/${APP_ID}.desktop"

	if command -v update-desktop-database >/dev/null 2>&1; then
		update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
	fi

	if command -v gtk-update-icon-cache >/dev/null 2>&1; then
		gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
	fi

	case ":${PATH}:" in
		*":${INSTALL_DIR}"*) ;;
		*)
			warn "${INSTALL_DIR} is not in PATH"
			print_path_help "$INSTALL_DIR"
			;;
	esac

	success "installed ${APP_NAME} ${VERSION} to ${INSTALL_DIR}/${APP_NAME}.AppImage"
}

main "$@"
