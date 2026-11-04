#!/bin/bash
# Run with Bash 3.2+ and the same system tools as 打包原始碼.command.
# SOURCE_REVIEW_TEST_SCRATCH selects the parent of this test's owned sandbox.
set -euo pipefail
export LC_ALL=en_US.UTF-8
export COPYFILE_DISABLE=1
unset ZIP ZIPOPT UNZIP UNZIPOPT ZIPINFO ZIPINFOOPT
umask 077
project=$(cd -P -- "${BASH_SOURCE[0]%/*}/../.." && pwd -P)
scratch=${SOURCE_REVIEW_TEST_SCRATCH:-${TMPDIR:-/tmp}}
scratch=$(cd -P -- "$scratch" && pwd -P)
sandbox=$(mktemp -d "$scratch/mac-source-review-test.XXXXXXXX")
root="$sandbox/中文 空格 O'Brien ! & (專案)"
archive="$sandbox/BabelOxide-source-review.zip"
passed=0
skipped=0
status=0
failure_case='unexpected shell or assertion failure'
invocations=0
opaque=''
encoded=''
last_saved_log=''
log_parent=${SOURCE_REVIEW_TEST_LOG_DIR:-"$project/.test-output/ci/source-packaging-macos"}
mkdir -p -- "$log_parent"
log_parent=$(cd -P -- "$log_parent" && pwd -P)
logs=$(mktemp -d "$log_parent/run.XXXXXXXX")

cleanup() {
    result=$?
    trap - EXIT
    if [ "$result" -ne 0 ]; then
        printf '\nPackaging test failed; complete redacted logs: %s\n' "$logs" >&2
        if [ -n "$last_saved_log" ] && [ -f "$last_saved_log" ]; then
            printf '%s\n' '--- Last 100 lines of the failing packaging invocation ---' >&2
            tail -n 100 "$last_saved_log" >&2 || :
            printf '%s\n' '--- End of packaging diagnostics ---' >&2
        fi
        printf 'FAIL %s; test exit %s; last packager exit %s. Logs: %s\n' "$failure_case" "$result" "$status" "$logs" >&2
    fi
    if [ "$result" -ne 0 ] && [ "${SOURCE_REVIEW_TEST_KEEP_FAILURE:-0}" = 1 ]; then
        printf 'Synthetic failure sandbox retained for inspection: %s\n' "$sandbox" >&2
        exit "$result"
    fi
    case "$sandbox" in
        "$scratch"/mac-source-review-test.????????)
            if [ -d "$sandbox" ] && [ ! -L "$sandbox" ]; then rm -rf -- "$sandbox"; fi
            ;;
        *) printf '%s\n' 'Refusing cleanup outside the owned test sandbox.' >&2; exit 1 ;;
    esac
    exit "$result"
}
trap cleanup EXIT

die() { failure_case=$1; printf 'FAIL %s\n' "$1" >&2; exit 1; }
pass() { passed=$((passed + 1)); printf 'PASS %s\n' "$1"; }
skip() { skipped=$((skipped + 1)); printf 'SKIP %s\n' "$1"; }
write_fixture() {
    local path=$1
    if [ "${path%/*}" != "$path" ]; then
        mkdir -p -- "${root}/${path%/*}"
    fi
    printf '%s' "$2" > "$root/$path"
}
hash_file() {
    perl -MDigest::SHA -e 'open my $f,"<:raw",$ARGV[0] or die "Cannot read hash input\n"; print Digest::SHA->new(256)->addfile($f)->hexdigest' "$1"
}
save_log() {
    last_saved_log="$logs/invocation-$invocations.log"
    # Keep the raw synthetic log for leak assertions, but never upload its values.
    perl -e '
        my $path=shift; open my $f,"<:raw",$path or die "Cannot read packaging log\n";
        local $/; my $text=<$f>;
        for my $value (@ARGV) { $text =~ s/\Q$value\E/[REDACTED]/g if length $value; }
        binmode STDOUT; print $text;
    ' "$sandbox/run.log" "$opaque" "$encoded" > "$last_saved_log"
    printf 'Invocation %s: exit %s; log %s\n' "$invocations" "$status" "$last_saved_log" >> "$logs/summary.log"
}
invoke() {
    invocations=$((invocations + 1))
    set +e
    (cd -- "$sandbox/unrelated" && "$BASH" "$root/打包原始碼.command" --no-pause) > "$sandbox/run.log" 2>&1
    status=$?
    set -e
    save_log
}
assert_code() { [ "$status" -eq "$1" ] || die "$2 (exit $status, expected $1)"; }
assert_no_temporary() {
    local candidate
    for candidate in "$sandbox"/.BabelOxide-source-review.*; do
        [ ! -e "$candidate" ] || die 'temporary archive/snapshot left behind'
    done
}
assert_no_secret_log() {
    [ -n "$1" ] || return 0
    perl -e 'open my $f,"<:raw",$ARGV[0] or die; local $/; exit(index(<$f>,$ARGV[1]) < 0 ? 0 : 1)' "$sandbox/run.log" "$1" || die 'synthetic credential appeared in output'
}
list_entries() {
    # Independently enumerate extracted filesystem names, not unzip's filtered
    # display list or the production script's ZIP metadata reader.
    local extracted
    extracted=$(mktemp -d "$sandbox/readback.XXXXXXXX")
    unzip -qq "$1" -d "$extracted"
    perl -MFile::Find -e '
        my $root=shift; my @names;
        find({no_chdir=>1,wanted=>sub {
            die "Unexpected extracted link\n" if -l $_;
            return if -d $_;
            die "Unexpected extracted file type\n" unless -f $_;
            push @names,substr($_,length($root)+1);
        }},$root);
        print "$_\n" for sort @names;
    ' "$extracted"
}
assert_retained() {
    [ "$(hash_file "$archive")" = "$old_hash" ] || die 'failure changed the old ZIP'
    assert_no_temporary
}
reject() {
    local name=$1 path=$2 text=$3 code=$4
    old_hash=$(hash_file "$archive")
    write_fixture "$path" "$text"
    invoke
    assert_code "$code" "$name"
    assert_retained
    assert_no_secret_log "$opaque"
    assert_no_secret_log "$encoded"
    rm -f -- "$root/$path"
    pass "$name; old ZIP and redaction preserved"
}
tree_snapshot() {
    perl -MFile::Find -MDigest::SHA -e '
        my $root=shift; my @records;
        find({no_chdir=>1,wanted=>sub {
            return unless -f $_;
            my @s=stat $_; open my $f,"<:raw",$_ or die;
            push @records, join("\t",$_,$s[7],$s[9],Digest::SHA->new(256)->addfile($f)->hexdigest);
        }},$root);
        print "$_\n" for sort @records;
    ' "$root"
}

{
    printf 'Bash: %s\nLocale: %s\n' "$BASH_VERSION" "$LC_ALL"
    uname -srm
    command -v perl zip unzip
    perl -e 'print "Perl: $^V\n"'
    zip -v
    unzip -v
} > "$logs/tools.log" 2>&1
printf 'Complete packaging test logs: %s\n' "$logs"

mkdir -p -- "$root" "$sandbox/unrelated"
cp -- "$project/打包原始碼.command" "$project/打包原始碼.cmd" "$project/.gitattributes" "$root/"
write_fixture Cargo.toml '[workspace]'
write_fixture Cargo.lock '# synthetic lock'
write_fixture pyproject.toml '[project]'
write_fixture uv.lock 'version = 1'
write_fixture README.md 'synthetic source review'
write_fixture '啟動開發版.cmd' 'exit /b 0'
write_fixture '啟動開發版.command' '#!/bin/bash'
write_fixture .gitignore '/target/'
write_fixture LICENSE '2. Basic Permissions.'
write_fixture THIRD_PARTY_NOTICES.md 'synthetic source and modification notice'
write_fixture .cargo/config.toml '[build]'
write_fixture crates/config/src/lib.rs 'pub fn config_source() {}'
write_fixture crates/core/src/lib.rs 'pub fn core_source() {}'
write_fixture python-worker/worker.py 'raise RuntimeError("worker must never be executed")'
write_fixture script/never_run.py 'raise RuntimeError("build scripts must never be executed")'
write_fixture tests/fixtures/small.txt 'small synthetic fixture'
write_fixture 'tests/fixtures/中文 [檔案].txt' 'literal names stay literal'
write_fixture 'tests/fixtures/功 能 (文件).txt' 'Unicode path fields bind legacy multibyte names'
write_fixture 'tests/fixtures/-leading $value ; & (v1) # + =,.txt' 'punctuation is never shell input'
write_fixture docs/ARCHITECTURE.md '# architecture'
write_fixture resources/config.example.toml '[translation]'
write_fixture resources/engine-assets.lock.json '{}'
write_fixture resources/build-inputs.lock.json '{}'
write_fixture resources/babeloxide.svg '<svg xmlns="http://www.w3.org/2000/svg"/>'
write_fixture resources/licenses/LICENSE-MIT 'synthetic license'
write_fixture .github/workflows/check.yml 'token: ${{ secrets.GITHUB_TOKEN }}'
write_fixture .env.example 'API_KEY=${API_KEY}'
for path in resources/babeloxide.png resources/babeloxide.ico resources/babeloxide.icns; do
    cp -- "$project/$path" "$root/$path"
done
mkdir -p -- "$root/crates/core/tests/fixtures/compression"
for path in "$project"/crates/core/tests/fixtures/compression/*; do
    cp -- "$path" "$root/crates/core/tests/fixtures/compression/"
done
opaque=$(perl -MDigest::SHA -e 'print Digest::SHA::sha256_hex(rand().time().$$)')
excluded=(target/leak.rs .venv/pyvenv.cfg dist/app.exe .git/HEAD .test-output/private.pdf config/private.toml build-inputs/private.json cache/private.db .pytest_cache/state.json python-worker/__pycache__/worker.pyc resources/model.onnx resources/font.ttf tests/private.pdf script/.env docs/credentials.json)
for path in "${excluded[@]}"; do write_fixture "$path" "$opaque"; done

before=$(tree_snapshot)
invoke
assert_code 0 'initial packaging with Chinese and punctuation paths'
assert_no_secret_log "$opaque"
[ "$before" = "$(tree_snapshot)" ] || die 'packager modified a source file or timestamp'
unzip -tqq "$archive"
list_entries "$archive" > "$sandbox/entries"
for path in "${excluded[@]}"; do
    if grep -Fx -- "$path" "$sandbox/entries" > /dev/null; then die "excluded path entered ZIP: $path"; fi
done
for path in Cargo.toml Cargo.lock pyproject.toml uv.lock python-worker/worker.py crates/core/src/lib.rs docs/ARCHITECTURE.md resources/babeloxide.icns '打包原始碼.cmd' '打包原始碼.command' '.gitattributes' 'tests/fixtures/中文 [檔案].txt' 'tests/fixtures/功 能 (文件).txt' 'tests/fixtures/-leading $value ; & (v1) # + =,.txt'; do
    grep -Fx -- "$path" "$sandbox/entries" > /dev/null || die "missing ZIP source: $path"
done
unzip -qq "$archive" -d "$sandbox/extracted"
perl -MDigest::SHA -e '
    my ($root,$out,$list)=@ARGV; open my $names,"<:raw",$list or die;
    while(my $p=<$names>){ $p=~s/\r?\n$//; my @h;
        for my $base ($root,$out){open my $f,"<:raw","$base/$p" or die "Cannot read extracted file\n"; push @h,Digest::SHA->new(256)->addfile($f)->hexdigest;}
        die "Extracted file differs\n" unless $h[0] eq $h[1];
    }
' "$root" "$sandbox/extracted" "$sandbox/entries"
assert_no_temporary
pass 'Chinese, spaces and punctuation; literal wildcard names; independent cwd; exact extraction; exclusions; no project writes'

for project_name in 'English-project' '中文 空格' "O'Brien ! & (project) [literal] # + ; =,"; do
    next_root="$sandbox/$project_name"
    # These are direct children of our resolved, unique sandbox; never move an
    # arbitrary caller path or merge into an existing directory.
    case "$root" in "$sandbox"/*) ;; *) die 'unsafe fixture move source' ;; esac
    case "$next_root" in "$sandbox"/*) ;; *) die 'unsafe fixture move target' ;; esac
    [ -d "$root" ] && [ ! -L "$root" ] && [ ! -e "$next_root" ] || die 'fixture move is not isolated'
    mv -- "$root" "$next_root"
    root=$next_root
    invoke
    assert_code 0 "path matrix: $project_name"
    unzip -tqq "$archive"
    list_entries "$archive" > "$sandbox/entries"
    grep -Fx 'tests/fixtures/中文 [檔案].txt' "$sandbox/entries" >/dev/null || die 'path matrix lost a literal source name'
    assert_no_temporary
    pass "path matrix: $project_name"
done

old_hash=$(hash_file "$archive")
write_fixture README.md 'updated synthetic source'
write_fixture crates/core/src/new.rs 'pub fn new_source() {}'
rm -f -- "$root/tests/fixtures/small.txt"
invoke
assert_code 0 'repeat packaging'
[ "$(hash_file "$archive")" != "$old_hash" ] || die 'ZIP did not refresh'
list_entries "$archive" > "$sandbox/entries"
grep -Fx 'crates/core/src/new.rs' "$sandbox/entries" >/dev/null || die 'new source missing'
if grep -Fx 'tests/fixtures/small.txt' "$sandbox/entries" >/dev/null; then die 'deleted source still archived'; fi
pass 'repeat packaging refreshes additions and deletions'

ZIP='--synthetic-forbidden-option' ZIPOPT='--synthetic-forbidden-option' \
UNZIP='--synthetic-forbidden-option' UNZIPOPT='--synthetic-forbidden-option' \
ZIPINFO='--synthetic-forbidden-option' ZIPINFOOPT='--synthetic-forbidden-option' invoke
assert_code 0 'caller ZIP options cannot alter packaging or diagnostics'
assert_no_temporary
pass 'caller ZIP and UnZip option variables cannot change the verified archive'

assign_name=api'_'key
env_name=API'_'KEY
bearer_name=access'_'token
reject 'opaque assigned credential' docs/reject.md "$assign_name = \"$opaque\"" 3
reject 'inline test host is not a file exemption' crates/config/src/storage.rs "$assign_name = \"$opaque\"" 3
reject 'strong token prefix in prose' docs/reject.md "prose: sk-$opaque" 3
reject 'unquoted YAML credential' docs/reject.yml "$bearer_name: $opaque" 3
reject 'dotenv template credential' .env.example "$env_name=$opaque" 3
write_fixture .env.example 'API_KEY=${API_KEY}'
reject 'CMD set credential' script/reject.cmd "set \"$env_name=$opaque\"" 3
reject 'shell credential' script/reject.sh "export $env_name=$opaque" 3
reject 'escaped source-string credential' docs/reject.rs "let c = \"{\\\"$assign_name\\\":\\\"$opaque\\\"}\";" 3
encoded=$(perl -MMIME::Base64 -e 'print MIME::Base64::encode_base64("qa:".$ARGV[0],"")' "$opaque")
reject 'HTTP authorization header' docs/reject.md "Authorization: Basic $encoded" 3
reject 'private key block' docs/reject.md '-----BEGIN '"PRIVATE KEY-----" 3
reject 'unknown root file' new-tool.py '# requires source-policy review' 2
reject 'unknown text resource' resources/unreviewed.txt 'requires resource-policy review' 2
reject 'unknown extension' tests/unreviewed.newtype 'unknown source format' 2

url_root='https://user'
write_fixture crates/app/src/settings_ui.rs "$url_root:password@example.com/v1"
invoke
assert_code 0 'exact reviewed URL-rejection fixture'
pass 'exact reviewed URL fixture remains allowed'
reject 'reviewed URL fixture is not a file exemption' crates/app/src/settings_ui.rs "$url_root:$opaque@example.com/v1" 3

write_fixture docs/utf16.txt ''
perl -MEncode -e 'open my $f,">:raw",$ARGV[0] or die; print {$f} "\xff\xfe",Encode::encode("UTF-16LE","synthetic Unicode text");' "$root/docs/utf16.txt"
invoke
assert_code 0 'UTF-16 text'
pass 'UTF-16 source text is scanned and retained'
old_hash=$(hash_file "$archive")
perl -e 'open my $f,">:raw",$ARGV[0] or die; print {$f} "\0\xff\xfe\1";' "$root/docs/utf16.txt"
invoke
assert_code 2 'binary disguised as text'
assert_retained
rm -f -- "$root/docs/utf16.txt"
pass 'binary disguised as text is rejected'

for path in resources/babeloxide.ico crates/core/tests/fixtures/compression/html.gzip; do
    old_hash=$(hash_file "$archive")
    printf '%s' 'changed binary' > "$root/$path"
    invoke
    assert_code 2 'changed pinned binary'
    assert_retained
    cp -- "$project/$path" "$root/$path"
done
pass 'modified artwork and compressed fixture require review'

old_hash=$(hash_file "$archive")
perl -e 'open my $f,">:raw",$ARGV[0] or die; print {$f} "A" x (5*1024*1024+1);' "$root/tests/large.txt"
invoke
assert_code 2 'single file size cap'
assert_retained
rm -f -- "$root/tests/large.txt"
pass '5 MiB single-file cap'

mkdir -p -- "$root/tests/size-cap"
perl -e 'for my $n(1..13){open my $f,">:raw","$ARGV[0]/$n.txt" or die; print {$f} "A" x (5*1024*1024);}' "$root/tests/size-cap"
invoke
assert_code 2 'total snapshot size cap'
assert_retained
rm -rf -- "$root/tests/size-cap"
pass '64 MiB total snapshot cap'

printf '%s' 'synthetic outside file' > "$sandbox/outside.txt"
ln -s -- "$sandbox/outside.txt" "$root/docs/linked.txt" 2>/dev/null || :
if [ -L "$root/docs/linked.txt" ]; then
    invoke
    assert_code 2 'allowlisted file symlink'
    assert_retained
    pass 'allowlisted file symlink is rejected'
else
    [ "$(uname -s)" != Darwin ] || die 'native macOS must run the file symlink rejection test'
    skip 'file symlinks unavailable in this host; native macOS check still needed'
fi
rm -f -- "$root/docs/linked.txt"
ln -s -- "$sandbox/extracted" "$root/docs/linked" 2>/dev/null || :
if [ -L "$root/docs/linked" ]; then
    invoke
    assert_code 2 'allowlisted directory symlink'
    assert_retained
    pass 'allowlisted directory symlink is rejected without traversal'
    rm -f -- "$root/docs/linked"
else
    # Git Bash may copy a directory instead of making a link. This is owned data.
    if [ -d "$root/docs/linked" ]; then rm -rf -- "$root/docs/linked"; fi
    [ "$(uname -s)" != Darwin ] || die 'native macOS must run the directory symlink rejection test'
    skip 'directory symlinks unavailable in this host; native macOS check still needed'
fi

real_zip=$(command -v zip)
mkdir -p -- "$sandbox/bin"
cat > "$sandbox/bin/zip" <<'WRAPPER'
#!/bin/bash
set -eu
if [ "$SOURCE_REVIEW_TEST_ZIP_MODE" = create-fail ]; then
    printf '%s\n' 'synthetic ZIP creation stderr' >&2
    exit 73
fi
"$SOURCE_REVIEW_TEST_REAL_ZIP" "$@"
archive=$4
case "$SOURCE_REVIEW_TEST_ZIP_MODE" in
    corrupt) printf '%s' 'not a ZIP' > "$archive" ;;
    alter)
        printf '%s' 'changed content with a valid ZIP CRC and the original entry name' > README.md
        "$SOURCE_REVIEW_TEST_REAL_ZIP" -q -X -nw "$archive" -- ./README.md
        ;;
    unexpected)
        printf '%s' 'unexpected but valid text' > tests/unexpected.txt
        "$SOURCE_REVIEW_TEST_REAL_ZIP" -q -X -nw "$archive" -- ./tests/unexpected.txt
        ;;
    *)
        # Alter only this test's synthetic ZIP metadata, never the source tree.
        perl - "$archive" "$SOURCE_REVIEW_TEST_ZIP_MODE" <<'PERL'
use strict;
use warnings;
my ($path, $mode)=@ARGV;
open my $f,'+<:raw',$path or die "Cannot open synthetic ZIP\n";
local $/; my $bytes=<$f>;
my $end=length($bytes)-22;
my (undef,undef,undef,$count,$count2,$size,$offset)=unpack('VvvvvVV',substr($bytes,$end,20));
if ($mode eq 'zip64') {
    my $end64=pack('VQ<vvVVQ<Q<Q<Q<',0x06064b50,44,45,45,0,0,$count,$count2,$size,$offset);
    my $locator=pack('VVQ<V',0x07064b50,0,$end,1);
    my $eocd=pack('VvvvvVVv',0x06054b50,0,0,0xffff,0xffff,0xffffffff,0xffffffff,0);
    substr($bytes,$end)=$end64.$locator.$eocd;
} elsif ($mode eq 'truncated') {
    substr($bytes,-8)='';
} elsif ($mode eq 'bad-offset') {
    substr($bytes,$end+16,4)=pack('V',length($bytes)+1);
} else {
    my $changed=0;
    while ($offset<$end) {
        die "Synthetic central header is invalid\n" unless substr($bytes,$offset,4) eq "PK\1\2";
        my ($n,$e,$c)=unpack('vvv',substr($bytes,$offset+28,6));
        my $name=substr($bytes,$offset+46,$n);
        if (($mode eq 'duplicate' && $name eq 'Cargo.lock') || ($name eq 'README.md' && $mode ne 'duplicate')) {
            if ($mode eq 'duplicate' || $mode eq 'traversal') {
                substr($bytes,$offset+46,$n)= $mode eq 'duplicate' ? 'Cargo.toml' : '../bad.rs';
            } elsif ($mode eq 'linked') {
                substr($bytes,$offset+38,4)=pack('V',0120777<<16);
            } elsif ($mode eq 'local-mismatch') {
                my $local=unpack('V',substr($bytes,$offset+42,4));
                substr($bytes,$local+30,$n)='../bad.rs';
            } elsif ($mode eq 'unicode-override') {
                my $field=pack('vvCV',0x7075,5+$n,1,0).$name;
                substr($bytes,$offset+46+$n+$e,0)=$field;
                substr($bytes,$offset+30,2)=pack('v',$e+length($field));
                substr($bytes,$end+length($field)+12,4)=pack('V',$size+length($field));
            } else { die "Unknown synthetic mutation\n"; }
            $changed=1; last;
        }
        $offset+=46+$n+$e+$c;
    }
    die "Synthetic mutation was not applied\n" unless $changed;
}
seek($f,0,0) or die "Cannot seek synthetic ZIP\n";
print {$f} $bytes or die "Cannot write synthetic ZIP\n";
truncate($f,length($bytes)) or die "Cannot truncate synthetic ZIP\n";
close $f or die "Cannot close synthetic ZIP\n";
PERL
        ;;
esac
WRAPPER
chmod +x "$sandbox/bin/zip"
export SOURCE_REVIEW_TEST_REAL_ZIP="$real_zip"
for mode in corrupt alter duplicate traversal linked bad-offset truncated unexpected unicode-override local-mismatch; do
    old_hash=$(hash_file "$archive")
    export SOURCE_REVIEW_TEST_ZIP_MODE="$mode"
    PATH="$sandbox/bin:$PATH" invoke
    assert_code 4 "invalid ZIP metadata or bytes: $mode"
    assert_retained
    pass "invalid ZIP rejected and old ZIP retained: $mode"
done
export SOURCE_REVIEW_TEST_ZIP_MODE=zip64
PATH="$sandbox/bin:$PATH" invoke
assert_code 0 'valid ZIP64 central directory'
unzip -tqq "$archive"
assert_no_temporary
pass 'valid ZIP64 directory retains UTF-8 entry verification'

old_hash=$(hash_file "$archive")
export SOURCE_REVIEW_TEST_ZIP_MODE=create-fail
PATH="$sandbox/bin:$PATH" invoke
assert_code 73 'ZIP tool failure keeps its real exit code'
assert_retained
grep -F 'synthetic ZIP creation stderr' "$sandbox/run.log" >/dev/null || die 'ZIP stderr was lost'
grep -F '階段=建立暫存 ZIP' "$sandbox/run.log" >/dev/null || die 'ZIP creation stage was lost'
pass 'ZIP creation failure retains stderr, stage, exit code and old ZIP'
unset SOURCE_REVIEW_TEST_ZIP_MODE SOURCE_REVIEW_TEST_REAL_ZIP

real_unzip=$(command -v unzip)
cat > "$sandbox/bin/unzip" <<'WRAPPER'
#!/bin/bash
set -eu
case "${1:-}" in
    -Z) printf '%s\n' 'display-only ZIP listing must not be parsed' >&2; exit 87 ;;
    -qq)
        if [ "$SOURCE_REVIEW_TEST_UNZIP_MODE" = extract-fail ]; then
            printf '%s\n' 'synthetic extraction stderr' >&2
            exit 74
        fi
        ;;
esac
exec "$SOURCE_REVIEW_TEST_REAL_UNZIP" "$@"
WRAPPER
chmod +x "$sandbox/bin/unzip"
rm -f -- "$sandbox/bin/zip"
export SOURCE_REVIEW_TEST_REAL_UNZIP="$real_unzip" SOURCE_REVIEW_TEST_UNZIP_MODE=normal
PATH="$sandbox/bin:$PATH" invoke
assert_code 0 'packaging is independent of filtered unzip display names'
assert_no_temporary
pass 'packaging never uses a locale-filtered unzip display list'
old_hash=$(hash_file "$archive")
export SOURCE_REVIEW_TEST_UNZIP_MODE=extract-fail
PATH="$sandbox/bin:$PATH" invoke
assert_code 74 'extraction failure keeps its real exit code'
assert_retained
grep -F 'synthetic extraction stderr' "$sandbox/run.log" >/dev/null || die 'extraction stderr was lost'
grep -F '階段=ZIP 解壓縮' "$sandbox/run.log" >/dev/null || die 'extraction stage was lost'
pass 'extraction failure retains stderr, stage, exit code and old ZIP'
unset SOURCE_REVIEW_TEST_REAL_UNZIP SOURCE_REVIEW_TEST_UNZIP_MODE
rm -f -- "$sandbox/bin/unzip"

printf '%s\n%s\n' "$opaque" "$encoded" > "$sandbox/run.log"
invocations=$((invocations + 1))
save_log
if grep -F "$opaque" "$last_saved_log" >/dev/null || grep -F "$encoded" "$last_saved_log" >/dev/null; then
    die 'persisted test diagnostics exposed a synthetic credential'
fi
grep -F '[REDACTED]' "$last_saved_log" >/dev/null || die 'test diagnostic redaction did not run'
pass 'persisted diagnostics redact synthetic credential values'

real_perl=$(command -v perl)
cat > "$sandbox/bin/perl" <<'WRAPPER'
#!/bin/bash
set -eu
if [ "${2:-}" = publish ]; then exit 86; fi
exec "$SOURCE_REVIEW_TEST_REAL_PERL" "$@"
WRAPPER
chmod +x "$sandbox/bin/perl"
export SOURCE_REVIEW_TEST_REAL_PERL="$real_perl"
PATH="$sandbox/bin:$PATH" invoke
assert_code 86 'simulated publication failure'
assert_retained
grep -F '階段=原子發布 ZIP' "$sandbox/run.log" >/dev/null || die 'publication failure stage was lost'
unset SOURCE_REVIEW_TEST_REAL_PERL
pass 'simulated publication failure preserves old ZIP and removes temporary files'

printf '%s checks passed; %s skipped. Bash %s on %s; Finder interaction is not tested.\n' "$passed" "$skipped" "$BASH_VERSION" "$(uname -s)"
