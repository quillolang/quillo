# Checks that this Windows machine can build and run Quillo, and says what to
# install for anything missing. scripts/doctor.sh does the same on macOS and
# Linux. In PowerShell, from the checkout:
#
#   powershell -ExecutionPolicy Bypass -File scripts\doctor.ps1          check the tools
#   powershell -ExecutionPolicy Bypass -File scripts\doctor.ps1 -Build   then build quillo and run the sample app
#
# Written for Windows PowerShell 5.1 (in every Windows 10 and 11) and later.

param(
    [switch]$Build,
    [Parameter(ValueFromRemainingArguments = $true)] $Rest
)
if ($Rest -contains '--build') { $Build = $true }

$ErrorActionPreference = 'Continue'
$NodeMin = 22        # tests/browser/run.js uses Node's built-in WebSocket
$DiskMinGB = 2       # a release build: about 1 GB of Cargo's compiled dependencies in target\
$DiskTestsGB = 7     # the tests' builds take about 6 GB more

$root = Split-Path -Parent $PSScriptRoot
$rustMin = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^rust-version = "(.*)"').Matches[0].Groups[1].Value
$script:missing = 0

function Ok($what) { Write-Host '  ok       ' -ForegroundColor Green -NoNewline; Write-Host $what }
function Need($what, $fix) {
    Write-Host '  missing  ' -ForegroundColor Red -NoNewline; Write-Host $what
    Write-Host "           $fix" -ForegroundColor DarkGray
    $script:missing++
}
function Opt($what, $fix) {
    Write-Host '  --       ' -ForegroundColor Yellow -NoNewline; Write-Host $what
    Write-Host "           $fix" -ForegroundColor DarkGray
}
function Has($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
# "1.99.0 (abc 2026-09-28)" or "v24.1.0" -> a [version] to compare
function Ver($text) { [version](($text -replace '^v', '') -split '[ -]')[0] }

Write-Host 'To build and run Quillo'

$host_triple = ''
if ((Has 'cargo') -and (Has 'rustc')) {
    $v = ((rustc --version) -split ' ')[1]
    $host_triple = ((rustc -vV | Select-String '^host: ') -replace '^host: ', '').Trim()
    if ((Ver $v) -ge (Ver $rustMin)) { Ok "Rust $v ($host_triple)" }
    else { Need "Rust $v is too old: Quillo needs $rustMin or later" 'rustup update stable' }
} else {
    Need 'Rust (cargo and rustc)' 'install it with rustup-init.exe from https://rustup.rs (let it install the Visual Studio C++ build tools), then open a new terminal'
}

# Rust's default Windows toolchain links with Microsoft's C++ build tools, and
# some dependencies compile C with them (ring, the bundled SQLite, mimalloc).
# They are found the way Rust's cc crate finds them: through vswhere.
$buildTools = 'winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"'
if ($host_triple -like '*-gnu') {
    if (Has 'gcc') { Ok "C compiler (gcc, for the GNU toolchain)" }
    else { Need 'gcc (the Rust toolchain is the GNU one)' 'rustup default stable-msvc (recommended), or install MinGW-w64' }
} else {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $component = 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64'
    if ($host_triple -like 'aarch64-*') { $component = 'Microsoft.VisualStudio.Component.VC.Tools.ARM64' }
    $vs = $null
    if (Test-Path $vswhere) { $vs = & $vswhere -latest -products * -requires $component -property installationPath }
    if ($vs) { Ok "Visual Studio C++ build tools ($vs)" }
    else { Need 'the Visual Studio C++ build tools (Rust links with them; some dependencies compile C)' $buildTools }
}

$drive = (Get-Item $root).PSDrive
$freeGB = [math]::Floor($drive.Free / 1GB)
if ($freeGB -lt $DiskMinGB) { Need "only $freeGB GB free on $($drive.Name):" "a release build keeps about 1 GB of Cargo's compiled dependencies in target\ (quillo itself is about 20 MB)" }
elseif ($freeGB -lt $DiskTestsGB) { Opt "$freeGB GB free on $($drive.Name): enough to build" "running the tests (cargo test) needs about 6 GB more in target\" }
else { Ok "$freeGB GB free on $($drive.Name):" }

if (Has 'Get-NetTCPConnection') {
    $taken = Get-NetTCPConnection -LocalPort 8080 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($taken) {
        $by = (Get-Process -Id $taken.OwningProcess -ErrorAction SilentlyContinue).ProcessName
        Opt "port 8080 is taken (by $by)" 'quillo run listens on 8080 unless given --port N or PORT'
    } else { Ok "port 8080 is free (quillo run's default)" }
}

Write-Host ''
Write-Host 'Optional: the tests, and extras'

if (Has 'node') {
    $v = node --version
    if ((Ver $v) -ge [version]"$NodeMin.0") { Ok "Node $v" }
    else { Opt "Node $v`: the browser tests need $NodeMin or later" 'https://nodejs.org' }
} else {
    Opt "Node $NodeMin or later: the component compiler's tests, the VS Code extension" 'winget install OpenJS.NodeJS.LTS'
}

$chrome = $env:QUILLO_CHROME
if (-not $chrome) {
    foreach ($c in @("$env:ProgramFiles\Google\Chrome\Application\chrome.exe", "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe", "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe")) {
        if (Test-Path $c) { $chrome = $c; break }
    }
    if ($chrome) { Opt "Chrome ($chrome)" 'the browser tests use it once QUILLO_CHROME is set to that path' }
    else { Opt 'Chrome: the browser tests are skipped without it' 'install it, and set QUILLO_CHROME to chrome.exe' }
} elseif (Test-Path $chrome) { Ok "Chrome ($chrome)" }
else { Opt "QUILLO_CHROME is set, but $chrome is not there" 'set it to chrome.exe' }

# quillo app (a native window) shows the page in WebView2, part of Windows 11.
$wv2 = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
if ((Test-Path $wv2) -and (Get-ItemProperty $wv2 -ErrorAction SilentlyContinue).pv) { Ok 'WebView2 (for quillo app, a native window)' }
else { Opt 'WebView2: only for quillo app (cargo build --features app)' 'https://developer.microsoft.com/microsoft-edge/webview2/' }

Write-Host '  (the PostgreSQL and MariaDB tests start their own servers on macOS and Linux only)' -ForegroundColor DarkGray

Write-Host ''
if ($script:missing -gt 0) {
    Write-Host "$($script:missing) thing(s) to install before Quillo builds." -ForegroundColor Red -NoNewline
    Write-Host " Each system's steps: https://quillolang.org/docs/install/"
    exit 1
}
Write-Host 'Everything Quillo needs is here.'

# A quillo on the PATH older than this checkout's build runs when you type
# `quillo`, and fails on newer code as if the code were wrong.
function PathQuillo($built) {
    $onPath = Get-Command quillo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $onPath -or -not (Test-Path $built)) { return }
    $p = $onPath.Source
    if ((Resolve-Path $p).Path -eq (Resolve-Path $built).Path) { return }
    if ((Get-Item $built).LastWriteTime -gt (Get-Item $p).LastWriteTime) {
        Opt "an older quillo is on the PATH: $p ($(& $p --version 2>$null))" 'it runs when you type quillo, not this build: cargo install --path . replaces it'
    }
}

if (-not $Build) {
    PathQuillo (Join-Path $root 'target\release\quillo.exe')
    Write-Host 'Next: cargo build --release   (or: powershell -ExecutionPolicy Bypass -File scripts\doctor.ps1 -Build)'
    exit 0
}

Write-Host ''
Write-Host 'Building (the first build takes a few minutes)'
Push-Location $root
cargo build --release
$built = $LASTEXITCODE -eq 0
Pop-Location
if (-not $built) { Write-Host 'The build failed: see the errors above.' -ForegroundColor Red; exit 1 }
$quillo = Join-Path $root 'target\release\quillo.exe'
PathQuillo $quillo

Write-Host ''
Write-Host 'Running the sample app'
# A copy, so its database is not left in app\.
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("quillo-doctor-" + [guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
Copy-Item -Recurse (Join-Path $root 'app') (Join-Path $tmp 'app')
$main = Join-Path $tmp 'app\main.qwe'
$server = $null
try {
    & $quillo check $main | Out-Null
    if ($LASTEXITCODE -ne 0) { Need 'quillo check app\main.qwe failed' 'run it to see why'; exit 1 }
    Ok 'quillo check app\main.qwe'

    $port = 18080
    if ($env:DOCTOR_PORT) { $port = $env:DOCTOR_PORT }
    $server = Start-Process -FilePath $quillo -ArgumentList @('run', "`"$main`"", '--port', $port, '--log', 'off') -PassThru -NoNewWindow `
        -RedirectStandardOutput (Join-Path $tmp 'out.log') -RedirectStandardError (Join-Path $tmp 'err.log')
    $page = $null
    for ($i = 0; $i -lt 50 -and -not $page; $i++) {
        if ($server.HasExited) { break }
        try { $page = (Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/" -TimeoutSec 2).Content } catch { Start-Sleep -Milliseconds 200 }
    }
    if (-not $page) {
        Need "the sample app did not answer on port $port" (Get-Content (Join-Path $tmp 'err.log') -Tail 3 -ErrorAction SilentlyContinue | Out-String).Trim()
        exit 1
    }
    $todos = (Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/api/todos").Content
    if ($page -like '*<h1>To do</h1>*' -and $todos.Trim() -eq '[]') { Ok 'the sample app answers: its page, and its JSON from SQLite' }
    else { Need 'the sample app answered, but not as expected' 'run quillo run app\main.qwe and open http://localhost:8080'; exit 1 }
} finally {
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue; $server.WaitForExit() }
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host 'Quillo builds and runs here.' -ForegroundColor Green -NoNewline
Write-Host ' Start with: .\target\release\quillo.exe run app\main.qwe --watch'
