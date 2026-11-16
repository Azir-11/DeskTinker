#!/bin/bash
set -euo pipefail
umask 022

version=1.72.0
tag="V$version"
revision=f0bcdc9743228f3f76606457033e509a2b2e7c1c
go_toolchain=go1.26.10
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
generated="$repo_root/src-tauri/generated"
downloads="$generated/mole-downloads"
payload="$generated/mole"
release="https://github.com/tw93/Mole/releases/download/$tag"

if [[ "$(uname -s)" != Darwin ]]; then
    printf '%s\n' 'Mole bundle preparation requires macOS.' >&2
    exit 1
fi

case "${TAURI_ENV_TARGET_TRIPLE:-$(uname -m)}" in
    aarch64-apple-darwin | arm64) architectures=(arm64) ;;
    x86_64-apple-darwin | x86_64) architectures=(amd64) ;;
    universal-apple-darwin) architectures=(arm64 amd64) ;;
    *) printf 'Unsupported Mole bundle target: %s\n' "$TAURI_ENV_TARGET_TRIPLE" >&2; exit 1 ;;
esac

if ! command -v go >/dev/null; then
    printf 'Install Go %s to include the Mole dependency source.\n' "${go_toolchain#go}" >&2
    exit 1
fi

mkdir -p "$downloads"
stage="$(mktemp -d "$generated/.mole-stage.XXXXXX")"
trap 'rm -rf -- "$stage"' EXIT

fetch_verified() {
    local name="$1" url="$2" hash="$3"
    local path="$downloads/$name"
    if [[ -f "$path" ]] && printf '%s  %s\n' "$hash" "$path" | shasum -a 256 -cs; then
        return
    fi

    curl --fail --location --silent --show-error --retry 3 --connect-timeout 15 --max-time 180 \
        "$url" -o "$stage/$name"
    printf '%s  %s\n' "$hash" "$stage/$name" | shasum -a 256 -c
    mv -f -- "$stage/$name" "$path"
}

source_archive="Mole-$version.tar.gz"
# Official tag archive digest is also pinned by Homebrew's Mole 1.72.0 formula.
fetch_verified "$source_archive" "https://github.com/tw93/Mole/archive/refs/tags/$tag.tar.gz" \
    fc62dabe1e0a0a44aa70f12cc2d82134f980f0b095450f00512406064da46f9c

source="$stage/Mole-$version"
destination="$stage/payload"
tar -xzf "$downloads/$source_archive" -C "$stage"
mkdir -p "$destination/source" "$stage/helpers"
cp -R "$source/bin" "$source/lib" "$destination/"
install -m 755 "$source/mo" "$source/mole" "$destination/"
install -m 644 "$source/LICENSE" "$source/TRADEMARK.md" "$destination/"
install -m 644 "$downloads/$source_archive" "$destination/source/"

for architecture in "${architectures[@]}"; do
    # Digests from the upstream release's SHA256SUMS and GitHub asset metadata.
    case "$architecture" in
        arm64) hash=0f4ba27aac35775ed309fe3cdc73d6f56d30652099d7f2e15bcce9297e42eb7e ;;
        amd64) hash=555ad54b44198dfc53e8bc6bc381df1f06ac91309dcc7f3d07b04090d670f4b3 ;;
    esac
    archive="binaries-darwin-$architecture.tar.gz"
    fetch_verified "$archive" "$release/$archive" "$hash"
    tar -xzf "$downloads/$archive" -C "$stage/helpers"
done

for helper in analyze status; do
    inputs=()
    for architecture in "${architectures[@]}"; do
        inputs+=("$stage/helpers/$helper-darwin-$architecture")
    done
    executable="$destination/bin/$helper-go"
    if [[ "${#inputs[@]}" -eq 1 ]]; then
        install -m 755 "${inputs[0]}" "$executable"
    else
        lipo -create "${inputs[@]}" -output "$executable"
    fi
    # Keep every slice runnable on macOS, including the unsigned upstream Intel binaries.
    codesign --force --timestamp=none --sign - "$executable"
    codesign --verify --all-architectures --strict "$executable"
done

# Include the source and licenses of the Go packages linked into both helpers.
# go.mod and go.sum come from the verified archive. The Go tool is build-only.
(
    cd -- "$source"
    GOFLAGS= GOWORK=off GOTOOLCHAIN="$go_toolchain" go mod vendor
    TZ=UTC find vendor -exec touch -t 200001010000.00 {} +
    find vendor -print0 | LC_ALL=C sort -z | COPYFILE_DISABLE=1 tar --format ustar \
        --uid 0 --gid 0 --uname root --gname root --no-recursion --null -T - -cf - |
        gzip -n > "$destination/source/go-vendor.tar.gz"
)

cat > "$destination/source/README.txt" <<EOF
Mole $version is distributed under the GNU General Public License version 3.
See ../LICENSE for the licence text and ../TRADEMARK.md for project attribution.

Upstream source commit: $revision
Upstream release: https://github.com/tw93/Mole/releases/tag/$tag
The source archive $source_archive contains that release's scripts,
Go source and build files. go-vendor.tar.gz contains the corresponding
Go package sources and their licenses. The packaged helper executables
use the upstream release binaries, with local macOS ad-hoc signatures.

To rebuild both helpers, extract $source_archive, then extract
go-vendor.tar.gz into the unpacked Mole directory. From inside that directory,
run the following with $go_toolchain installed. Use GOARCH=amd64 for Intel.

  CGO_ENABLED=0 GOOS=darwin GOARCH=arm64 go build -mod=vendor -ldflags='-s -w' -o bin/analyze-go ./cmd/analyze
  CGO_ENABLED=0 GOOS=darwin GOARCH=arm64 go build -mod=vendor -ldflags='-s -w' -o bin/status-go ./cmd/status
EOF

if [[ -e "$payload" ]]; then
    mv -- "$payload" "$stage/previous"
fi
mv -- "$destination" "$payload"
printf 'Prepared Mole %s for %s\n' "$version" "${architectures[*]}"
