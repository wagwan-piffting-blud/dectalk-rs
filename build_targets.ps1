# Build dectalk for every shipping target and collect the results into dist/.
#
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Only linux-arm64
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -NoMac
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Dist D:\drop
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Check
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -NoBuild -Check   check dist/ as it is
#
# -Only takes macos-universal, windows-amd64, windows-x86, linux-amd64,
# linux-arm64, linux-armv7 or linux-armv6.
#
# Windows and Linux build here with no cross toolchain. Windows is MSVC with
# the C runtime linked statically, so no VC++ redistributable is needed. Linux
# is musl, linked by Rust's own bundled rust-lld, so each binary is static with
# no runtime dependency on the target.
#
# macOS cannot be linked from here, so it goes to the Mac over ssh:
# build_macos.sh builds both slices, lipos them into one universal binary,
# signs and notarizes it. -MacPath must be THIS checkout as the Mac sees it (a
# share), because the result is picked up from dist/ on this side. It runs
# first, so the keychain password prompt comes while you are still here.
# Double-clicked from Explorer, the script skips macOS, as -NoMac does, and
# waits for Enter before the window closes, on success or failure.
#
# -Check renders every check case below through every binary built in this
# run and demands that each output's SHA256 equal the one from windows-amd64.
# The reference is rendered twice, once as "ref", so run-to-run determinism is
# checked as well. Windows binaries run here; Linux ones under WSL, the ARM
# ones through qemu-user on the CPU model they target (`apt install
# qemu-user-static` in the default WSL distro); both macOS slices on the Mac
# over ssh, the Intel one under Rosetta. Everything the check reads or writes
# must be on the share the Mac sees. A failed check leaves its renders in
# dist/.check.
#
# Run it from PowerShell, not cmd.

param(
    [string]$Only = "",
    [string]$Dist = "dist",
    [string]$MacHost = "wags@192.168.1.86",
    [string]$MacPath = "",
    [switch]$NoMac,
    [switch]$Check,
    [switch]$NoBuild,
    [switch]$NoDist
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# Double-clicked, the script is started by Explorer: the Mac's keychain
# password prompt never reaches the window, so the build would wait forever,
# and the window would close on the result before it could be read.
$parent = (Get-CimInstance Win32_Process -Filter "ProcessId = $PID").ParentProcessId
$FromExplorer = (Get-Process -Id $parent -ErrorAction SilentlyContinue).ProcessName -eq "explorer"
if ($FromExplorer -and -not $NoMac) {
    Write-Host "started from Explorer: skipping macos-universal; build it from a terminal with" -ForegroundColor Yellow
    Write-Host "  powershell -ExecutionPolicy Bypass -File build_mac_all.ps1" -ForegroundColor Yellow
    $NoMac = $true
}
trap {
    if ($FromExplorer) {
        Write-Host ""
        Write-Host "FAILED: $_" -ForegroundColor Red
        [void](Read-Host "press Enter to close")
    }
    break
}

# ---- per-app -----------------------------------------------------------------

$Bin = "dectalk"
$CrateDir = "dectalk-rs"
$Select = @("--bin", "dectalk")
if (-not $MacPath) { $MacPath = "/Volumes/LocalUser/Documents/dect" }
$Flags = @{ all = @(); windows = @(); linux = @() }

function Invoke-AppPre { }

function Get-AppArtifact($x, [string]$p) { return $p }

# Every run renders each phrase (or only those listed in p). {in} and {out}
# are the phrase and output files; any other {key} is $CheckPaths[key], a
# Windows path that is converted for WSL and the Mac. Phrases are written as
# raw ASCII bytes, because a BOM would be spoken.
$CheckPhrases = @(
    "The National Weather Service in Norman has issued a severe thunderstorm warning for Cleveland County until 5:45 PM CDT.",
    "[:np] Hello, I am Paul. [:nh] I am Harry. [:nf] I am Frank. [:nb] I am Betty. [:nu] I am Ursula.",
    "[:rate 350] This is fast. [:rate 120] This is slow. [:rate 180] [:tone 440,500] Back to normal.",
    "On 10/07/2026 the total was `$1,234.56; call 1-800-555-0199 or write to wx@example.gov."
)
$CheckPaths = @{}
$CheckRuns = @(
    @{ k = "11k"; a = @("-o", "{out}", "-f", "{in}") },
    @{ k = "8k";  a = @("--8k", "-o", "{out}", "-f", "{in}") }
)

# ---- end per-app -------------------------------------------------------------

# n     short name, also the -Only key and the dist filename suffix
# t     rust target triple
# arch  what the built binary must report, so a mis-targeted build cannot ship
# run   where -Check runs it
$targets = @(
    @{ n = "macos-universal"; t = ""; os = "macos";
       note = "Apple silicon + Intel, built on $MacHost"; arch = "Mach-O/arm64+x86_64"; run = "mac";
       out = "$Bin-macos-universal" },
    @{ n = "windows-amd64"; t = "x86_64-pc-windows-msvc"; os = "windows";
       note = "Windows 10/11 64-bit"; arch = "PE/x86-64"; run = "native";
       out = "$Bin-windows-amd64.exe" },
    @{ n = "windows-x86"; t = "i686-pc-windows-msvc"; os = "windows";
       note = "32-bit Windows"; arch = "PE/x86"; run = "native";
       out = "$Bin-windows-x86.exe" },
    @{ n = "linux-amd64"; t = "x86_64-unknown-linux-musl"; os = "linux";
       note = "64-bit Linux PC, server, Docker, WSL"; arch = "ELF/x86-64/64"; run = "wsl";
       out = "$Bin-linux-amd64" },
    @{ n = "linux-arm64"; t = "aarch64-unknown-linux-musl"; os = "linux";
       note = "64-bit Raspberry Pi OS: Pi 3/4/5, Zero 2 W"; arch = "ELF/AArch64/64"; run = "wsl"; qemu = "qemu-aarch64-static";
       out = "$Bin-linux-arm64" },
    @{ n = "linux-armv7"; t = "armv7-unknown-linux-musleabihf"; os = "linux";
       note = "32-bit Raspberry Pi OS on Pi 2/3/4"; arch = "ELF/ARM/32"; run = "wsl"; qemu = "qemu-arm-static -cpu cortex-a7";
       out = "$Bin-linux-armv7" },
    @{ n = "linux-armv6"; t = "arm-unknown-linux-musleabihf"; os = "linux";
       note = "Pi 1, Pi Zero / Zero W"; arch = "ELF/ARM/32"; run = "wsl"; qemu = "qemu-arm-static -cpu arm1176";
       out = "$Bin-linux-armv6" }
)

if ($Only -and -not ($targets | Where-Object { $_.n -eq $Only })) {
    throw "-Only $Only is not a target: $(($targets | ForEach-Object { $_.n }) -join ', ')"
}
if ($Check -and -not $CheckRuns) { throw "-Check: $Bin has no check cases" }

# All flags go through CARGO_TARGET_<TRIPLE>_RUSTFLAGS, so a stray RUSTFLAGS
# would replace them wholesale.
Remove-Item env:RUSTFLAGS, env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
foreach ($x in $targets) {
    if (-not $x.t) { continue }
    $v = $x.t.ToUpper().Replace("-", "_")
    $f = @("-Ctarget-feature=+crt-static") + $Flags.all + $Flags[$x.os]
    Set-Item "env:CARGO_TARGET_${v}_RUSTFLAGS" ($f -join " ")
    if ($x.os -eq "linux") { Set-Item "env:CARGO_TARGET_${v}_LINKER" "rust-lld" }
}

function Get-TargetDir {
    if ($env:CARGO_TARGET_DIR) { return $env:CARGO_TARGET_DIR }
    return Join-Path $PSScriptRoot "$CrateDir/target"
}

# What the file header actually says. It is also the first thing worth
# checking on "exec format error".
function Get-BinArch([string]$path) {
    $b = [System.IO.File]::ReadAllBytes($path)
    if ($b.Length -ge 20 -and $b[0] -eq 0x7F -and $b[1] -eq 0x45 -and $b[2] -eq 0x4C -and $b[3] -eq 0x46) {
        $bits = if ($b[4] -eq 2) { 64 } else { 32 }
        $machine = $b[18] -bor ($b[19] -shl 8)
        switch ($machine) {
            0x28 { return "ELF/ARM/$bits" }
            0x3E { return "ELF/x86-64/$bits" }
            0xB7 { return "ELF/AArch64/$bits" }
            default { return ("ELF/0x{0:x}/{1}" -f $machine, $bits) }
        }
    }
    if ($b.Length -ge 0x40 -and $b[0] -eq 0x4D -and $b[1] -eq 0x5A) {
        $pe = [BitConverter]::ToInt32($b, 0x3C)
        $machine = [BitConverter]::ToUInt16($b, $pe + 4)
        switch ($machine) {
            0x8664 { return "PE/x86-64" }
            0x014C { return "PE/x86" }
            0xAA64 { return "PE/ARM64" }
            default { return ("PE/0x{0:x}" -f $machine) }
        }
    }
    # Fat Mach-O: big-endian count, then 20 bytes per slice led by its cputype.
    if ($b.Length -ge 8 -and $b[0] -eq 0xCA -and $b[1] -eq 0xFE -and $b[2] -eq 0xBA -and $b[3] -eq 0xBE) {
        $n = ([int]$b[4] -shl 24) -bor ([int]$b[5] -shl 16) -bor ([int]$b[6] -shl 8) -bor $b[7]
        $slices = @()
        for ($i = 0; $i -lt $n; $i++) {
            $o = 8 + 20 * $i
            $cpu = ([int]$b[$o] -shl 24) -bor ([int]$b[$o + 1] -shl 16) -bor ([int]$b[$o + 2] -shl 8) -bor $b[$o + 3]
            switch ($cpu) {
                0x0100000C { $slices += "arm64" }
                0x01000007 { $slices += "x86_64" }
                default { $slices += ("0x{0:x}" -f $cpu) }
            }
        }
        return "Mach-O/" + (($slices | Sort-Object) -join "+")
    }
    if ($b.Length -ge 4 -and $b[0] -eq 0xCF -and $b[1] -eq 0xFA -and $b[2] -eq 0xED -and $b[3] -eq 0xFE) {
        return "Mach-O/single"
    }
    return "unknown"
}

function Get-Shipped($b) {
    if ($NoDist) { return $b.Source }
    return Join-Path (Resolve-Path $Dist).Path $b.Name
}

function ConvertTo-WslPath([string]$p) {
    $ErrorActionPreference = "Continue"
    $r = & wsl -e wslpath -a $p 2>$null
    if ($LASTEXITCODE -ne 0) { throw "WSL is needed to check the Linux targets" }
    return $r.Trim()
}

# $MacPath is $PSScriptRoot as the Mac sees it, so a path elsewhere maps by
# climbing both until one holds it.
function ConvertTo-MacPath([string]$p) {
    $full = [System.IO.Path]::GetFullPath($p).TrimEnd("\")
    $base = $PSScriptRoot.TrimEnd("\")
    $mac = $MacPath.TrimEnd("/")
    while (-not ($full -eq $base -or $full.StartsWith("$base\", [StringComparison]::OrdinalIgnoreCase))) {
        $up = Split-Path $base -Parent
        if (-not $up -or $mac -notmatch "/[^/]+$") { throw "$p is not on the share the Mac sees" }
        $base = $up.TrimEnd("\")
        $mac = $mac -replace "/[^/]+$", ""
    }
    return $mac + $full.Substring($base.Length).Replace("\", "/")
}

# The previous binary is set aside rather than deleted, so a Mac that is off
# does not cost the last good build; a file left in dist/ afterwards is then
# known to be this run's.
function Build-Mac($x) {
    $p = Join-Path $PSScriptRoot "dist/$($x.out)"
    $prev = "$p.prev"
    if (Test-Path $p) { Move-Item $p $prev -Force }
    try {
        & ssh -t $MacHost "cd '$MacPath' && sh build_macos.sh"
        if ($LASTEXITCODE -ne 0) { throw "macos-universal failed" }
        if (-not (Test-Path $p)) { throw "$p not here - is $MacPath this checkout?" }
    } catch {
        if (Test-Path $prev) { Move-Item $prev $p -Force }
        throw
    }
    if (Test-Path $prev) { Remove-Item $prev -Force }
    return $p
}

# ---- check -------------------------------------------------------------------

function Get-CheckCases {
    foreach ($r in $CheckRuns) {
        $ps = if ($null -ne $r.p) { $r.p } else { 0..($CheckPhrases.Count - 1) }
        foreach ($i in $ps) {
            [pscustomobject]@{ Name = "$($r.k)-p$i"; Phrase = $i; Args = $r.a }
        }
    }
}

function Expand-CheckArgs($case, $paths, [string]$work, [string]$sep, [string]$tag) {
    $map = @{ in = "$work${sep}p$($case.Phrase).txt"; out = "$work$sep$tag-$($case.Name).wav" }
    foreach ($k in $paths.Keys) { $map[$k] = $paths[$k] }
    foreach ($a in $case.Args) {
        $s = $a
        foreach ($k in $map.Keys) { $s = $s.Replace("{$k}", $map[$k]) }
        $s
    }
}

function ConvertTo-ShArg([string]$s) { return "'" + $s.Replace("'", "'\''") + "'" }

# One script per target, rendering up to $2 cases at once; a failed render is
# written to $1 by name.
function Write-RenderScript([string]$path, [string[]]$lines) {
    $sh = @(
        'fails="$1"; max="$2"; n=0',
        'run() { t=$1; shift; "$@" >/dev/null 2>&1 || echo "$t exited $?" >> "$fails"; }',
        'go() { run "$@" & n=$((n + 1)); if [ $n -ge "$max" ]; then wait; n=0; fi; }'
    ) + $lines + @("wait")
    [System.IO.File]::WriteAllText($path, ($sh -join "`n") + "`n")
}

function Invoke-Check {
    $t0 = Get-Date
    $cases = @(Get-CheckCases)
    foreach ($k in @($CheckPaths.Keys)) {
        $CheckPaths[$k] = [System.IO.Path]::GetFullPath($CheckPaths[$k])
        if (-not (Test-Path $CheckPaths[$k])) { throw "-Check: {$k} is $($CheckPaths[$k]), which is not there" }
    }
    $work = Join-Path $PSScriptRoot "dist/.check"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Path $work | Out-Null
    for ($i = 0; $i -lt $CheckPhrases.Count; $i++) {
        [System.IO.File]::WriteAllBytes((Join-Path $work "p$i.txt"), [System.Text.Encoding]::ASCII.GetBytes($CheckPhrases[$i]))
    }

    $ref = $built | Where-Object { $_.Target -eq "windows-amd64" } | Select-Object -First 1
    $refExe = if ($ref) { Get-Shipped $ref } else { Join-Path (Get-TargetDir) "x86_64-pc-windows-msvc/release/$Bin.exe" }
    if (-not (Test-Path $refExe)) { throw "-Check needs a windows-amd64 build at $refExe" }

    # tag -> how it runs; macos-universal is two of them, one per slice
    $runs = @([pscustomobject]@{ Tag = "ref"; Run = "native"; Exe = $refExe; Qemu = $null })
    foreach ($b in $built) {
        switch ($b.Run) {
            "native" { $runs += [pscustomobject]@{ Tag = $b.Target; Run = "native"; Exe = (Get-Shipped $b); Qemu = $null } }
            "wsl"    { $runs += [pscustomobject]@{ Tag = $b.Target; Run = "wsl"; Exe = (Get-Shipped $b); Qemu = $b.Qemu } }
            "mac" {
                foreach ($s in "arm64", "x86_64") {
                    $runs += [pscustomobject]@{ Tag = "macos-$s"; Run = "mac"; Exe = $b.Source; Qemu = "arch -$s" }
                }
            }
        }
    }

    $wslRuns = @($runs | Where-Object { $_.Run -eq "wsl" })
    $macRuns = @($runs | Where-Object { $_.Run -eq "mac" })
    $procs = @()

    if ($wslRuns) {
        $wWork = ConvertTo-WslPath $work
        $wPaths = @{}
        foreach ($k in $CheckPaths.Keys) { $wPaths[$k] = ConvertTo-WslPath $CheckPaths[$k] }
        $per = [math]::Max(2, [math]::Floor([Environment]::ProcessorCount / $wslRuns.Count))
        foreach ($r in $wslRuns) {
            $exe = ConvertTo-WslPath $r.Exe
            $lines = foreach ($c in $cases) {
                $argv = @($c.Name) + @(if ($r.Qemu) { -split $r.Qemu }) + @($exe) + @(Expand-CheckArgs $c $wPaths $wWork "/" $r.Tag)
                "go " + (($argv | ForEach-Object { ConvertTo-ShArg $_ }) -join " ")
            }
            Write-RenderScript (Join-Path $work "run-$($r.Tag).sh") $lines
            $procs += Start-Process wsl -NoNewWindow -PassThru -ArgumentList @(
                "-e", "sh", "$wWork/run-$($r.Tag).sh", "$wWork/fails-$($r.Tag).txt", $per)
        }
    }

    if ($macRuns) {
        $mWork = ConvertTo-MacPath $work
        $mPaths = @{}
        foreach ($k in $CheckPaths.Keys) { $mPaths[$k] = ConvertTo-MacPath $CheckPaths[$k] }
        $lines = foreach ($r in $macRuns) {
            $exe = ConvertTo-MacPath $r.Exe
            foreach ($c in $cases) {
                $argv = @("$($r.Tag)/$($c.Name)") + @(-split $r.Qemu) + @($exe) + @(Expand-CheckArgs $c $mPaths $mWork "/" $r.Tag)
                "go " + (($argv | ForEach-Object { ConvertTo-ShArg $_ }) -join " ")
            }
        }
        Write-RenderScript (Join-Path $work "run-macos.sh") $lines
        $procs += Start-Process ssh -NoNewWindow -PassThru -ArgumentList @(
            $MacHost, "sh '$mWork/run-macos.sh' '$mWork/fails-macos.txt' `$(sysctl -n hw.ncpu)")
    }

    # The native ones run here while WSL and the Mac work.
    foreach ($r in ($runs | Where-Object { $_.Run -eq "native" })) {
        foreach ($c in $cases) {
            $argv = @(Expand-CheckArgs $c $CheckPaths $work "\" $r.Tag)
            $ErrorActionPreference = "Continue"
            & $r.Exe @argv 2>$null | Out-Null
            $code = $LASTEXITCODE
            $ErrorActionPreference = "Stop"
            if ($code -ne 0) { Add-Content (Join-Path $work "fails-$($r.Tag).txt") "$($c.Name) exited $code" }
        }
    }
    $procs | Wait-Process

    $refHash = @{}
    foreach ($c in $cases) {
        $f = Join-Path $work "ref-$($c.Name).wav"
        if (-not (Test-Path $f)) { throw "the reference failed on $($c.Name); see $work" }
        $refHash[$c.Name] = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
    }
    Write-Host ""
    Write-Host "reference SHA256 (windows-amd64):" -ForegroundColor DarkGray
    foreach ($c in $cases) { Write-Host ("  {0}  {1}" -f $refHash[$c.Name], $c.Name) -ForegroundColor DarkGray }
    Write-Host ""

    $fail = 0
    foreach ($r in ($runs | Where-Object { $_.Tag -ne "ref" })) {
        $bad = @()
        foreach ($c in $cases) {
            $f = Join-Path $work "$($r.Tag)-$($c.Name).wav"
            if (-not (Test-Path $f)) { $bad += "$($c.Name) (no output)"; continue }
            $h = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
            if ($h -ne $refHash[$c.Name]) { $bad += "$($c.Name) ($($h.Substring(0, 16)))" }
        }
        if ($bad.Count -eq 0) {
            Write-Host ("  {0,-16} identical  {1}/{1}" -f $r.Tag, $cases.Count) -ForegroundColor Green
        } else {
            Write-Host ("  {0,-16} DIFFERS    {1}/{2}: {3}" -f $r.Tag, ($cases.Count - $bad.Count), $cases.Count, ($bad -join ", ")) -ForegroundColor Red
            $fail++
        }
    }
    foreach ($f in Get-ChildItem $work -Filter "fails-*.txt") {
        Get-Content $f.FullName | ForEach-Object { Write-Host "  $($f.BaseName.Substring(6)): $_" -ForegroundColor Red }
    }
    Write-Host ("  {0} cases x {1} binaries in {2:n0} s" -f $cases.Count, ($runs.Count - 1), ((Get-Date) - $t0).TotalSeconds) -ForegroundColor DarkGray
    if ($fail) { throw "$fail binaries differ from windows-amd64; renders kept in $work" }
    Remove-Item -Recurse -Force $work
}

# ---- build -------------------------------------------------------------------

Invoke-AppPre

Push-Location $CrateDir
try {
    $installed = (& rustup target list --installed) -split "`r?`n"
} finally {
    Pop-Location
}

$built = @()
foreach ($x in $targets) {
    if ($Only -and $x.n -ne $Only) { continue }
    if ($x.os -eq "macos" -and $NoMac -and -not $Only) { continue }

    Write-Host ""
    Write-Host "=== $($x.n): $(if ($x.t) { $x.t } else { 'aarch64 + x86_64-apple-darwin' }) " -NoNewline
    Write-Host "($($x.note))" -ForegroundColor DarkGray

    if ($NoBuild) {
        $p = Join-Path $Dist $x.out
    } elseif ($x.os -eq "macos") {
        $p = Build-Mac $x
    } else {
        if ($installed -notcontains $x.t) {
            Write-Host "installing rust target $($x.t)" -ForegroundColor DarkGray
            Push-Location $CrateDir
            try {
                & rustup target add $x.t
                if ($LASTEXITCODE -ne 0) { throw "rustup target add $($x.t) failed" }
            } finally {
                Pop-Location
            }
        }

        # cargo reads .cargo/config.toml and rust-toolchain.toml from the
        # working directory, not the manifest's.
        Push-Location $CrateDir
        try {
            & cargo build --release @Select --target $x.t
            if ($LASTEXITCODE -ne 0) { throw "$($x.n) failed" }
        } finally {
            Pop-Location
        }

        $src = if ($x.os -eq "windows") { "$Bin.exe" } else { $Bin }
        $p = Get-AppArtifact $x (Join-Path (Get-TargetDir) "$($x.t)/release/$src")
    }

    if (-not (Test-Path $p)) { throw "missing $p" }
    $got = Get-BinArch $p
    if ($got -ne $x.arch) { throw "$($x.n): built $got, expected $($x.arch)" }
    $built += [pscustomobject]@{
        Target = $x.n
        Source = (Resolve-Path $p).Path
        Name   = $x.out
        Arch   = $got
        Run    = $x.run
        Qemu   = $x.qemu
        MB     = [math]::Round((Get-Item $p).Length / 1MB, 2)
    }
}

Write-Host ""
$built | Format-Table Target, Name, Arch, MB -AutoSize

if (-not $NoDist) {
    if (-not (Test-Path $Dist)) { New-Item -ItemType Directory -Path $Dist | Out-Null }
    $distFull = (Resolve-Path $Dist).Path
    foreach ($b in $built) {
        $dst = Join-Path $distFull $b.Name
        if ($b.Source -ne $dst) { Copy-Item $b.Source $dst -Force }
    }
    Get-ChildItem $distFull -File | Select-Object Name, Length | Format-Table -AutoSize
    Write-Host "collected into $distFull" -ForegroundColor Green
}

if ($Check) { Invoke-Check }

if ($FromExplorer) {
    Write-Host ""
    [void](Read-Host "done; press Enter to close")
}
