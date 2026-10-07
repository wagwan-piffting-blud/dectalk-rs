#!/bin/sh
# Build dectalk for macOS, Apple Silicon and Intel, as ONE universal binary,
# then sign it with a Developer ID and notarize it.
# Runs on a Mac; build_targets.ps1 -Mac drives it over ssh from Windows.
#
#     sh build_macos.sh              build, check both slices, sign, notarize
#     sh build_macos.sh --no-check   skip the slice check
#     sh build_macos.sh --no-sign    leave the linker's ad-hoc signature
#
# Needs rustup with aarch64-apple-darwin and x86_64-apple-darwin, and the
# Xcode command line tools for the linker, lipo, codesign and notarytool. The
# x86_64 slice is checked under Rosetta, so that has to be installed on an
# Apple Silicon Mac.
#
# Signing settings come from the environment, or from macos_sign.env beside
# this script (gitignored), which is sourced if present:
#
#     DECTALK_SIGN_ID          Developer ID Application identity, by SHA-1
#     DECTALK_NOTARY_KEY_ID    App Store Connect API key ID
#     DECTALK_NOTARY_ISSUER    App Store Connect issuer ID
#     DECTALK_NOTARY_KEY       path to AuthKey_<key id>.p8
#                              (default ~/Desktop/AuthKey_<key id>.p8)
#
# Over ssh the login keychain is locked and codesign fails with
# errSecInternalComponent, so the script unlocks it and retries -- which asks
# for the Mac password and needs a terminal (ssh -t). Notarization uses the
# API key and needs no keychain at all.
#
# The target directory stays on the Mac's own disk: the source may be an SMB
# share of the Windows checkout, whose target/ belongs to Windows.

set -eu

ROOT=$(cd "$(dirname "$0")" && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/dectalk-target}"

if [ -f "$ROOT/macos_sign.env" ]; then
    . "$ROOT/macos_sign.env"
fi
SIGN_ID="${DECTALK_SIGN_ID:-}"
NOTARY_KEY_ID="${DECTALK_NOTARY_KEY_ID:-}"
NOTARY_ISSUER="${DECTALK_NOTARY_ISSUER:-}"
NOTARY_KEY="${DECTALK_NOTARY_KEY:-$HOME/Desktop/AuthKey_$NOTARY_KEY_ID.p8}"

CHECK=1
SIGN=1
for a in "$@"; do
    case "$a" in
        --no-check) CHECK=0 ;;
        --no-sign) SIGN=0 ;;
        *) echo "unknown option $a" >&2; exit 2 ;;
    esac
done

if [ $SIGN = 1 ] && [ -z "$SIGN_ID" ]; then
    echo "DECTALK_SIGN_ID is not set (see macos_sign.env); use --no-sign for an unsigned build" >&2
    exit 1
fi

ARM=aarch64-apple-darwin
X86=x86_64-apple-darwin
OUT="$ROOT/dist/dectalk-macos-universal"

for t in $ARM $X86; do
    rustup target list --installed | grep -qx "$t" || rustup target add "$t"
done

# cargo reads .cargo/config.toml from the working directory, not the manifest's.
cd "$ROOT/dectalk-rs"
cargo build --release --bin dectalk --target $ARM --target $X86

mkdir -p "$ROOT/dist"
lipo -create -output "$OUT" \
    "$CARGO_TARGET_DIR/$ARM/release/dectalk" \
    "$CARGO_TARGET_DIR/$X86/release/dectalk"
ARCHS=$(lipo -archs "$OUT")
case "$ARCHS" in
    *arm64*x86_64*|*x86_64*arm64*) echo "slices: $ARCHS" ;;
    *) echo "expected arm64 and x86_64, got: $ARCHS" >&2; exit 1 ;;
esac

# Signed before the check, so the phrases run through the file that ships.
sign() {
    codesign --force --options runtime --timestamp \
        --identifier com.wags.dectalk --sign "$SIGN_ID" "$OUT"
}
if [ $SIGN = 1 ]; then
    if ! sign; then
        echo "codesign failed; unlocking the login keychain and retrying"
        security unlock-keychain login.keychain
        sign
    fi
    codesign --verify --strict --verbose=2 "$OUT"
fi
ls -l "$OUT"

if [ $CHECK = 1 ]; then
    # The same phrases build_targets.ps1 -Check uses; both slices must agree to the byte.
    WORK=$(mktemp -d)
    printf '%s' "The National Weather Service in Norman has issued a severe thunderstorm warning for Cleveland County until 5:45 PM CDT." > "$WORK/p0.txt"
    printf '%s' "[:np] Hello, I am Paul. [:nh] I am Harry. [:nf] I am Frank. [:nb] I am Betty. [:nu] I am Ursula." > "$WORK/p1.txt"
    printf '%s' "[:rate 350] This is fast. [:rate 120] This is slow. [:rate 180] [:tone 440,500] Back to normal." > "$WORK/p2.txt"
    printf '%s' "On 10/07/2026 the total was \$1,234.56; call 1-800-555-0199 or write to wx@example.gov." > "$WORK/p3.txt"
    if [ "$(uname -m)" = "arm64" ]; then
        SLICES="arm64 x86_64"
    else
        SLICES="x86_64"
    fi
    FAIL=0
    for i in 0 1 2 3; do
        for fmt in 11k 8k; do
            flag=""
            [ $fmt = 8k ] && flag="--8k"
            for s in $SLICES; do
                arch -$s "$OUT" $flag -o "$WORK/$s-p$i-$fmt.wav" -f "$WORK/p$i.txt" 2>/dev/null
            done
            if [ "$SLICES" != "x86_64" ] && ! cmp -s "$WORK/arm64-p$i-$fmt.wav" "$WORK/x86_64-p$i-$fmt.wav"; then
                echo "p$i/$fmt: arm64 and x86_64 slices differ" >&2
                FAIL=1
            fi
        done
    done
    rm -rf "$WORK"
    [ $FAIL = 0 ] || exit 1
    echo "slices agree on all phrases"
fi

[ $SIGN = 1 ] || exit 0

# A bare Mach-O cannot be stapled, so the ticket lives with Apple and
# Gatekeeper fetches it on first run. notarytool only takes zip, pkg or dmg.
if [ ! -f "$NOTARY_KEY" ]; then
    echo "no notary key at $NOTARY_KEY - signed but NOT notarized" >&2
    exit 1
fi
ZIP=$(mktemp -d)/dectalk-macos-universal.zip
ditto -c -k --keepParent "$OUT" "$ZIP"
RESULT=$(xcrun notarytool submit "$ZIP" --wait --output-format plist \
    --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")
STATUS=$(printf '%s' "$RESULT" | plutil -extract status raw -)
SUBMISSION=$(printf '%s' "$RESULT" | plutil -extract id raw -)
echo "notarization $SUBMISSION: $STATUS"
if [ "$STATUS" != "Accepted" ]; then
    xcrun notarytool log "$SUBMISSION" \
        --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER"
    exit 1
fi
rm -f "$ZIP"
