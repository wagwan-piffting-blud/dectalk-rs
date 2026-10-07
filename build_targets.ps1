# Build the dectalk binary for every shipping target from a Windows box, and
# collect the results into dist/.
#
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Only linux-arm64
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Check
#     powershell -ExecutionPolicy Bypass -File build_targets.ps1 -Mac user@mac:/Volumes/share/dect
#
# Nothing here needs a cross toolchain. The Linux targets are musl, linked by
# Rust's own bundled rust-lld (see dectalk-rs/.cargo/config.toml), so each one
# comes out a single static binary with no runtime dependency on the target.
# Both dictionaries are compiled in from dectalk-rs/dict, so a shipped binary
# needs nothing beside it.
#
# -Check renders a fixed set of phrases through every built binary and demands
# byte-identical WAVs against windows-amd64. Linux binaries run under WSL; the
# ARM ones through qemu-user on the CPU model they target, which needs
# `apt install qemu-user-static` in the default WSL distro.
#
# macOS cannot be linked from here, so -Mac hands it to a Mac over ssh:
# build_macos.sh builds both slices, lipos them into one universal binary,
# checks both slices agree, then signs and notarizes it. The path must be THIS
# checkout as the Mac sees it (a share), because the result is picked up from
# dist/ on this side.
#
# Run it from PowerShell, not cmd.

param(
    [string]$Only = "",
    [string]$Dist = "dist",
    [string]$Mac = "",
    [switch]$Check,
    [switch]$NoDist
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$crate = Join-Path $PSScriptRoot "dectalk-rs"

# n     short name, also the -Only key and the dist filename suffix
# t     rust target triple
# arch  what the built binary must report, so a mis-targeted build cannot ship
# wsl   run under WSL for -Check
$targets = @(
    @{ n = "windows-amd64"; t = "x86_64-pc-windows-msvc";
       note = "Windows 10/11 64-bit"; arch = "PE/x86-64"; wsl = $false;
       src = "dectalk.exe"; out = "dectalk-windows-amd64.exe" },
    @{ n = "windows-x86"; t = "i686-pc-windows-msvc";
       note = "32-bit Windows"; arch = "PE/x86"; wsl = $false;
       src = "dectalk.exe"; out = "dectalk-windows-x86.exe" },
    @{ n = "linux-amd64"; t = "x86_64-unknown-linux-musl";
       note = "64-bit Linux PC, server, Docker, WSL"; arch = "ELF/x86-64/64"; wsl = $true;
       src = "dectalk"; out = "dectalk-linux-amd64" },
    @{ n = "linux-arm64"; t = "aarch64-unknown-linux-musl";
       note = "64-bit Raspberry Pi OS: Pi 3/4/5, Zero 2 W"; arch = "ELF/AArch64/64"; wsl = $true; qemu = "qemu-aarch64-static";
       src = "dectalk"; out = "dectalk-linux-arm64" },
    @{ n = "linux-armv7"; t = "armv7-unknown-linux-musleabihf";
       note = "32-bit Raspberry Pi OS on Pi 2/3/4"; arch = "ELF/ARM/32"; wsl = $true; qemu = "qemu-arm-static -cpu cortex-a7";
       src = "dectalk"; out = "dectalk-linux-armv7" },
    @{ n = "linux-armv6"; t = "arm-unknown-linux-musleabihf";
       note = "Pi 1, Pi Zero / Zero W"; arch = "ELF/ARM/32"; wsl = $true; qemu = "qemu-arm-static -cpu arm1176";
       src = "dectalk"; out = "dectalk-linux-armv6" }
)

# What the file header actually says, so an arm64 binary cannot quietly ship as
# armv7. It is also the first thing worth checking on "exec format error".
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
    return "unknown"
}

$installed = (& rustup target list --installed) -split "`r?`n"

$built = @()
foreach ($x in $targets) {
    if ($Only -and $x.n -ne $Only) { continue }

    if ($installed -notcontains $x.t) {
        Write-Host "installing rust target $($x.t)" -ForegroundColor DarkGray
        & rustup target add $x.t
        if ($LASTEXITCODE -ne 0) { throw "rustup target add $($x.t) failed" }
    }

    Write-Host ""
    Write-Host "=== $($x.n): $($x.t) " -NoNewline
    Write-Host "($($x.note))" -ForegroundColor DarkGray

    # cargo reads .cargo/config.toml from the working directory, not the manifest's.
    Push-Location $crate
    try {
        & cargo build --release --bin dectalk --target $x.t
        if ($LASTEXITCODE -ne 0) { throw "$($x.n) failed" }
    } finally {
        Pop-Location
    }

    $p = Join-Path $crate "target/$($x.t)/release/$($x.src)"
    if (-not (Test-Path $p)) { throw "missing $p" }
    $got = Get-BinArch $p
    if ($got -ne $x.arch) { throw "$($x.n): built $got, expected $($x.arch)" }
    $built += [pscustomobject]@{
        Target = $x.n
        Source = (Resolve-Path $p).Path
        Name   = $x.out
        Arch   = $got
        Wsl    = $x.wsl
        Qemu   = $x.qemu
        MB     = [math]::Round((Get-Item $p).Length / 1MB, 2)
    }
}

if ($Mac -and (-not $Only -or $Only -eq "macos-universal")) {
    $hostPart, $path = $Mac -split ":", 2
    Write-Host ""
    Write-Host "=== macos-universal: aarch64 + x86_64-apple-darwin " -NoNewline
    Write-Host "(on $hostPart)" -ForegroundColor DarkGray
    # -t: signing may unlock the login keychain, which prompts for the password
    & ssh -t $hostPart "cd '$path' && sh build_macos.sh"
    if ($LASTEXITCODE -ne 0) { throw "macos-universal failed" }
    $p = "dist/dectalk-macos-universal"
    if (-not (Test-Path $p)) { throw "$p not here - is $path this checkout?" }
    $built += [pscustomobject]@{
        Target = "macos-universal"
        Source = (Resolve-Path $p).Path
        Name   = "dectalk-macos-universal"
        Arch   = "Mach-O arm64+x86_64"
        Wsl    = $null
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

if (-not $Check) { return }

# ---- check -------------------------------------------------------------------
# Checks the files that ship: dist/, or the cargo output with -NoDist. Phrase
# files are written as raw bytes, because a BOM would be spoken.

$phrases = @(
    "The National Weather Service in Norman has issued a severe thunderstorm warning for Cleveland County until 5:45 PM CDT.",
    "[:np] Hello, I am Paul. [:nh] I am Harry. [:nf] I am Frank. [:nb] I am Betty. [:nu] I am Ursula.",
    "[:rate 350] This is fast. [:rate 120] This is slow. [:rate 180] [:tone 440,500] Back to normal.",
    "On 10/07/2026 the total was `$1,234.56; call 1-800-555-0199 or write to wx@example.gov."
)

$work = Join-Path $PSScriptRoot "dist/.check"
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Path $work | Out-Null
for ($i = 0; $i -lt $phrases.Count; $i++) {
    [System.IO.File]::WriteAllBytes((Join-Path $work "p$i.txt"), [System.Text.Encoding]::ASCII.GetBytes($phrases[$i]))
}
$formats = @(@{ k = "11k"; a = @() }, @{ k = "8k"; a = @("--8k") })

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

$ref = $built | Where-Object { $_.Target -eq "windows-amd64" } | Select-Object -First 1
$refExe = if ($ref) { Get-Shipped $ref } else { Join-Path $crate "target/x86_64-pc-windows-msvc/release/dectalk.exe" }
if (-not (Test-Path $refExe)) { throw "-Check needs a windows-amd64 build at $refExe" }

$wslWork = $null
function Invoke-Render($exe, $useWsl, $tag, $qemu) {
    # Windows PowerShell turns redirected native stderr into a terminating error under Stop.
    $ErrorActionPreference = "Continue"
    $dir = if ($useWsl) { $script:wslWork } else { $work }
    $sep = if ($useWsl) { "/" } else { "\" }
    foreach ($f in $formats) {
        for ($i = 0; $i -lt $phrases.Count; $i++) {
            $argv = $f.a + @("-o", "$dir$sep$tag-p$i-$($f.k).wav", "-f", "$dir${sep}p$i.txt")
            if ($useWsl) {
                $runner = @(if ($qemu) { -split $qemu })
                & wsl -e @runner $exe @argv 2>$null
            } else {
                & $exe @argv 2>$null
            }
            if ($LASTEXITCODE -ne 0) { throw "$tag exited $LASTEXITCODE on p$i $($f.k)" }
        }
    }
}

Invoke-Render $refExe $false "ref"

$fail = 0
foreach ($b in $built) {
    if ($null -eq $b.Wsl) { continue }
    $exe = Get-Shipped $b
    if ($b.Wsl) {
        if (-not $wslWork) { $wslWork = ConvertTo-WslPath $work }
        $exe = ConvertTo-WslPath $exe
    }
    Invoke-Render $exe $b.Wsl $b.Target $b.Qemu
    $bad = @()
    foreach ($f in $formats) {
        for ($i = 0; $i -lt $phrases.Count; $i++) {
            $a = [System.IO.File]::ReadAllBytes((Join-Path $work "ref-p$i-$($f.k).wav"))
            $c = [System.IO.File]::ReadAllBytes((Join-Path $work "$($b.Target)-p$i-$($f.k).wav"))
            if ($a.Length -ne $c.Length -or [Convert]::ToBase64String($a) -ne [Convert]::ToBase64String($c)) {
                $bad += "p$i/$($f.k)"
            }
        }
    }
    if ($bad.Count -eq 0) {
        Write-Host ("  {0,-16} identical ({1} renders)" -f $b.Target, ($phrases.Count * $formats.Count)) -ForegroundColor Green
    } else {
        Write-Host ("  {0,-16} DIFFERS: {1}" -f $b.Target, ($bad -join " ")) -ForegroundColor Red
        $fail++
    }
}
Remove-Item -Recurse -Force $work
if ($fail) { throw "$fail target(s) differ from windows-amd64" }
