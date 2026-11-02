#!/bin/bash
# macOS Bash 3.2; only system tools, including Perl (also used by shasum).
# Keep LF line endings. No project code, worker, or build script is executed.
set -Eeuo pipefail
# Info-ZIP must use a UTF-8 locale to preserve Chinese entry names.
export LC_ALL=en_US.UTF-8
export COPYFILE_DISABLE=1
unset ZIP ZIPOPT UNZIP UNZIPOPT ZIPINFO ZIPINFOOPT
umask 077

work=''
parent=''
destination=''
published=0
stage='初始化'

report_error() {
    local code=$1 line=$2
    # Never print BASH_COMMAND, shell traces, environment values or source bytes.
    printf '打包命令失敗：階段=%s；腳本行=%s；退出碼=%s。請見上方 stderr。\n' "$stage" "$line" "$code" >&2 || :
    exit "$code"
}

finish() {
    result=$?
    trap - EXIT ERR HUP INT TERM
    set +e  # A failed diagnostic/cleanup write must not replace the original exit.
    if [ -n "$work" ] && [ -n "$parent" ]; then
        case "$work" in
            "$parent"/.BabelOxide-source-review.????????)
                if [ -d "$work" ] && [ ! -L "$work" ]; then
                    rm -rf -- "$work" || printf '%s\n' '警告：無法清理暫存目錄，請檢查專案上一層的 .BabelOxide-source-review.*。' >&2
                fi
                ;;
        esac
    fi
    if [ "$result" -ne 0 ]; then
        if [ "$published" -eq 0 ]; then
            printf '\n打包失敗（階段：%s，退出碼：%s）；未發布新 ZIP，既有 ZIP 保持不變。\n' "$stage" "$result" >&2
        else
            printf '\nZIP 已發布；後續結果顯示或終端機作業失敗（退出碼：%s）。\n' "$result" >&2
        fi
    fi
    if [ -t 0 ] && [ -t 1 ] && [ "${1:-}" != '--no-pause' ]; then
        printf '\n按 Return 關閉此視窗...'
        read -r ignored || :
    fi
    exit "$result"
}
pause_option=${1:-}
trap 'finish "$pause_option"' EXIT
trap 'report_error "$?" "$LINENO"' ERR
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

# Perl, Encode, and Digest::SHA are supplied by macOS; no downloaded runtime.
# The same scanner is used on the source snapshot and on extracted ZIP bytes.
review_files() {
    perl - "$@" <<'PERL'
use strict;
use warnings;
use utf8;
use Fcntl qw(:DEFAULT :mode SEEK_SET);
use Encode qw(decode encode FB_CROAK LEAVE_SRC);
use Digest::SHA qw(sha256_hex);
use Compress::Raw::Zlib qw(crc32);
use File::Path qw(make_path);
binmode STDERR, ':encoding(UTF-8)';

my ($operation, $root, $work) = @ARGV;
my ($count, $total, $excluded, $private) = (0, 0, 0, 0);
my %seen;
my @roots = qw(crates python-worker script tests docs .cargo .github resources);
my %roots = map { $_ => 1 } @roots;
my %root_files = map { $_ => 1 } (
    qw(Cargo.toml Cargo.lock pyproject.toml uv.lock README.md .gitignore LICENSE THIRD_PARTY_NOTICES.md),
    '打包原始碼.cmd', '打包原始碼.command',
    qw(AGENTS.md CHANGELOG.md CONTRIBUTING.md SECURITY.md .gitattributes .gitmodules .editorconfig .env.example),
    qw(rust-toolchain.toml rust-toolchain rustfmt.toml .rustfmt.toml clippy.toml .clippy.toml deny.toml Makefile justfile)
);
# Filenames from readdir and argv are UTF-8 bytes, even when this file uses utf8.
%root_files = map { (utf8::is_utf8($_) ? encode('UTF-8', $_) : $_) => 1 } keys %root_files;
my @required = (
    qw(Cargo.toml Cargo.lock pyproject.toml uv.lock README.md .gitignore LICENSE THIRD_PARTY_NOTICES.md),
    qw(python-worker/worker.py .cargo/config.toml resources/config.example.toml),
    qw(resources/engine-assets.lock.json resources/build-inputs.lock.json),
    qw(resources/babeloxide.svg resources/babeloxide.png resources/babeloxide.ico resources/babeloxide.icns),
    encode('UTF-8', '打包原始碼.cmd'), encode('UTF-8', '打包原始碼.command')
);
my %resource_files = map { $_ => 1 } qw(
    resources/config.example.toml resources/engine-assets.lock.json resources/engine-assets.json
    resources/manifest.json resources/build-inputs.lock.json resources/windows-README.md
    resources/macos-README.md resources/README.md resources/babeloxide.svg
    resources/babeloxide.png resources/babeloxide.ico resources/babeloxide.icns
);
my %artwork = (
    'resources/babeloxide.png' => '750b663b661a0335a24ef4a4b052fe3fde26545b9c02e6fa00ca6b076d7daa67',
    'resources/babeloxide.ico' => '7b73177a261e02bc85ee08442e474d396765952ffc54abe08e52bc582197a8a9',
    'resources/babeloxide.icns' => '0c9a164605c59d3ff67f7887e80a60326eaaa883814943bb3527eb51be399f06'
);
my %fixtures = (
    'html.br' => 'd2329f3b58be1ed44293d05c7ac2ac049a0aa9d7042dd2f65186c4c0151b907a',
    'html.deflate' => 'dec66362e5cb5da04cb99fd1b76889bfe5d3226b268001cfc45d2a2135c16f4b',
    'html.gzip' => 'c85604e19d21faffbfa0f720d36a4c7b4e97687ea67f42ac315474c4ad607e81',
    'html.zstd' => 'f066aae30e3651ecbbd1de2d6364386db8912498e2b66ae55c1e44acf0bb24aa',
    'json.br' => 'cfd3b99e1c63672d6e7c421f81a82c24b451b801de1d5f95935cebd239b13698',
    'json.deflate' => 'fb852a0829a62dcedd8c0f34faf153032384cc4b81c3a4cc74fcb248f278ca6b',
    'json.gzip' => '08104fafbbe135372e832e321e6b1b54df43dceb78da138e3e2be2b491b5e0e6',
    'json.zstd' => 'fc005e6df2b88e67bc7cfec9a1df878bc4b0e2077c65b1cca60695a0ea85a596',
    'oversize.gzip' => '53c917864db91e9bfe5482299c950ebf3ec6c6f17110aa216b614f441fb2edf0'
);
my %dummy = (
    'crates/app/src/settings_ui.rs' => [qw(093866188144b0216fb125431572e8ac2df8980dfeeb6ebeb3d9f3987d1e4c1d)],
    'crates/config/tests/settings.rs' => [qw(ec875e4d285b285b668305e00236ff763d715805dd18cc739b5a11b892ff38e3)],
    'crates/config/tests/store.rs' => [qw(ab1d629e15eb32f67f8fc6823ee39a0f98003b907d8c28dbe90a83c62f7a3bc5 62c8ae2b244d92b6e8093098e683d2e736a7f46808520fbf7dee0f2143dbcca7)],
    'crates/core/tests/cache_storage.rs' => [qw(5be3b05a2339aecdb0c543afefc6a563085f5861e6c109dce236363b3319566e 1413c4efe71a5beb9f564fbceffd94b8fefa6f96e1d89c5c879dd2a909d9b201 11adac4deca4835e20ea77c556c8f659f7bae73f561342a12526b159350c5b9e)],
    'crates/core/tests/provider_compatibility.rs' => [qw(f98103e9217f099208569d295c1b276f1821348636c268c854bb2a086e0037cd f72e714302c899c35b32189fb6f90b6b2636b69348429ab54f40921b78cacbc0)]
);
my $excluded_dirs = qr/^(?:target|dist|build|\.git|\.venv.*|venv|__pycache__|\.pytest_cache|\.ruff_cache|\.mypy_cache|\.tox|node_modules|\.test-output|\.uv-cache|cache|\.cache|\.idea|\.vscode|\.serena|\.codebase-memory|\.baseline-home|\.build-tools|\.agents|\.codex|outputs?|logs)$/i;
my $binary_ext = qr/\.(?:exe|dll|pyd|so|dylib|a|lib|o|obj|rlib|rmeta|pdb|msi|msix|zip|whl|7z|tar|gz|bz2|xz|br|deflate|gzip|zstd|pdf|docx|xlsx|pptx|png|jpg|jpeg|gif|ico|icns|webp|mp4|mp3|onnx|pt|pth|bin|safetensors|h5|tflite|ttf|otf|woff|woff2|db|sqlite|sqlite3|pyc|pyo|class|jar|wasm|dmp)$/i;
my $text_ext = qr/\.(?:rs|py|pyi|ps1|psm1|psd1|cmd|bat|sh|bash|command|toml|lock|json|md|rst|txt|yaml|yml|ini|cfg|xml|manifest|csv|tsv|html|css|js|ts|tsx|jsx|sql|proto|c|h|cc|cpp|hpp|cmake|in|plain|feature|svg)$/i;
my $credential_name = qr/(?<![a-z0-9_-])[a-z0-9_-]*(?:api[_-]?key|access[_-]?key|secret|password|passwd|pwd|token|authorization)[a-z0-9_-]*/i;
my $strong = qr/-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----|(?<![a-z0-9_-])sk-(?:proj-|svcacct-)?[a-z0-9_-]{16,}|\b(?:gh[pousr]_[a-z0-9]{20,}|github_pat_[a-z0-9_]{30,}|xox[baprs]-[a-z0-9-]{15,}|AIza[a-z0-9_-]{30,}|(?:AKIA|ASIA)[A-Z0-9]{16})\b|\beyJ[a-z0-9_-]{10,}\.[a-z0-9_-]{10,}\.[a-z0-9_-]{10,}\b|(?:https?|postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqps?):\/\/[^\s\/:\@]+:[^\s\/\@]+\@/im;
my $assignment = qr/["']?$credential_name["']?\s*[:=]\s*(?:(?:Some|String::from)\(\s*)?["']([^"'\r\n]+)["']/i;
my $bare = qr/^\s*(?:set\s+["']?|export\s+)?["']?$credential_name["']?\s*[:=]\s*(\$\{\{[^\r\n]*?\}\}|\$\{[^}\r\n]+\}|[^\s"'\#;,{}\[\]]+)/im;

sub stop {
    my ($reason, $path, $code) = @_;
    if (defined $path) {
        $path = decode('UTF-8', $path) unless utf8::is_utf8($path);
        $path =~ s/[\x00-\x1f\x7f]/?/g;
        $reason .= "：$path";
    }
    print STDERR "$reason\n";
    exit($code || 2);
}

sub safe_name {
    my ($path, $code) = @_;
    stop('檔名含控制字元或反斜線，無法安全封裝', $path, $code) if $path =~ /[\x00-\x1f\x7f\\]/;
    eval { decode('UTF-8', $path, FB_CROAK | LEAVE_SRC) };
    stop('檔名不是有效 UTF-8', $path, $code) if $@;
}

sub excluded_file {
    my ($name) = @_;
    if ($name =~ /^(?:\.env(?:\..*)?|credentials(?:\..*)?|secrets?(?:\.(?:json|toml|ya?ml|txt))?|id_rsa|id_ed25519|\.netrc|\.npmrc|\.pypirc|.*\.(?:pem|key|p12|pfx)|.*\.(?:local|private)\..*|config\.v\d+\.toml)$/i && $name !~ /^\.env\.(?:example|sample|template)$/) {
        $private++;
        return 1;
    }
    return $name =~ $binary_ext || $name =~ /\.(?:log|tmp|bak|orig|swp)$|^\.DS_Store$|^Thumbs\.db$/i;
}

sub placeholder {
    my ($value, $path) = @_;
    return 1 if $value =~ /^(?:\$\{[A-Z0-9_]+\}|<[A-Za-z0-9 _-]+>|\*+|\[REDACTED\])$/;
    return 1 if $value =~ /^(?:\$[A-Z_][A-Z0-9_]*|%[A-Z_][A-Z0-9_]*%)$/;
    return 1 if $value =~ /^\$\{\{\s*(?:secrets|env|github|inputs|vars)\.[A-Z0-9_.]+\s*\}\}$/;
    return 1 if $value =~ /^(?:your[-_](?:api[-_]?key|token|password)|replace[-_]me)$/i;
    my $test_file = $path =~ /(?:^|\/)tests?\/|\/[^\/]*tests?\.(?:rs|py|ps1|sh)$/ ||
        $path =~ m{^(?:crates/app/src/app\.rs|crates/core/src/lib\.rs|crates/config/src/storage\.rs|script/build_hybrid_windows\.py)$};
    return 1 if $test_file && $value =~ /^(?:Bearer )?(?:synthetic|fake|dummy|test|example|fixture|not)(?:[-_ ][a-z0-9]+)*$/i;
    my $hash = sha256_hex(encode('UTF-8', $value));
    return scalar grep { $_ eq $hash } @{$dummy{$path} || []};
}

sub scan {
    my ($path, $bytes) = @_;
    my $pin = $artwork{$path};
    my $limit = 256 * 1024;
    if ($path =~ m{^crates/core/tests/fixtures/compression/([^/]+)$} && exists $fixtures{$1}) {
        $pin = $fixtures{$1};
        $limit = 65536;
    }
    if (defined $pin) {
        stop('受控圖示／二進位 Fixture 已變動，須先人工審查允許雜湊', $path) if length($bytes) > $limit || sha256_hex($bytes) ne $pin;
        return;
    }
    my $text;
    eval {
        if (substr($bytes, 0, 2) eq "\xff\xfe") { $text = decode('UTF-16LE', substr($bytes, 2), FB_CROAK); }
        elsif (substr($bytes, 0, 2) eq "\xfe\xff") { $text = decode('UTF-16BE', substr($bytes, 2), FB_CROAK); }
        else { $text = decode('UTF-8', $bytes, FB_CROAK | LEAVE_SRC); }
    };
    stop('無法確認為 UTF-8／UTF-16 文字，停止以免收錄二進位內容', $path) if $@;
    stop('文字檔含二進位控制字元', $path) if $text =~ /[\x00-\x08\x0b\x0e-\x1f\x7f]/;
    $text =~ s/\\"/"/g;
    $text =~ s/\\'/'/g;
    $text =~ s/\\n/\n/g;
    $text =~ s/\\r/\r/g;
    my $suspect = 0;
    while ($text =~ /$strong/g) { $suspect = 1 unless placeholder($&, $path); }
    while ($text =~ /$assignment/g) {
        my $value = $1;
        $value =~ s/^\s+|\s+$//g;
        next if $value =~ /^(?:true|false|null|None)$/;
        $suspect = 1 unless placeholder($value, $path);
    }
    if ($path =~ /\.(?:yaml|yml|ini|cfg|toml|cmd|bat|sh|bash|command)$/i || $path =~ /(?:^|\/)\.env\.(?:example|sample|template)$/) {
        while ($text =~ /$bare/g) {
            my $value = $1;
            $value =~ s/^\s+|\s+$//g;
            next if $value =~ /^(?:true|false|null|None)$/;
            $suspect = 1 unless placeholder($value, $path);
        }
    }
    while ($text =~ /(?:["']((?:Bearer|Basic) [a-z0-9+\/_\.=-]{8,})["']|\bAuthorization\s*:\s*((?:Bearer|Basic) [a-z0-9+\/_\.=-]{8,}))/ig) {
        $suspect = 1 unless placeholder(defined($1) ? $1 : $2, $path);
    }
    stop('發現疑似憑證，已停止；請人工檢查此檔（不顯示內容）', $path, 3) if $suspect;
}

sub read_regular {
    my ($base, $path) = @_;
    # Recheck every ancestor so ordinary directory links cannot escape the root.
    my @parts = split '/', $path;
    pop @parts;
    my $ancestor = $base;
    for my $part ('', @parts) {
        $ancestor .= "/$part" if length $part;
        my @s = lstat $ancestor;
        stop('允許範圍內含連結或非實體目錄，拒絕跟隨', $path) unless @s && S_ISDIR($s[2]) && !S_ISLNK($s[2]);
    }
    my $full = "$base/$path";
    my @before = lstat $full;
    stop('允許範圍內含連結或非一般檔案，拒絕跟隨', $path) unless @before && S_ISREG($before[2]) && !S_ISLNK($before[2]);
    stop('檔案超過 5 MiB，須先人工確認', $path) if $before[7] > 5 * 1024 * 1024;
    sysopen my $input, $full, O_RDONLY or stop('無法讀取檔案', $path);
    binmode $input;
    my @opened = stat $input;
    my @after = lstat $full;
    stop('讀取時檔案已變動或成為連結', $path) unless @after && S_ISREG($after[2]) && $opened[0] == $before[0] && $opened[1] == $before[1] && $after[0] == $before[0] && $after[1] == $before[1];
    my $bytes = '';
    while (1) {
        my $n = read $input, my $chunk, 65536;
        stop('讀取檔案失敗', $path) unless defined $n;
        last unless $n;
        $bytes .= $chunk;
        stop('檔案超過 5 MiB，須先人工確認', $path) if length($bytes) > 5 * 1024 * 1024;
    }
    close $input or stop('關閉檔案失敗', $path);
    return ($bytes, $before[2]);
}

sub entries {
    my ($directory) = @_;
    opendir my $handle, $directory or stop('無法列出目錄', $directory);
    my @names = sort grep { $_ ne '.' && $_ ne '..' } readdir $handle;
    closedir $handle;
    return @names;
}

sub snapshot {
    my ($path, $manifest) = @_;
    safe_name($path);
    my $name = (split '/', $path)[-1];
    my @s = lstat "$root/$path";
    stop('無法讀取檔案屬性', $path) unless @s;
    stop('允許範圍內含連結，拒絕跟隨', $path) if S_ISLNK($s[2]);
    if (S_ISDIR($s[2])) {
        if ($name =~ $excluded_dirs) { $excluded++; return; }
        snapshot("$path/$_", $manifest) for entries("$root/$path");
        return;
    }
    stop('允許範圍內含非一般檔案', $path) unless S_ISREG($s[2]);
    my $pinned = exists $artwork{$path} || ($path =~ m{^crates/core/tests/fixtures/compression/([^/]+)$} && exists $fixtures{$1});
    if (!$pinned && excluded_file($name)) { $excluded++; return; }
    if ($path =~ m{^resources/} && !$resource_files{$path} && $path !~ m{^resources/(?:licenses|manifests)/}) {
        stop('resources 內出現未審查的文字資源，請先維護允許清單', $path);
    }
    my $known = $name =~ /^(?:LICENSE(?:[-.].*)?|COPYING(?:\..*)?|NOTICE(?:\..*)?|Makefile|justfile|rust-toolchain|\.gitignore|\.gitattributes|\.gitmodules|\.editorconfig|\.gitkeep|\.env\.(?:example|sample|template))$/i;
    stop('未允許的檔案類型，請先檢查是否為必要原始碼', $path) unless $pinned || $known || $name =~ $text_ext;
    my ($bytes, $mode) = read_regular($root, $path);
    $total += length $bytes;
    stop('快照超過大小上限（單檔 5 MiB／總計 64 MiB）') if $total > 64 * 1024 * 1024;
    scan($path, $bytes);
    my $output = "$work/source/$path";
    $output =~ m{^(.*)/[^/]+$};
    make_path($1);
    sysopen my $copy, $output, O_WRONLY | O_CREAT | O_EXCL, 0600 or stop('無法建立快照', $path);
    binmode $copy;
    print {$copy} $bytes or stop('無法寫入快照', $path);
    close $copy or stop('無法關閉快照', $path);
    # A Windows checkout may not retain executable permissions; the Mac launcher must.
    my $executable = ($mode & 0111) || $path eq encode('UTF-8', '打包原始碼.command');
    chmod($executable ? 0755 : 0644, $output) or stop('無法設定快照權限', $path);
    print {$manifest} sha256_hex($bytes), "\t", length($bytes), "\t$path\n" or stop('無法寫入驗證清單');
    $seen{$path} = 1;
    $count++;
}

sub manifest {
    open my $input, '<:raw', "$work/manifest" or stop('無法讀取驗證清單', undef, 4);
    my %expected;
    while (my $line = <$input>) {
        chomp $line;
        my ($hash, $size, $path) = split /\t/, $line, 3;
        stop('驗證清單格式錯誤', undef, 4) unless defined($path) && $hash =~ /^[0-9a-f]{64}$/ && $size =~ /^\d+$/ && !exists $expected{$path};
        $expected{$path} = [$hash, $size];
    }
    close $input;
    return %expected;
}

sub zip_entries {
    my ($expected) = @_;
    open my $zip, '<:raw', "$work/archive.zip" or stop('無法開啟暫存 ZIP', undef, 4);
    my $length = -s $zip;
    # Read only central-directory metadata, never a locale-filtered display list.
    # ZIP creation, CRC testing and extraction remain with the system zip/unzip.
    my $at = sub {
        my ($offset, $size) = @_;
        stop('ZIP 目錄紀錄越界或截斷', undef, 4) if $offset < 0 || $size < 0 || $offset > $length || $size > $length - $offset;
        seek($zip, $offset, SEEK_SET) or stop('無法定位 ZIP 目錄紀錄', undef, 4);
        my $bytes = '';
        while (length($bytes) < $size) {
            my $read = read($zip, my $chunk, $size - length($bytes));
            stop('無法完整讀取 ZIP 目錄紀錄', undef, 4) unless defined($read) && $read > 0;
            $bytes .= $chunk;
        }
        return $bytes;
    };
    # Our new archives have no archive comments, disk splits or encryption.
    my $end = $length - 22;
    my ($signature, $disk, $start_disk, $disk_count, $count, $size, $offset, $comment) =
        unpack('VvvvvVVv', $at->($end, 22));
    stop('ZIP 結尾紀錄錯誤（須為無註解的單一 ZIP）', undef, 4) unless $signature == 0x06054b50 && !$disk && !$start_disk && !$comment;
    if ($end >= 20 && substr($at->($end - 20, 20), 0, 4) eq "PK\x06\x07") {
        my (undef, $zip64_disk, $zip64_offset, $disks) = unpack('VVQ<V', $at->($end - 20, 20));
        stop('ZIP64 分割磁碟或目錄位置錯誤', undef, 4) unless !$zip64_disk && $disks == 1 && $zip64_offset < $end - 20;
        my ($sig64, $record_size, undef, undef, $disk64, $start64, $disk_count64, $count64, $size64, $offset64) =
            unpack('VQ<vvVVQ<Q<Q<Q<', $at->($zip64_offset, 56));
        stop('ZIP64 結尾紀錄錯誤', undef, 4) unless $sig64 == 0x06064b50 && $record_size >= 44 &&
            $record_size == $end - 20 - $zip64_offset - 12 && !$disk64 && !$start64 && $disk_count64 == $count64 &&
            ($disk_count == 0xffff || $disk_count == $disk_count64) && ($count == 0xffff || $count == $count64) &&
            ($size == 0xffffffff || $size == $size64) && ($offset == 0xffffffff || $offset == $offset64);
        ($disk_count, $count, $size, $offset, $end) = ($disk_count64, $count64, $size64, $offset64, $zip64_offset);
    }
    stop('ZIP 目錄數量或範圍不符', undef, 4) unless $count == keys(%$expected) && $disk_count == $count &&
        $offset <= $end && $size == $end - $offset;
    my $directory_start = $offset;
    open my $list, '>:raw', "$work/archive.entries" or stop('無法儲存原始 ZIP 項目清單', undef, 4);
    for (1 .. $count) {
        my $header = $at->($offset, 46);
        stop('ZIP 中央目錄標頭錯誤', undef, 4) unless substr($header, 0, 4) eq "PK\x01\x02";
        my ($name_size, $extra_size, $comment_size, $entry_disk) = unpack('vvvv', substr($header, 28, 8));
        my $flags = unpack('v', substr($header, 8, 2));
        my $attributes = unpack('V', substr($header, 38, 4));
        my $local_offset = unpack('V', substr($header, 42, 4));
        my $next = $offset + 46 + $name_size + $extra_size + $comment_size;
        stop('ZIP 項目中繼資料或範圍錯誤', undef, 4) if !$name_size || $next > $end || $comment_size || $entry_disk || ($flags & 0x41) || S_ISLNK($attributes >> 16);
        my $raw_name = $at->($offset + 46, $name_size);
        stop('ZIP 原始檔名含不安全路徑', undef, 4) if $raw_name =~ /[\x00-\x1f\x7f]|^\/|^[A-Za-z]:|(?:^|\/)\.\.?(?:\/|$)/;
        my $extra = $at->($offset + 46 + $name_size, $extra_size);
        my $unicode_name;
        while (length $extra) {
            stop('ZIP 額外欄位截斷', undef, 4) if length($extra) < 4;
            my ($id, $field_size) = unpack('vv', substr($extra, 0, 4, ''));
            stop('ZIP 額外欄位長度錯誤', undef, 4) if $field_size > length($extra);
            my $field = substr($extra, 0, $field_size, '');
            if ($id == 1 && $local_offset == 0xffffffff) {
                my $skip = (unpack('V', substr($header, 24, 4)) == 0xffffffff ? 8 : 0) +
                    (unpack('V', substr($header, 20, 4)) == 0xffffffff ? 8 : 0);
                stop('ZIP64 項目位置欄位截斷', undef, 4) if $field_size < $skip + 8;
                $local_offset = unpack('Q<', substr($field, $skip, 8));
            }
            if ($id == 0x7075) {
                # Native Windows Info-ZIP can use a legacy name and a Unicode
                # path field. Bind it to the exact raw name before trusting it.
                stop('ZIP Unicode 路徑欄位錯誤', undef, 4) if defined($unicode_name) || $field_size < 5 ||
                    ord(substr($field, 0, 1)) != 1 || unpack('V', substr($field, 1, 4)) != crc32($raw_name);
                $unicode_name = substr($field, 5);
            }
        }
        my $path = defined($unicode_name) ? $unicode_name : $raw_name;
        stop('ZIP UTF-8 檔名與 Unicode 欄位不一致', undef, 4) if ($flags & 0x800) && $path ne $raw_name;
        safe_name($path, 4);
        stop('ZIP 有非預期、重複或不安全項目', $path, 4) unless exists $expected->{$path};
        stop('ZIP 項目位置不在檔案資料區', $path, 4) if $local_offset >= $directory_start;
        my $local = $at->($local_offset, 30);
        my ($local_name_size, $local_extra_size) = unpack('vv', substr($local, 26, 4));
        stop('ZIP 本機標頭與中央目錄檔名或旗標不符', $path, 4) unless substr($local, 0, 4) eq "PK\x03\x04" &&
            $local_name_size == $name_size && $local_offset + 30 + $name_size + $local_extra_size <= $directory_start &&
            (unpack('v', substr($local, 6, 2)) & 0x841) == ($flags & 0x841) &&
            $at->($local_offset + 30, $name_size) eq $raw_name;
        print {$list} "$path\n" or stop('無法寫入原始 ZIP 項目清單', undef, 4);
        delete $expected->{$path};
        $offset = $next;
    }
    stop('ZIP 中央目錄長度或項目不符', undef, 4) if $offset != $end || keys %$expected;
    close $list or stop('無法關閉原始 ZIP 項目清單', undef, 4);
    close $zip or stop('無法關閉暫存 ZIP', undef, 4);
}

if ($operation eq 'prepare') {
    my @r = lstat $root;
    stop('專案根目錄必須是實體目錄') unless @r && S_ISDIR($r[2]) && !S_ISLNK($r[2]);
    for my $path (grep { $_ ne '.github' } @roots) {
        my @s = lstat "$root/$path";
        stop('缺少必要實體目錄', $path) unless @s && S_ISDIR($s[2]) && !S_ISLNK($s[2]);
    }
    open my $manifest, '>:raw', "$work/manifest" or stop('無法建立驗證清單');
    for my $name (entries($root)) {
        safe_name($name);
        my @s = lstat "$root/$name";
        stop('無法讀取根目錄項目', $name) unless @s;
        if ($roots{$name} || $root_files{$name}) { snapshot($name, $manifest); }
        elsif (S_ISDIR($s[2]) && ($name =~ $excluded_dirs || $name =~ /^(?:config|build-inputs|pdf2zh_files|--google|test)$/)) { $excluded++; }
        elsif (!S_ISDIR($s[2]) && excluded_file($name)) { $excluded++; }
        else { stop('發現未分類的根目錄項目，請先維護允許清單', $name); }
    }
    close $manifest or stop('無法關閉驗證清單');
    for my $path (@required) { stop('缺少必要檔案或必要檔案被排除', $path) unless $seen{$path}; }
    print "$count\t$total\t$excluded\t$private\n";
} elsif ($operation eq 'entries') {
    my %expected = manifest();
    zip_entries(\%expected);
} elsif ($operation eq 'verify') {
    my %expected = manifest();
    my @pending = ('');
    while (@pending) {
        my $directory = shift @pending;
        for my $name (entries("$work/verified" . (length($directory) ? "/$directory" : ''))) {
            my $path = length($directory) ? "$directory/$name" : $name;
            my @s = lstat "$work/verified/$path";
            stop('ZIP 解壓後含連結或非一般項目', $path, 4) unless @s && !S_ISLNK($s[2]) && (S_ISDIR($s[2]) || S_ISREG($s[2]));
            if (S_ISDIR($s[2])) { push @pending, $path; next; }
            stop('ZIP 解壓後含非預期檔案', $path, 4) unless exists $expected{$path};
            my ($bytes) = read_regular("$work/verified", $path);
            my ($hash, $size) = @{$expected{$path}};
            stop('ZIP 內檔案長度或 SHA256 不符', $path, 4) if length($bytes) != $size || sha256_hex($bytes) ne $hash;
            scan($path, $bytes);
            delete $expected{$path};
        }
    }
    stop('ZIP 解壓後缺少檔案', undef, 4) if keys %expected;
} elsif ($operation eq 'publish') {
    # Same-filesystem rename atomically replaces a regular file, without delete-first.
    my @old = lstat $root;
    stop('ZIP 目的地是目錄、連結或非一般檔案，拒絕取代', undef, 4) if @old && (!S_ISREG($old[2]) || S_ISLNK($old[2]));
    rename("$work/archive.zip", $root) or stop('無法原子取代 ZIP；請確認檔案權限、空間或是否被占用', undef, 4);
} else {
    stop('不支援的驗證階段');
}
PERL
}

for tool in perl zip unzip mktemp rm wc; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf '缺少 macOS 系統工具：%s\n' "$tool" >&2
        exit 2
    fi
done

script=${BASH_SOURCE[0]}
case "$script" in /*) ;; *) script="$PWD/$script" ;; esac
script_directory=${script%/*}
if [ -L "$script" ] || [ -L "$script_directory" ]; then
    printf '%s\n' '腳本或專案根目錄是連結，請從實體目錄執行。' >&2
    exit 2
fi
root=$(cd -P -- "$script_directory" && pwd -P)
parent=${root%/*}
[ -n "$parent" ] || parent='/'
destination="$parent/BabelOxide-source-review.zip"
if [ -L "$destination" ] || { [ -e "$destination" ] && [ ! -f "$destination" ]; }; then
    printf '%s\n' 'ZIP 目的地是目錄、連結或非一般檔案，拒絕取代。' >&2
    exit 2
fi

printf 'BabelOxide 原始碼審查 ZIP 打包工具\n專案：%s\n' "$root"
stage='建立安全暫存目錄'
work=$(mktemp -d "$parent/.BabelOxide-source-review.XXXXXXXX")
stage='允許清單、文字與敏感資訊檢查'
printf '%s\n' '[1/4] 依白名單讀取快照，檢查大小、圖示／Fixture 雜湊與疑似憑證...'
stats=$(review_files prepare "$root" "$work")
IFS=$'\t' read -r file_count source_bytes excluded_count private_count <<< "$stats"

stage='建立暫存 ZIP'
printf '%s\n' '[2/4] 在專案外建立暫存 ZIP...'
files=()
while IFS=$'\t' read -r hash size path; do
    files+=("./$path")
done < "$work/manifest"
(
    cd -- "$work/source"
    zip -q -X -nw "$work/archive.zip" -- "${files[@]}"
)

stage='ZIP 原始項目清單檢查'
printf '%s\n' '[3/4] 檢查 ZIP 項目及完整性，解壓後逐檔比對長度、SHA256 並複查憑證...'
review_files entries "$root" "$work"
stage='ZIP CRC 完整性檢查'
unzip -tqq "$work/archive.zip"
stage='ZIP 解壓縮'
unzip -qq "$work/archive.zip" -d "$work/verified"
stage='ZIP 逐檔 SHA256 與敏感資訊複查'
review_files verify "$root" "$work"
zip_bytes=$(wc -c < "$work/archive.zip")

stage='原子發布 ZIP'
printf '%s\n' '[4/4] 驗證通過，原子取代上一版 ZIP...'
review_files publish "$destination" "$work"
published=1
printf '\n打包成功。\nZIP 完整路徑：%s\nZIP 大小：%s bytes\n包含的檔案數量：%s\n原始碼總大小：%s bytes\n' "$destination" "$zip_bytes" "$file_count" "$source_bytes"
printf '驗證結果：ZIP 完整性、項目清單、逐檔 SHA256 及敏感資訊複查均通過。\n已略過的項目：%s（私密檔名：%s；不遞迴計算已排除目錄）。\n' "$excluded_count" "$private_count"
printf '%s\n' '主要排除：.git/、.venv/、target/、dist/、build-inputs/、config/、快取、Log、私人 PDF、模型、大型字型及編譯產物。'
printf '%s\n' '此檢查可攔截常見疑似憑證；發現時只顯示檔案路徑，不顯示憑證內容。'
