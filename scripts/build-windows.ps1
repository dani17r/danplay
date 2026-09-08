# Construye DanPlay entero en Windows. Es el equivalente de scripts/build.sh.
#
#   .\scripts\build-windows.ps1                 interfaz + nucleo + app
#   .\scripts\build-windows.ps1 -Instalador     ademas, el instalador (NSIS)
#   .\scripts\build-windows.ps1 -SinHerramientas   sin ffmpeg ni fpcalc dentro
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
    [switch]$Instalador,
    [switch]$SinHerramientas
)

$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
Set-Location $raiz

function Paso($texto) { Write-Host "`n== $texto" -ForegroundColor Cyan }
function Fallo($texto) { Write-Host "`n$texto" -ForegroundColor Red; exit 1 }

foreach ($programa in @('python', 'node', 'npm', 'cargo', 'rustc')) {
    if (-not (Get-Command $programa -ErrorAction SilentlyContinue)) {
        Fallo "Falta $programa. Mira la cabecera de este archivo."
    }
}

# --- entorno de Python -------------------------------------------------------
if (-not (Test-Path .venv)) {
    Paso '0/5  entorno de Python'
    python -m venv .venv
}
$py = Join-Path $raiz '.venv\Scripts\python.exe'
& $py -m pip install --upgrade pip --quiet
& $py -m pip install -r requirements.txt pyinstaller --quiet

# --- interfaz ----------------------------------------------------------------
Paso '1/5  interfaz (Vite)'
Push-Location desktop
if (-not (Test-Path node_modules)) { npm ci }
npx vite build
Pop-Location

# --- nucleo ------------------------------------------------------------------
Paso '2/5  nucleo empaquetado (PyInstaller)'
$trabajo = Join-Path $env:TEMP "danplay-build-$PID"
& $py -m PyInstaller --noconfirm --clean --workpath $trabajo --distpath dist/core packaging/core.spec
Remove-Item -Recurse -Force $trabajo -ErrorAction SilentlyContinue
if (-not (Test-Path dist/core/danplay-core.exe)) { Fallo 'no se genero dist/core/danplay-core.exe' }

# Que arranque de verdad antes de meterlo en un instalador: un núcleo que no
# levanta solo se descubre al abrir la aplicacion ya instalada.
Paso '3/5  comprobar que el nucleo arranca'
$puerto = 8731
$env:DANPLAY_TOKEN = [guid]::NewGuid().ToString('N')
$proceso = Start-Process -PassThru -WindowStyle Hidden -FilePath dist/core/danplay-core.exe `
           -ArgumentList '--host', '127.0.0.1', '--port', "$puerto"
try {
    $bien = $false
    foreach ($intento in 1..30) {
        Start-Sleep -Seconds 1
        try {
            $r = Invoke-WebRequest -Uri "http://127.0.0.1:$puerto/api/status" `
                 -Headers @{ Authorization = "Bearer $env:DANPLAY_TOKEN" } -TimeoutSec 3
            if ($r.StatusCode -eq 200) { $bien = $true; break }
        } catch { }
    }
    if (-not $bien) { Fallo 'el nucleo no contesto en 30 segundos' }
    Write-Host '   contesta' -ForegroundColor Green
} finally {
    Stop-Process -Id $proceso.Id -Force -ErrorAction SilentlyContinue
}

# --- el nucleo, junto a la app ----------------------------------------------
Paso '4/5  copiar el nucleo junto a la app'
$triple = ((rustc -vV | Select-String '^host:') -split ' ')[1]
New-Item -ItemType Directory -Force -Path desktop/src-tauri/binaries | Out-Null
Copy-Item dist/core/danplay-core.exe "desktop/src-tauri/binaries/danplay-core-$triple.exe" -Force

# --- ffmpeg y fpcalc ---------------------------------------------------------
$tools = 'desktop/src-tauri/tools'
New-Item -ItemType Directory -Force -Path $tools | Out-Null
if (-not $SinHerramientas -and -not (Test-Path "$tools/ffmpeg.exe")) {
    Paso '4b/5  ffmpeg y fpcalc (van dentro del instalador)'
    $tmp = Join-Path $env:TEMP "danplay-tools-$PID"
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        Invoke-WebRequest -Uri 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip' `
                          -OutFile "$tmp/ffmpeg.zip"
        Expand-Archive "$tmp/ffmpeg.zip" -DestinationPath "$tmp/ffmpeg"
        Get-ChildItem -Recurse "$tmp/ffmpeg" -Include ffmpeg.exe, ffprobe.exe |
            ForEach-Object { Copy-Item $_.FullName $tools -Force }
        Invoke-WebRequest -Uri 'https://github.com/acoustid/chromaprint/releases/download/v1.5.1/chromaprint-fpcalc-1.5.1-windows-x86_64.zip' `
                          -OutFile "$tmp/fpcalc.zip"
        Expand-Archive "$tmp/fpcalc.zip" -DestinationPath "$tmp/fpcalc"
        Get-ChildItem -Recurse "$tmp/fpcalc" -Include fpcalc.exe |
            ForEach-Object { Copy-Item $_.FullName $tools -Force }
    } catch {
        Write-Host "   no se pudieron bajar; se sigue sin ellas: $_" -ForegroundColor Yellow
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

# --- la aplicacion -----------------------------------------------------------
Paso '5/5  aplicacion de escritorio (Tauri)'
Push-Location desktop
if ($Instalador) { npx tauri build --bundles nsis } else { cargo build --release --manifest-path src-tauri/Cargo.toml }
Pop-Location

Write-Host "`nLISTO" -ForegroundColor Green
Write-Host "  app     : desktop\src-tauri\target\release\danplay-app.exe"
Write-Host "  nucleo  : dist\core\danplay-core.exe"
if ($Instalador) {
    $nsis = Get-ChildItem -Recurse desktop/src-tauri/target/release/bundle -Include *-setup.exe -ErrorAction SilentlyContinue
    foreach ($f in $nsis) { Write-Host "  instalador: $($f.FullName)" }
}
