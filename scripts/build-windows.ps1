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

# --- ffmpeg, fpcalc y Deno --------------------------------------------------
# Con version fija y su SHA-256, las mismas que el instalador de la CI
# (.github/workflows/windows.yml): antes se bajaba «la ultima» sin comprobar
# nada. Deno es el motor de JavaScript que YouTube exige a yt-dlp.
$herramientas = @(
    @{ Url = 'https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip'
       Sha = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
       Exes = @('ffmpeg.exe', 'ffprobe.exe') },
    @{ Url = 'https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-windows-x86_64.zip'
       Sha = '735d6182b38e9f364b84ce6f4ccd682c75e2851de89735711d6b762d12b92a4e'
       Exes = @('fpcalc.exe') },
    @{ Url = 'https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-pc-windows-msvc.zip'
       Sha = 'a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238'
       Exes = @('deno.exe') }
)
$tools = 'desktop/src-tauri/tools'
New-Item -ItemType Directory -Force -Path $tools | Out-Null
if (-not $SinHerramientas -and -not (Test-Path "$tools/deno.exe")) {
    Paso '4b/5  ffmpeg, fpcalc y Deno (van dentro del instalador)'
    $tmp = Join-Path $env:TEMP "danplay-tools-$PID"
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        foreach ($h in $herramientas) {
            $zip = Join-Path $tmp ([IO.Path]::GetFileName($h.Url))
            Invoke-WebRequest -Uri $h.Url -OutFile $zip
            $real = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
            if ($real -ne $h.Sha) { throw "$([IO.Path]::GetFileName($zip)) no es lo esperado: $real" }
            $dir = "$zip.d"
            Expand-Archive $zip -DestinationPath $dir
            Get-ChildItem -Recurse $dir -Include $h.Exes |
                ForEach-Object { Copy-Item $_.FullName $tools -Force }
        }
    } catch {
        Write-Host "   no se pudieron bajar; se sigue sin ellas: $_" -ForegroundColor Yellow
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

# --- la aplicacion -----------------------------------------------------------
Paso '5/5  aplicacion de escritorio (Tauri)'
Push-Location desktop
if ($Instalador) { npx tauri build --bundles nsis } else { cargo build --release -p danplay-app }
Pop-Location

Write-Host "`nLISTO" -ForegroundColor Green
Write-Host "  app     : target\release\danplay-app.exe"
Write-Host "  nucleo  : dist\core\danplay-core.exe"
if ($Instalador) {
    $nsis = Get-ChildItem -Recurse target/release/bundle -Include *-setup.exe -ErrorAction SilentlyContinue
    foreach ($f in $nsis) { Write-Host "  instalador: $($f.FullName)" }
}
