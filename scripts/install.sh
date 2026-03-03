#!/bin/sh
# AKM (Agent Kit Manager) — Binary installer
# Downloads the latest release from GitHub and installs to ~/.local/bin.
#
# Usage:
#   curl -fsSL https://akm.raphaelsimon.fr/install | sh
#
# Options (via environment variables):
#   AKM_VERSION    — Install a specific version (e.g., "1.0.0" or "v1.0.0")
#   AKM_INSTALL_DIR — Install directory (default: ~/.local/bin)
set -eu

REPO="akm-rs/akm-rs"
INSTALL_DIR="${AKM_INSTALL_DIR:-$HOME/.local/bin}"

# --- Helpers ---

info() {
  printf '%s\n' "$@"
}

error() {
  printf 'Error: %s\n' "$@" >&2
  exit 1
}

# --- Platform detection ---

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)  ;;
    Darwin) error "macOS is not yet supported. Install via: cargo install akm" ;;
    *)      error "Unsupported OS: $os. Install via: cargo install akm" ;;
  esac

  case "$arch" in
    x86_64|amd64) ;;
    aarch64|arm64) error "ARM64 is not yet supported. Install via: cargo install akm" ;;
    *)             error "Unsupported architecture: $arch. Install via: cargo install akm" ;;
  esac

  PLATFORM="linux"
  ARCH="x86_64"
  ASSET_NAME="akm-${PLATFORM}-${ARCH}"
}

# --- Resolve version ---

resolve_version() {
  if [ -n "${AKM_VERSION:-}" ]; then
    # Strip leading 'v' if present
    VERSION="${AKM_VERSION#v}"
    TAG="v${VERSION}"
  else
    info "Fetching latest release..."
    local api_response http_code
    api_response="$(mktemp)"
    http_code="$(curl -sSL -w '%{http_code}' \
      -o "$api_response" \
      "https://api.github.com/repos/${REPO}/releases/latest" \
      -H "Accept: application/vnd.github+json")" \
      || error "Failed to connect to GitHub API. Check your network connection."

    if [ "$http_code" -lt 200 ] || [ "$http_code" -ge 300 ]; then
      rm -f "$api_response"
      error "GitHub API returned HTTP ${http_code}. The repository may not exist or you may be rate-limited."
    fi

    local response
    response="$(cat "$api_response")"
    rm -f "$api_response"

    TAG="$(printf '%s' "$response" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
    if [ -z "$TAG" ]; then
      error "Could not determine latest version from GitHub API response."
    fi
    VERSION="${TAG#v}"
  fi

  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}"
  CHECKSUM_URL="${DOWNLOAD_URL}.sha256"
}

# --- Check dependencies ---

check_deps() {
  if ! command -v curl >/dev/null 2>&1; then
    error "curl is required but not installed. Install curl and try again."
  fi
}

# --- Download and install ---

download_and_install() {
  local tmpdir
  tmpdir="$(mktemp -d)" || error "Failed to create temporary directory."
  trap 'rm -rf "${tmpdir:-}"' EXIT

  info "Downloading akm ${VERSION}..."
  local http_code
  http_code="$(curl -sSL --retry 3 --retry-delay 2 -w '%{http_code}' \
    -o "${tmpdir}/${ASSET_NAME}" "${DOWNLOAD_URL}")" \
    || error "Download failed. Check your network connection. URL: ${DOWNLOAD_URL}"

  if [ "$http_code" -lt 200 ] || [ "$http_code" -ge 300 ]; then
    rm -f "${tmpdir}/${ASSET_NAME}"
    error "Download failed with HTTP ${http_code}. Version ${VERSION} may not exist."
  fi

  # Verify file is non-empty and reasonably sized (> 64 KB)
  local file_size
  file_size="$(wc -c < "${tmpdir}/${ASSET_NAME}")"
  if [ "$file_size" -lt 65536 ]; then
    error "Downloaded file is too small (${file_size} bytes). The download may be corrupt."
  fi

  # Verify checksum if sha256sum is available
  if command -v sha256sum >/dev/null 2>&1; then
    info "Verifying checksum..."
    if curl -fsSL -o "${tmpdir}/${ASSET_NAME}.sha256" "${CHECKSUM_URL}" 2>/dev/null; then
      (cd "$tmpdir" && sha256sum -c "${ASSET_NAME}.sha256") \
        || error "Checksum verification failed. The download may be corrupt."
    else
      info "Warning: Checksum file not available. Skipping verification."
    fi
  else
    info "Warning: sha256sum not found. Skipping checksum verification."
  fi

  # Install
  mkdir -p "$INSTALL_DIR" || error "Failed to create install directory: ${INSTALL_DIR}"
  mv "${tmpdir}/${ASSET_NAME}" "${INSTALL_DIR}/akm" \
    || error "Failed to install binary to ${INSTALL_DIR}/akm. Check permissions."
  chmod +x "${INSTALL_DIR}/akm"

  info "Installed akm ${VERSION} to ${INSTALL_DIR}/akm"
}

# --- Post-install checks ---

post_install() {
  # Check PATH
  case ":$PATH:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      info ""
      info "Warning: ${INSTALL_DIR} is not on your PATH."
      info "Add to your shell profile:"
      info "  export PATH=\"${INSTALL_DIR}:\$PATH\""
      info ""
      ;;
  esac

  # Check for existing Bash version
  if [ -f "${XDG_DATA_HOME:-$HOME/.local/share}/akm/shell/akm-init.sh" ]; then
    info "Note: An existing AKM shell init was detected."
    info "The Rust version uses 'akm setup' to configure shell integration."
    info ""
  fi

  info ""
  info "Run 'akm setup' to configure features and wire shell integration."
}

# --- Main ---

main() {
  check_deps
  detect_platform
  resolve_version
  download_and_install
  post_install
}

# Allow sourcing for tests: AKM_DRY_RUN=1 source scripts/install.sh
if [ -z "${AKM_DRY_RUN:-}" ]; then
  main
fi
