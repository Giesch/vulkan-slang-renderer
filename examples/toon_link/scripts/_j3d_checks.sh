# Shared tier-1 structural checks for the J3D extraction scripts. Sourced, not
# run: `J3D_PROG=extract_ship . "$(dirname "$0")/_j3d_checks.sh"`.
#
# Tier 1 is the half of the two-tier verification that works on the very first
# run, before any golden hash exists -- so the tier-2 bootstrap can never
# enshrine garbage. Only checks that are genuinely file-format-generic live
# here; anything that knows about one particular asset stays in its own script.
#
# `die` prefixes its message with $J3D_PROG so a sourced helper still reports
# the calling script's name.

: "${J3D_PROG:=j3d}"

die() { echo "$J3D_PROG: error: $*" >&2; exit 1; }

be_u16() { # file offset -> decimal value of big-endian u16
    local -a b
    b=($(od -An -tu1 -j"$2" -N2 "$1"))
    echo $(( (b[0] << 8) | b[1] ))
}

be_u32() { # file offset -> decimal value of big-endian u32
    local -a b
    b=($(od -An -tu1 -j"$2" -N4 "$1"))
    echo $(( (b[0] << 24) | (b[1] << 16) | (b[2] << 8) | b[3] ))
}

check_size() { # file min max (bytes)
    local size
    size=$(stat -c%s "$1")
    { [ "$size" -ge "$2" ] && [ "$size" -le "$3" ]; } \
        || die "$1: size $size outside expected range [$2, $3] -- wrong archive member?"
}

check_bdl_header() {
    [ "$(head -c8 "$1")" = "J3D2bdl4" ] || die "$1: bad magic, expected J3D2bdl4"
    local claimed actual
    claimed=$(be_u32 "$1" 8)
    actual=$(stat -c%s "$1")
    [ "$claimed" -eq "$actual" ] \
        || die "$1: J3D header claims $claimed bytes but file is $actual (truncated?)"
}

check_bti_header() {
    [ "$(stat -c%s "$1")" -gt 32 ] || die "$1: smaller than a BTI header"
    local fmt w h
    fmt=$(od -An -tu1 -j0 -N1 "$1" | tr -d ' ')
    case "$fmt" in
        0|1|2|3|4|5|6|8|9|10|14) ;;
        *) die "$1: byte 0 = $fmt is not a valid GX texture format id" ;;
    esac
    w=$(be_u16 "$1" 2)
    h=$(be_u16 "$1" 4)
    { [ "$w" -ge 1 ] && [ "$w" -le 1024 ] && [ "$h" -ge 1 ] && [ "$h" -le 1024 ]; } \
        || die "$1: implausible dimensions ${w}x${h}"
}
