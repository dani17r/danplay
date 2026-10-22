# Construye DanPlay entero en Windows. Es el equivalente de scripts/build.sh.
#
#   .\scripts\build-windows.ps1                 interfaz + nucleo + app
#   .\scripts\build-windows.ps1 -Installer      ademas, el instalador (NSIS)
#   .\scripts\build-windows.ps1 -NoTools        sin ffmpeg ni fpcalc dentro
#
# Por que hace falta ejecutarlo AQUI y no en Linux: PyInstaller no compila
# para otro sistema. El nucleo Python (danplay-core.exe) solo se puede
# empaquetar en Windows. El resto (interfaz y app de Rust) si se podria cruzar,
# pero sin nucleo no serviria de nada.
#
# Lo que hace falta tener puesto:
#   - Python 3.13            https://www.python.org/downloads/
#   - Node 24                https://nodejs.org/
#   - Rust (MSVC)            https://rustup.rs/
#   - Herramientas de compilacion de Visual Studio (Desktop C++)
#   - WebView2 viene ya en Windows 10/11 actualizados
[CmdletBinding()]
param(
    [switch]$Installer,
    [switch]$NoTools
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "`n$text" -ForegroundColor Red; exit 1 }

foreach ($program in @('python', 'node', 'npm', 'cargo', 'rustc')) {
    if (-not (Get-Command $program -ErrorAction SilentlyContinue)) {
        Fail "Falta $program. Mira la cabecera de este archivo."
    }
}

# --- entorno de Python -------------------------------------------------------
if (-not (Test-Path .venv)) {
    Step '0/5  entorno de Python'
    python -m venv .venv
}
$py = Join-Path $root '.venv\Scripts\python.exe'
& $py -m pip install --upgrade pip --quiet
& $py -m pip install -r requirements.txt pyinstaller --quiet

# --- interfaz ----------------------------------------------------------------
Step '1/5  interfaz (Vite)'
Push-Location desktop
if (-not (Test-Path node_modules)) { npm ci }
npx vite build
Pop-Location

# --- nucleo ------------------------------------------------------------------
Step '2/5  nucleo empaquetado (PyInstaller)'
$workDir = Join-Path $env:TEMP "danplay-build-$PID"
& $py -m PyInstaller --noconfirm --clean --workpath $workDir --distpath dist/core packaging/core.spec
Remove-Item -Recurse -Force $workDir -ErrorAction SilentlyContinue
if (-not (Test-Path dist/core/danplay-core/danplay-core.exe)) { Fail 'no se genero dist/core/danplay-core/danplay-core.exe' }

# Que arranque de verdad antes de meterlo en un instalador: un núcleo que no
# levanta solo se descubre al abrir la aplicacion ya instalada.
Step '3/5  comprobar que el nucleo arranca'
$port = 8731
$env:DANPLAY_TOKEN = [guid]::NewGuid().ToString('N')
$process = Start-Process -PassThru -WindowStyle Hidden -FilePath dist/core/danplay-core/danplay-core.exe `
           -ArgumentList '--host', '127.0.0.1', '--port', "$port"
try {
    $ready = $false
    foreach ($attempt in 1..30) {
        Start-Sleep -Seconds 1
        try {
            $r = Invoke-WebRequest -Uri "http://127.0.0.1:$port/api/status" `
                 -Headers @{ Authorization = "Bearer $env:DANPLAY_TOKEN" } -TimeoutSec 3
            if ($r.StatusCode -eq 200) { $ready = $true; break }
        } catch { }
    }
    if (-not $ready) { Fail 'el nucleo no contesto en 30 segundos' }
    Write-Host '   contesta' -ForegroundColor Green
} finally {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
}

# --- el nucleo, junto a la app ----------------------------------------------
Step '4/5  copiar el nucleo junto a la app'
# una carpeta (onedir) entre los recursos de Tauri, core/ (ver packaging/core.spec)
Get-ChildItem desktop/src-tauri/core -Exclude LEEME.txt | Remove-Item -Recurse -Force
Copy-Item -Recurse -Force dist/core/danplay-core/* desktop/src-tauri/core/

# --- ffmpeg, fpcalc y QuickJS -----------------------------------------------
# Con version fija y su SHA-256, las mismas que el instalador de la CI
# (.github/workflows/windows.yml): antes se bajaba «la ultima» sin comprobar
# nada. QuickJS (qjs.exe, un .exe suelto) es el motor de JavaScript que
# YouTube exige a yt-dlp.
$downloads = @(
    @{ Url = 'https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip'
       Sha = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
       Exes = @('ffmpeg.exe') },
    @{ Url = 'https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-windows-x86_64.zip'
       Sha = '735d6182b38e9f364b84ce6f4ccd682c75e2851de89735711d6b762d12b92a4e'
       Exes = @('fpcalc.exe') },
    @{ Url = 'https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0/qjs-windows-x86_64.exe'
       Sha = '2aeabf0092c3262d6b2609824418f7dd7ed1f1df939f73b2b15645230cac0d77'
       Exes = @('qjs.exe') }
)
$tools = 'desktop/src-tauri/tools'
New-Item -ItemType Directory -Force -Path $tools | Out-Null
if (-not $NoTools -and -not (Test-Path "$tools/qjs.exe")) {
    Step '4b/5  ffmpeg, fpcalc y QuickJS (van dentro del instalador)'
    $tmp = Join-Path $env:TEMP "danplay-tools-$PID"
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        foreach ($item in $downloads) {
            $zip = Join-Path $tmp ([IO.Path]::GetFileName($item.Url))
            Invoke-WebRequest -Uri $item.Url -OutFile $zip
            $real = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
            if ($real -ne $item.Sha) { throw "$([IO.Path]::GetFileName($zip)) no es lo esperado: $real" }
            if ($zip -like '*.exe') {
                # un .exe suelto (QuickJS): no hay nada que descomprimir
                Copy-Item $zip (Join-Path $tools $item.Exes[0]) -Force
                continue
            }
            $dir = "$zip.d"
            Expand-Archive $zip -DestinationPath $dir
            Get-ChildItem -Recurse $dir -Include $item.Exes |
                ForEach-Object { Copy-Item $_.FullName $tools -Force }
        }
    } catch {
        Write-Host "   no se pudieron bajar; se sigue sin ellas: $_" -ForegroundColor Yellow
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

# --- la aplicacion -----------------------------------------------------------
Step '5/5  aplicacion de escritorio (Tauri)'
Push-Location desktop
if ($Installer) { npx tauri build --bundles nsis } else { cargo build --release -p danplay-app }
Pop-Location

Write-Host "`nLISTO" -ForegroundColor Green
Write-Host "  app     : target\release\danplay-app.exe"
Write-Host "  nucleo  : dist\core\danplay-core.exe"
if ($Installer) {
    $nsis = Get-ChildItem -Recurse target/release/bundle -Include *-setup.exe -ErrorAction SilentlyContinue
    foreach ($f in $nsis) { Write-Host "  instalador: $($f.FullName)" }
}
