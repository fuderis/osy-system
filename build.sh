#!/usr/bin/env bash

set -Eeuo pipefail

###############################################################################
# Configuration
###############################################################################

INSTALL_DIR="/usr/local/bin"


###############################################################################
# Output Style
###############################################################################

NC=$'\033[0m'
BOLD=$'\033[1m'

# Colors
BLUE=$'\033[34m'
CYAN=$'\033[36m'
RED=$'\033[31m'
GRAY=$'\033[37m'
DGRAY=$'\033[38;5;238m'

# Semantic Colors
GREEN=$'\033[1;32m'
ERROR_RED=$'\033[1;31m'
YELLOW=$'\033[1;33m'

header() {
    printf "\n${BOLD}%s${NC}\n" "$1"
}

info() {
    printf "\n  ${GRAY}│${NC} ${BOLD}%s${NC}\n" "$1"
}

field() {
    printf "  ${GRAY}%-10s${NC} %s\n" "$1" "$2"
}

item() {
    printf "  ${GRAY}•${NC} %s ${GRAY}→${NC} %s\n" "$1" "$2"
}

subitem() {
    printf "  ${GRAY}└─${NC} %s\n" "$1"
}

ok() {
    printf "  ${GREEN}✓${NC} %s\n" "$1"
}

warn() {
    printf "  ${YELLOW}ℹ${NC} %s\n" "$1"
}

err() {
    printf "  ${ERROR_RED}✗${NC} %s\n" "$1" >&2
}

error() {
    printf "\n${DGRAY}───────────────────────────────────────────────────────${NC}\n" >&2
    printf "${ERROR_RED}${BOLD}%s${NC} %s\n" "$1" "$2" >&2
}

die() {
    err "$1"
    exit 1
}

success() {
    printf "\n${DGRAY}───────────────────────────────────────────────────────${NC}\n"
    printf "${GREEN}${BOLD}%s${NC} %s\n" "$1" "$2"
}


###############################################################################
# Graceful Cleanup / Error Handling
###############################################################################

cleanup() {
    local exit_code=$?
    if [[ $exit_code -ne 0 ]]; then
        error "Failed" "Installation aborted (exit code $exit_code)"
    fi
}
trap cleanup EXIT


###############################################################################
# Requirements
###############################################################################

header "Checking requirements"

command -v cargo >/dev/null 2>&1 || die "cargo is not installed"
command -v jq >/dev/null 2>&1    || die "jq is not installed"
[[ -f Cargo.toml ]]             || die "Cargo.toml not found in current directory"

ok "Environment ready"


###############################################################################
# Metadata Parsing
###############################################################################

header "Inspecting project"

METADATA="$(cargo metadata --format-version=1 --no-deps)"

mapfile -t PACKAGES < <(
    jq -r '
        .packages[]
        | select(any(.targets[]; .kind | any(. == "bin")))
        | .name
    ' <<< "$METADATA"
)

[[ ${#PACKAGES[@]} -gt 0 ]] || die "No binary targets found in project"

declare -A BIN_MAP
declare -A PATH_MAP

while IFS=$'\t' read -r package manifest_path binary; do
    BIN_MAP["$package"]+="$binary "
    
    pkg_dir=$(dirname "$manifest_path")
    pkg_dir=${pkg_dir#"$(pwd)/"}
    [[ "$pkg_dir" == "$(pwd)" || "$pkg_dir" == "." ]] && pkg_dir="./"
    
    PATH_MAP["$package"]="$pkg_dir"
done < <(
    jq -r '
        .packages[] as $pkg
        | $pkg.targets[]
        | select(.kind | any(. == "bin"))
        | [$pkg.name, $pkg.manifest_path, .name]
        | @tsv
    ' <<< "$METADATA"
)

TOTAL_BINARIES=0
for package in "${PACKAGES[@]}"; do
    count=$(wc -w <<< "${BIN_MAP[$package]}")
    TOTAL_BINARIES=$((TOTAL_BINARIES + count))
done

if jq -e '.workspace_members | length > 1' <<< "$METADATA" >/dev/null 2>&1; then
    PROJECT_TYPE="Workspace"
else
    PROJECT_TYPE="Package"
fi

field "Type"     "$PROJECT_TYPE"
field "Packages" "${#PACKAGES[@]}"
field "Binaries" "$TOTAL_BINARIES"
echo

for package in "${PACKAGES[@]}"; do
    pkg_path="${PATH_MAP[$package]}"
    
    printf "  ${BOLD}%s${NC} ${GRAY}(%s)${NC}\n" "$package" "$pkg_path"
    
    for binary in ${BIN_MAP[$package]}; do
        subitem "$binary"
    done
done


###############################################################################
# Build
###############################################################################

header "Building binaries"

BUILD_CMD=(cargo build --release)

for package in "${PACKAGES[@]}"; do
    BUILD_CMD+=(--package "$package")
done

BUILD_CMD+=("$@")

"${BUILD_CMD[@]}"

ok "Compilation completed"


###############################################################################
# Install
###############################################################################

header "Installing binaries"

if [[ -w "$INSTALL_DIR" ]]; then
    INSTALL=(install)
else
    warn "No write access to $INSTALL_DIR, using sudo"
    INSTALL=(sudo install)
fi

for package in "${PACKAGES[@]}"; do
    for binary in ${BIN_MAP[$package]}; do
        SOURCE="target/release/$binary"

        [[ -f "$SOURCE" ]] || die "Binary missing: $SOURCE"

        "${INSTALL[@]}" -m 755 "$SOURCE" "$INSTALL_DIR/$binary"

        item "$binary" "$INSTALL_DIR/$binary"
    done
done


###############################################################################
# Completed
###############################################################################

success "Completed" "Installation finished"
