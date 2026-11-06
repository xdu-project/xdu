#!/bin/sh
# xdu installer script
# Usage: curl -sSfL https://xdu-project.org/install.sh | sh
#
# Environment variables:
#   XDU_VERSION  - Version to install (default: latest)
#   XDU_INSTALL  - Installation directory for binaries (default: ~/.local/bin)
#   XDU_PREFIX   - Prefix for share/ files: man pages, completions (default: ~/.local)
#
# The DuckDB extensions a release ships go to lib/xdu/extensions beside the directory the
# binaries are installed into (XDU_INSTALL/../lib/xdu/extensions), where the tools look for them.

set -eu

REPO="xdu-project/xdu"
BINARIES="xdu xdu-find xdu-view xdu-rm xdu-cp xdu-mv"

# Colors for output (disabled if not a terminal)
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    NC='\033[0m' # No Color
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    NC=''
fi

info() {
    printf "${BLUE}info${NC}: %s\n" "$1"
}

warn() {
    printf "${YELLOW}warn${NC}: %s\n" "$1" >&2
}

error() {
    printf "${RED}error${NC}: %s\n" "$1" >&2
    exit 1
}

success() {
    printf "${GREEN}success${NC}: %s\n" "$1"
}

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        *)       error "Unsupported operating system: $(uname -s)" ;;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *)             error "Unsupported architecture: $(uname -m)" ;;
    esac
}

# Get the latest release version from GitHub
get_latest_version() {
    if command -v curl >/dev/null 2>&1; then
        curl -sSfL "https://api.github.com/repos/${REPO}/releases/latest" |
            grep '"tag_name":' |
            sed -E 's/.*"([^"]+)".*/\1/'
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" |
            grep '"tag_name":' |
            sed -E 's/.*"([^"]+)".*/\1/'
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

# Download a file
download() {
    url="$1"
    output="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -sSfL "$url" -o "$output"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$url" -O "$output"
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

# Main installation function
main() {
    OS=$(detect_os)
    ARCH=$(detect_arch)

    info "Detected OS: ${OS}, Arch: ${ARCH}"

    # Determine version to install
    VERSION="${XDU_VERSION:-}"
    if [ -z "$VERSION" ]; then
        info "Fetching latest release..."
        VERSION=$(get_latest_version)
        if [ -z "$VERSION" ]; then
            error "Could not determine latest version. Set XDU_VERSION to install a specific version."
        fi
    fi

    info "Installing xdu ${VERSION}"

    # Determine installation directories
    PREFIX="${XDU_PREFIX:-$HOME/.local}"
    INSTALL_DIR="${XDU_INSTALL:-${PREFIX}/bin}"

    # Create installation directory if it doesn't exist
    if [ ! -d "$INSTALL_DIR" ]; then
        info "Creating installation directory: ${INSTALL_DIR}"
        mkdir -p "$INSTALL_DIR"
    fi

    # Construct target triple
    case "${OS}-${ARCH}" in
        linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
        linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
        macos-x86_64)   TARGET="x86_64-apple-darwin" ;;
        macos-aarch64)  TARGET="aarch64-apple-darwin" ;;
        *)              error "Unsupported platform: ${OS}-${ARCH}" ;;
    esac

    # Construct download URL
    ARCHIVE_NAME="xdu-${VERSION}-${TARGET}.tar.gz"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE_NAME}"

    # Create temporary directory
    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT

    ARCHIVE_PATH="${TMPDIR}/${ARCHIVE_NAME}"

    info "Downloading ${DOWNLOAD_URL}"
    if ! download "$DOWNLOAD_URL" "$ARCHIVE_PATH"; then
        error "Failed to download ${DOWNLOAD_URL}"
    fi

    # Verify the download against SHA256SUMS from the same release BEFORE extracting.
    # Every release publishes a single SHA256SUMS covering all tarballs; abort on any
    # mismatch (or if it cannot be fetched/verified) rather than extract untrusted bytes.
    SUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS"
    SUMS_PATH="${TMPDIR}/SHA256SUMS"

    info "Downloading checksums ${SUMS_URL}"
    if ! download "$SUMS_URL" "$SUMS_PATH"; then
        error "Failed to download checksums ${SUMS_URL}"
    fi

    # Reduce SHA256SUMS to just our archive's line so the verify tool checks only it.
    EXPECTED_PATH="${TMPDIR}/SHA256SUMS.expected"
    if ! grep "${ARCHIVE_NAME}$" "$SUMS_PATH" > "$EXPECTED_PATH"; then
        error "No checksum listed for ${ARCHIVE_NAME} in SHA256SUMS"
    fi

    info "Verifying checksum for ${ARCHIVE_NAME}"
    # -c reads filenames relative to CWD; the archive and the reduced sums file share TMPDIR.
    (
        cd "$TMPDIR"
        if command -v sha256sum >/dev/null 2>&1; then
            sha256sum -c "SHA256SUMS.expected"
        elif command -v shasum >/dev/null 2>&1; then
            shasum -a 256 -c "SHA256SUMS.expected"
        else
            error "Neither sha256sum nor shasum found. Cannot verify download."
        fi
    ) || error "Checksum verification failed for ${ARCHIVE_NAME}"
    success "Checksum verified"

    info "Extracting archive"
    tar -xzf "$ARCHIVE_PATH" -C "$TMPDIR"

    # Install binaries from bin/
    for bin in $BINARIES; do
        if [ -f "${TMPDIR}/bin/${bin}" ]; then
            mv "${TMPDIR}/bin/${bin}" "${INSTALL_DIR}/${bin}"
            chmod +x "${INSTALL_DIR}/${bin}"
        fi
    done
    success "Installed binaries to ${INSTALL_DIR}"

    # Install the DuckDB extensions where the installed binaries look for them: lib/xdu/extensions
    # beside the directory that holds the real executables. That is relative to INSTALL_DIR, not
    # PREFIX, since XDU_INSTALL may point elsewhere, and it is the physical path, so a symlinked
    # install directory lands where the resolved executable looks. Tarballs from releases that
    # predate shipped extensions have no lib/ and install as before.
    if [ -d "${TMPDIR}/lib/xdu/extensions" ]; then
        BIN_REAL=$(cd "$INSTALL_DIR" && pwd -P)
        EXT_DIR="${BIN_REAL%/*}/lib/xdu/extensions"
        mkdir -p "$EXT_DIR"
        for ext in "${TMPDIR}/lib/xdu/extensions/"*; do
            [ -f "$ext" ] || continue
            name=${ext##*/}
            # Copy under a temporary name in the destination, then rename over the old file.
            # Overwriting in place would invalidate a cached macOS code signature, so the next
            # load is killed, and can crash a running process that has the file mapped.
            cp "$ext" "${EXT_DIR}/.${name}.$$"
            mv -f "${EXT_DIR}/.${name}.$$" "${EXT_DIR}/${name}"
        done
        success "Installed DuckDB extensions to ${EXT_DIR}"
    fi

    # Install man pages
    MAN_DIR="${PREFIX}/share/man/man1"
    if [ -d "${TMPDIR}/share/man/man1" ]; then
        mkdir -p "$MAN_DIR"
        cp "${TMPDIR}/share/man/man1/"*.1 "$MAN_DIR/"
        success "Installed man pages to ${MAN_DIR}"
    fi

    # Install bash completions
    BASH_COMP_DIR="${PREFIX}/share/bash-completion/completions"
    if [ -d "${TMPDIR}/share/bash-completion/completions" ]; then
        mkdir -p "$BASH_COMP_DIR"
        cp "${TMPDIR}/share/bash-completion/completions/"* "$BASH_COMP_DIR/"
        success "Installed bash completions to ${BASH_COMP_DIR}"
    fi

    # Install zsh completions
    ZSH_COMP_DIR="${PREFIX}/share/zsh/site-functions"
    if [ -d "${TMPDIR}/share/zsh/site-functions" ]; then
        mkdir -p "$ZSH_COMP_DIR"
        cp "${TMPDIR}/share/zsh/site-functions/"* "$ZSH_COMP_DIR/"
        success "Installed zsh completions to ${ZSH_COMP_DIR}"
    fi

    # Check if install directory is in PATH
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            warn "${INSTALL_DIR} is not in your PATH"
            echo ""
            echo "Add this to your shell configuration file (~/.bashrc, ~/.zshrc, etc.):"
            echo ""
            echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
            echo ""
            ;;
    esac

    # Shell configuration guidance
    echo ""
    echo "To enable bash completions, ensure bash-completion is loaded and add:"
    echo "    export XDG_DATA_DIRS=\"${PREFIX}/share:\$XDG_DATA_DIRS\""
    echo ""
    echo "To enable zsh completions, add to your ~/.zshrc (before compinit):"
    echo "    fpath=(${ZSH_COMP_DIR} \$fpath)"
    echo ""
    echo "Man pages are available via: man xdu"
    echo ""
    echo "Run 'xdu --help' to get started."
}

main "$@"
