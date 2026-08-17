#!/usr/bin/env bash
#
# Regenerate the committed link inputs in platform/targets/x64glibc/.
#
# Measures what the host archive leaves undefined, assigns each symbol to the
# system library that defines it, and emits one stub shared object per library
# with that library's SONAME and no symbol versions. The executable roc links
# then records a plain libc.so.6-style DT_NEEDED and no GLIBC_2.xx requirement.
#
# See llm_notes/roc_platform_release.md for why stubs, and
# llm_notes/roc_platform_release/02_stub_generator.md for the algorithm.

set -euo pipefail
export LC_ALL=C

cd "$(dirname "$0")/.."

# The glibc floor. Raising it narrows the audience for every executable built
# against the published platform: the stub sizes and the symbol set both come
# from this machine's libraries. llm_notes/tech_debt.md §18 tracks the cost.
REQUIRED_GLIBC=2.39
REQUIRED_GCC_MAJOR=13

RUST_TRIPLE=x86_64-unknown-linux-gnu
TARGET_DIR=platform/targets/x64glibc
STUB_DIR=stubs

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() {
    echo "error: $*" >&2
    exit 1
}

# --- step 1: assert the floor ------------------------------------------------

[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] ||
    fail "this platform builds x64glibc only (got $(uname -s) $(uname -m))"

for tool in nm readelf ar gcc cargo; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is not on PATH"
done

# `sed -n 1p` rather than `head -1`: pipefail turns head's early exit into a
# SIGPIPE failure for the producer, intermittently, depending on the pipe buffer.
ldd_line=$(ldd --version | sed -n 1p)
gcc_line=$(gcc --version | sed -n 1p)
ld_line=$(ld --version | sed -n 1p)

glibc_version=$(echo "$ldd_line" | grep -oE '[0-9]+\.[0-9]+$')
[ "$glibc_version" = "$REQUIRED_GLIBC" ] || fail \
    "glibc $glibc_version does not match the floor $REQUIRED_GLIBC.
  A newer glibc silently raises the floor for every player: the stubs would
  carry symbols an older libc.so.6 cannot resolve. An older one lowers it
  without the review llm_notes/tech_debt.md §18 asks for.
  To move the floor deliberately, change REQUIRED_GLIBC in $0."

gcc_major=$(gcc -dumpversion | cut -d. -f1)
[ "$gcc_major" = "$REQUIRED_GCC_MAJOR" ] || fail \
    "gcc $gcc_major does not match the floor gcc $REQUIRED_GCC_MAJOR.
  libstdc++.a is committed from this compiler, so its symbol set moves with it."

# `gcc -print-file-name=X` echoes X unchanged when it cannot find the file, so
# an absolute path is the success test.
find_file() {
    local name=$1 path
    path=$(gcc -print-file-name="$name")
    [ "${path#/}" != "$path" ] && [ -e "$path" ] || fail "missing system file: $name"
    echo "$path"
}

LIBC_SO=$(find_file libc.so.6)
LIBM_SO=$(find_file libm.so.6)
LIBGCC_SO=$(find_file libgcc_s.so.1)
LIBVULKAN_SO=$(find_file libvulkan.so.1)
LIBC_NONSHARED=$(find_file libc_nonshared.a)
LIBSTDCXX_A=$(find_file libstdc++.a)
LD_SO=/lib64/ld-linux-x86-64.so.2
[ -e "$LD_SO" ] || fail "missing dynamic linker: $LD_SO"

# Assignment order. Earlier providers win, so libm claims every symbol that libc
# and libm both export. Both libraries are in DT_NEEDED, so either assignment
# resolves. ld.so is last and its symbols are emitted into the libc stub: ld.so
# is always in the global search scope, so they resolve at run time.
PROVIDERS="libvulkan libgcc_s libm libc ldso"
provider_path() {
    case $1 in
        libvulkan) echo "$LIBVULKAN_SO" ;;
        libgcc_s)  echo "$LIBGCC_SO" ;;
        libm)      echo "$LIBM_SO" ;;
        libc)      echo "$LIBC_SO" ;;
        ldso)      echo "$LD_SO" ;;
    esac
}

# Stub emission groups. ldso has no entry: it folds into libc.
STUBS="libvulkan libgcc_s libm libc"
stub_soname() {
    case $1 in
        libvulkan) echo libvulkan.so.1 ;;
        libgcc_s)  echo libgcc_s.so.1 ;;
        libm)      echo libm.so.6 ;;
        libc)      echo libc.so.6 ;;
    esac
}

echo "floor: glibc $glibc_version, gcc $(gcc -dumpversion)"
echo ""

# --- step 2: build the measurement input -------------------------------------

echo "=== Building the host archive ==="
cargo build --release --lib --target "$RUST_TRIPLE"
LIBHOST_A="target/$RUST_TRIPLE/release/libhost.a"
[ -e "$LIBHOST_A" ] || fail "cargo produced no $LIBHOST_A"
echo ""

# --- step 3: copy toolchain artifacts ----------------------------------------

echo "=== Copying toolchain artifacts into $TARGET_DIR ==="
mkdir -p "$TARGET_DIR"
# --remove-destination: a leftover symlink from the pre-stub build.sh points at
# the source file, so a plain cp would refuse it as the same file, and a plain
# redirect would write through the link into /usr/lib.
for name in Scrt1.o crti.o crtn.o; do
    cp --remove-destination "$(find_file "$name")" "$TARGET_DIR/$name"
    echo "  $name"
done
cp --remove-destination "$LIBSTDCXX_A" "$TARGET_DIR/libstdc++.a"
echo "  libstdc++.a"

# Everything this script owns, plus the archive build.sh writes. roc's targets
# validator checks only that each declared input exists, so a stale file left
# here is invisible to it and would ship in the bundle.
EXPECTED="Scrt1.o crti.o crtn.o libstdc++.a libc_forward.a libc.so libm.so
    libgcc_s.so libvulkan.so libhost.a"
for path in "$TARGET_DIR"/*; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    name=$(basename "$path")
    case " $(echo $EXPECTED) " in
        *" $name "*) continue ;;
    esac
    rm -f "$path"
    echo "  pruned $name"
done
echo ""

# --- step 4: build the forwarding archive ------------------------------------

# glibc keeps atexit and three others out of libc.so.6 on every version, so a
# stub cannot supply them. Compile in $work: nothing may land in stubs/forward/.
echo "=== Building libc_forward.a ==="
forward_objs=()
for src in "$STUB_DIR"/forward/*.c; do
    obj="$work/$(basename "${src%.c}").o"
    gcc -O2 -fno-stack-protector -c -o "$obj" "$src"
    forward_objs+=("$obj")
done
rm -f "$TARGET_DIR/libc_forward.a"
# rcD zeroes timestamps, uid and gid, so two runs produce identical bytes.
ar rcD "$TARGET_DIR/libc_forward.a" "${forward_objs[@]}"
echo "  $(nm --defined-only --extern-only "$TARGET_DIR/libc_forward.a" |
    awk 'NF>=2 && length($(NF-1))==1 {print $NF}' | sort -u | tr '\n' ' ')"
echo ""

# --- step 5: measure ---------------------------------------------------------

# Every link input except `app`. The CRT objects belong here as much as the
# archives do: Scrt1.o references __libc_start_main, and nothing else in the
# link does, so leaving them out produces a stub set that fails to link.
MEASURED=(
    "$TARGET_DIR/Scrt1.o"
    "$TARGET_DIR/crti.o"
    "$TARGET_DIR/crtn.o"
    "$LIBHOST_A"
    "$TARGET_DIR/libstdc++.a"
    "$TARGET_DIR/libc_forward.a"
)

: > "$work/u.txt"
: > "$work/d.txt"
for archive in "${MEASURED[@]}"; do
    # Strong U only. A symbol that is weak-undefined in one object and strong
    # in another still needs a provider, so a weak set cannot be subtracted.
    nm --undefined-only "$archive" 2> /dev/null |
        awk 'NF>=2 && $(NF-1)=="U" {print $NF}' >> "$work/u.txt"
    # --extern-only is load-bearing. SDL_gpu_vulkan.c.o defines a local bss
    # symbol named vkGetInstanceProcAddr; counting locals would cancel ash's
    # genuine reference and empty the libvulkan stub.
    nm --defined-only --extern-only "$archive" 2> /dev/null |
        awk 'NF>=2 && length($(NF-1))==1 {print $NF}' >> "$work/d.txt"
done
sort -u "$work/u.txt" -o "$work/u.txt"
sort -u "$work/d.txt" -o "$work/d.txt"
comm -23 "$work/u.txt" "$work/d.txt" > "$work/s.txt"

undefined_count=$(wc -l < "$work/u.txt")
defined_count=$(wc -l < "$work/d.txt")

# --- step 6: subtract what the other link inputs define ----------------------

# LLD synthesizes these for an executable link.
cat > "$work/allow.txt" << 'EOF'
_DYNAMIC
_GLOBAL_OFFSET_TABLE_
__bss_start
__dso_handle
__ehdr_start
__fini_array_end
__fini_array_start
__init_array_end
__init_array_start
_edata
_end
EOF
# The `app` input defines whatever the platform header provides. The brace
# counter reads the block whether it is on one line or many. `|| provides=`:
# grep exits 1 on no match, and pipefail would abort before the fail message.
provides=$(awk '
    !inblock && /provides[[:space:]]*{/ { inblock = 1 }
    inblock {
        depth += gsub(/{/, "{")
        depth -= gsub(/}/, "}")
        print
        if (depth <= 0) exit
    }' platform/main.roc | grep -o '"[^"]*"' | tr -d '"' | sort -u) || provides=
[ -n "$provides" ] || fail "could not parse a provides entry from platform/main.roc"
echo "$provides" >> "$work/allow.txt"
sort -u "$work/allow.txt" -o "$work/allow.txt"
comm -23 "$work/s.txt" "$work/allow.txt" > "$work/s1.txt"

# --- step 7: route nonshared symbols -----------------------------------------

# Defined, default-version exports only. A compat-only export (single @) cannot
# satisfy an unversioned reference: glibc binds such a reference to the default
# version. A UND row is an import, not an export — libvulkan.so.1 has 61 of them.
# A LOCAL row is _DYNAMIC in a stub, which is not an export either.
#
# TYPE and size must come from the default-version (@@) row. glibc exports
# memcpy twice: FUNC size 44 at GLIBC_2.2.5 and IFUNC size 273 at the default
# GLIBC_2.14. Reading the wrong row misclassifies it, and for a data symbol it
# would size the copy relocation wrong.
dyn_rows() {
    readelf -sW --dyn-syms "$1" | awk '
        NF>=8 && $1 ~ /^[0-9]+:$/ && $5!="LOCAL" && $7!="UND" {
            n=$8
            if (n ~ /@@/)     { sub(/@@.*/, "", n) }
            else if (n ~ /@/) { next }
            print n, $4, $3, $7
        }' | sort -u
}
dynsyms() { dyn_rows "$1" | awk '{print $1}' | sort -u; }

dynsyms "$LIBC_SO" > "$work/libc_dyn.txt"
nm --defined-only --extern-only "$LIBC_NONSHARED" 2> /dev/null |
    awk 'NF>=2 && length($(NF-1))==1 {print $NF}' | sort -u > "$work/nonshared.txt"

# Route from U, not from S. libc_forward.a is in the measured set and defines
# these four, so they never reach S — and routing from S would make the
# named-forwarder assertion below dead code. A nonshared symbol with no
# forwarder would then fail step 8 as merely unassigned.
comm -12 "$work/u.txt" "$work/nonshared.txt" |
    comm -23 - "$work/libc_dyn.txt" > "$work/routed.txt"

printf 'at_quick_exit\natexit\npthread_atfork\n__stack_chk_fail_local\n' |
    sort -u > "$work/forwarders.txt"
unforwarded=$(comm -23 "$work/routed.txt" "$work/forwarders.txt")
if [ -n "$unforwarded" ]; then
    fail "libc_nonshared.a symbols with no forwarder in $STUB_DIR/forward/:
$(echo "$unforwarded" | sed 's/^/    /')"
fi

# Each routed symbol's forwarding target must itself be a libc.so.6 export.
target_of() {
    case $1 in
        atexit)                 echo __cxa_atexit ;;
        at_quick_exit)          echo __cxa_at_quick_exit ;;
        pthread_atfork)         echo __register_atfork ;;
        __stack_chk_fail_local) echo __stack_chk_fail ;;
    esac
}
# Versioned names, so a GLIBC_PRIVATE-only export can be told apart from a
# public one. Dumped to a file: `grep -q` on a 2900-symbol pipe would SIGPIPE
# the producer, and pipefail would turn that into a failure.
nm -D --defined-only "$LIBC_SO" |
    awk 'NF>=2 {print $NF}' | sort -u > "$work/libc_dyn_versioned.txt"

while read -r sym; do
    [ -n "$sym" ] || continue
    target=$(target_of "$sym")
    grep -qx "$target" "$work/libc_dyn.txt" ||
        fail "$sym forwards to $target, which libc.so.6 does not export"
    if ! grep -q "^$target@@GLIBC_2\." "$work/libc_dyn_versioned.txt"; then
        echo "  warning: $target is GLIBC_PRIVATE only; it carries no stability promise"
    fi
done < "$work/routed.txt"

comm -23 "$work/s1.txt" "$work/routed.txt" > "$work/s2.txt"

# --- step 8: assign providers ------------------------------------------------

cp "$work/s2.txt" "$work/rest.txt"
for p in $PROVIDERS; do
    dynsyms "$(provider_path "$p")" > "$work/$p.dyn"
    comm -12 "$work/rest.txt" "$work/$p.dyn" > "$work/assigned.$p"
    comm -23 "$work/rest.txt" "$work/$p.dyn" > "$work/next.txt"
    mv "$work/next.txt" "$work/rest.txt"
done

if [ -s "$work/rest.txt" ]; then
    fail "no provider defines these symbols — a new dependency entered the host:
$(sed 's/^/    /' "$work/rest.txt")"
fi

# --- step 9: classify --------------------------------------------------------

echo "=== Classifying ==="

# Ndx -> section name. The nm letter cannot do this job: environ is a weak
# object (letter V), which no B/D/R/G mapping covers.
section_names() {
    readelf -SW "$1" | sed -n 's/^ *\[ *\([0-9]\+\)\] \([^ ]*\).*/\1 \2/p'
}

for p in $PROVIDERS; do
    path=$(provider_path "$p")
    dyn_rows "$path" > "$work/$p.rows"
    section_names "$path" > "$work/$p.sections"

    : > "$work/$p.classified"
    while read -r sym; do
        [ -n "$sym" ] || continue
        row=$(awk -v s="$sym" '$1==s {print; exit}' "$work/$p.rows")
        [ -n "$row" ] || fail "$sym has no .dynsym row in $path"
        type=$(echo "$row" | cut -d' ' -f2)
        size=$(echo "$row" | cut -d' ' -f3)
        ndx=$(echo "$row" | cut -d' ' -f4)

        case $type in
            FUNC | IFUNC)
                echo "$sym FUNC" >> "$work/$p.classified"
                ;;
            OBJECT)
                section=$(awk -v n="$ndx" '$1==n {print $2; exit}' "$work/$p.sections")
                case $section in
                    .bss*)              stub_section=.bss ;;
                    .data | .data.rel.ro) stub_section=.data ;;
                    .rodata*)           stub_section=.rodata ;;
                    *) fail "$sym is an OBJECT in section '$section' of $path; only .bss, .data and .rodata can be stubbed" ;;
                esac
                [ "$size" = 8 ] ||
                    echo "  note: $sym is $size bytes, not 8; that size can drift between glibc versions"
                echo "$sym OBJECT $size $stub_section" >> "$work/$p.classified"
                ;;
            *)
                fail "$sym is $type in $path; a $type symbol cannot be stubbed as a function or an object"
                ;;
        esac
    done < "$work/assigned.$p"
done

# --- step 10: emit -----------------------------------------------------------

echo "=== Emitting stub sources ==="
for stub in $STUBS; do
    src="$STUB_DIR/${stub}_stub.s"
    soname=$(stub_soname "$stub")

    # ld.so's symbols fold into the libc stub.
    rows="$work/$stub.classified"
    if [ "$stub" = libc ]; then
        rows="$work/libc_and_ldso.classified"
        sort -u "$work/libc.classified" "$work/ldso.classified" > "$rows"
    fi

    {
        echo "# Stub shared object for $soname. Generated by $STUB_DIR/generate.sh."
        echo "#"
        echo "# Declares the symbols the host archive leaves undefined, so the link"
        echo "# resolves and the executable records $soname as a plain DT_NEEDED. The"
        echo "# real library provides every implementation at run time. No .symver"
        echo "# anywhere: a versioned reference would pin the executable to a"
        echo "# GLIBC_2.xx the player may not have."
        echo "#"
        echo "# glibc floor: $REQUIRED_GLIBC. Toolchain: built_with_toolchain.txt."
        echo ""
        echo ".text"
        awk '$2=="FUNC" {
            printf ".balign 8\n.globl %s\n.type %s, @function\n%s: ret\n\n", $1, $1, $1
        }' "$rows"

        # .size is the point of the object branch. The linker resolves a data
        # reference into a shared library with R_X86_64_COPY and sizes the copy
        # from the stub, so st_size 0 gives the loader a slot too small for the
        # real object. There is no link error, and the failure appears only on
        # the player's machine.
        for section in .bss .data .rodata; do
            if awk -v s="$section" '$2=="OBJECT" && $4==s {found=1} END {exit !found}' "$rows"; then
                echo ".section $section"
                awk -v s="$section" '$2=="OBJECT" && $4==s {
                    printf ".balign 8\n.globl %s\n.type %s, @object\n.size %s, %s\n%s: .skip %s\n\n", $1, $1, $1, $3, $1, $3
                }' "$rows"
            fi
        done
    } > "$src"
    echo "  $src ($(grep -c '^\.globl' "$src") symbols)"
done
echo ""

# --- step 11: assemble ------------------------------------------------------

echo "=== Assembling stubs into $TARGET_DIR ==="
for stub in $STUBS; do
    soname=$(stub_soname "$stub")
    # rm first. A leftover symlink from the pre-stub build.sh points into
    # /usr/lib, and gcc -o would follow it and overwrite the real library.
    rm -f "$TARGET_DIR/$stub.so"
    # --build-id=none keeps two runs byte-identical.
    gcc -nostdlib -shared -Wl,-soname,"$soname" -Wl,--build-id=none \
        -o "$TARGET_DIR/$stub.so" "$STUB_DIR/${stub}_stub.s"
    echo "  $stub.so -> SONAME $soname"
done
echo ""

# --- step 12: self-verify ---------------------------------------------------

echo "=== Verifying ==="
for stub in $STUBS; do
    so="$TARGET_DIR/$stub.so"
    soname=$(stub_soname "$stub")

    readelf -d "$so" > "$work/dyn.out"
    readelf -SW "$so" > "$work/sec.out"

    grep -q "SONAME.*\[$soname\]" "$work/dyn.out" ||
        fail "$so does not declare SONAME $soname"
    if grep -q NEEDED "$work/dyn.out"; then
        fail "$so declares a DT_NEEDED entry; a stub must depend on nothing"
    fi
    if grep -qE '\.gnu\.version(_d|_r)' "$work/sec.out"; then
        fail "$so carries a symbol version section"
    fi
    echo "  $stub.so: SONAME $soname, no DT_NEEDED, no version sections"
done

vk_exports=$(dynsyms "$TARGET_DIR/libvulkan.so" | tr '\n' ' ' | sed 's/ $//')
[ "$vk_exports" = vkGetInstanceProcAddr ] ||
    fail "libvulkan.so exports '$vk_exports', expected exactly vkGetInstanceProcAddr"
echo "  libvulkan.so exports exactly vkGetInstanceProcAddr"

nm --defined-only --extern-only "$TARGET_DIR/libc_forward.a" 2> /dev/null |
    awk 'NF>=2 && length($(NF-1))==1 {print $NF}' | sort -u > "$work/forward_def.txt"
for sym in atexit at_quick_exit pthread_atfork __stack_chk_fail_local; do
    grep -qx "$sym" "$work/forward_def.txt" ||
        fail "libc_forward.a does not define $sym"
done
echo "  libc_forward.a defines all four forwarders"

[ -z "$(find "$TARGET_DIR" -type l)" ] || fail "$TARGET_DIR still contains a symlink"
echo "  no symlinks in $TARGET_DIR"
echo ""

# --- step 13: record --------------------------------------------------------

cat > built_with_toolchain.txt << EOF
floor: ubuntu 24.04
glibc: $ldd_line
gcc: $gcc_line
binutils: $ld_line
rustc: $(rustc --version)
libstdc++.a: $LIBSTDCXX_A
generated-by: stubs/generate.sh
EOF

stub_total=0
echo "=== Symbol set ==="
printf '  %-28s %s\n' "undefined" "$undefined_count"
printf '  %-28s %s\n' "defined (extern)" "$defined_count"
printf '  %-28s %s\n' "undefined and not defined" "$(wc -l < "$work/s.txt")"
echo ""
for p in $PROVIDERS; do
    count=$(wc -l < "$work/assigned.$p")
    funcs=$(awk '$2=="FUNC"' "$work/$p.classified" | wc -l)
    objs=$(awk '$2=="OBJECT"' "$work/$p.classified" | wc -l)
    printf '  %-28s %4s  (%s func, %s object)\n' "$(basename "$(provider_path "$p")")" "$count" "$funcs" "$objs"
    stub_total=$((stub_total + count))
done
printf '  %-28s %4s\n' "libc_forward.a" "$(wc -l < "$work/routed.txt")"
printf '  %-28s %4s\n' "allowlisted (not stubbed)" "$(comm -12 "$work/s.txt" "$work/allow.txt" | wc -l)"
echo ""
printf '  %-28s %4s\n' "stub set" "$stub_total"
echo ""
echo "Done. Commit $TARGET_DIR, $STUB_DIR/*.s and built_with_toolchain.txt."
