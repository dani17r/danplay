# Construye DanPlay entero en Windows. Es el equivalente de scripts/build.sh.
#
#   .\scripts\build-windows.ps1                 interfaz + datos + app
#   .\scripts\build-windows.ps1 -Installer      ademas, el instalador (NSIS)
#   .\scripts\build-windows.ps1 -NoTools        sin ffmpeg, fpcalc, QuickJS ni yt-dlp dentro
#
# Sin Python: el nucleo va dentro de la app (el crate `server`). Lo que viaja
# con ella son sus datos (desktop/src-tauri/data: los modelos, las voces, la
# foto del catalogo y ONNX Runtime) y sus programas (desktop/src-tauri/tools).
#
# Lo que hace falta tener puesto:
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

foreach ($program in @('node', 'npm', 'cargo', 'rustc')) {
    if (-not (Get-Command $program -ErrorAction SilentlyContinue)) {
        Fail "Falta $program. Mira la cabecera de este archivo."
    }
}

# Baja `url` a la cache como `name`, si no esta ya, y comprueba su SHA-256.
# La cache, como la de scripts/tools-linux.sh: la segunda vez no se baja nada.
$cache = Join-Path $env:LOCALAPPDATA 'danplay-windows'
New-Item -ItemType Directory -Force -Path $cache | Out-Null
function Download($url, $sha, $name) {
    $file = Join-Path $cache $name
    if ((Test-Path $file) -and ((Get-FileHash $file -Algorithm SHA256).Hash.ToLower() -eq $sha)) { return $file }
    Write-Host "   bajando $name"
    Invoke-WebRequest -Uri $url -OutFile "$file.part"
    $real = (Get-FileHash "$file.part" -Algorithm SHA256).Hash.ToLower()
    if ($real -ne $sha) { Remove-Item "$file.part"; throw "$name no es lo esperado: $real" }
    Move-Item -Force "$file.part" $file
    return $file
}

# --- interfaz ----------------------------------------------------------------
Step '1/4  interfaz (Vite)'
Push-Location desktop
if (-not (Test-Path node_modules)) { npm ci }
npx vite build
Pop-Location

# --- datos: lo de danplay/data y ONNX Runtime --------------------------------
# Lo mismo que scripts/data.sh en Linux y macOS, con la misma version y suma:
# para subirla, se cambian aqui, alli y en .github/workflows/windows.yml.
Step '2/4  datos que viajan con la app (ONNX Runtime y danplay/data)'
$data = 'desktop/src-tauri/data'
Get-ChildItem $data -Exclude LEEME.txt | Remove-Item -Recurse -Force
$ort = Download 'https://github.com/microsoft/onnxruntime/releases/download/v1.31.0/onnxruntime-win-x64-1.31.0.zip' `
                '3958d8a44984160692c1b4ffd0de3cb48f8c39f9d34bb31e433db06e30b82195' 'onnxruntime-win-x64-1.31.0.zip'
# Del .zip, solo la biblioteca y su licencia: con sus simbolos son 450 MB
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($ort)
try {
    $wanted = @{ 'lib/onnxruntime.dll' = 'onnxruntime.dll'; 'LICENSE' = 'LICENSE-onnxruntime.txt'
                 'ThirdPartyNotices.txt' = 'ThirdPartyNotices-onnxruntime.txt' }
    foreach ($name in $wanted.Keys) {
        $entry = $zip.GetEntry("onnxruntime-win-x64-1.31.0/$name")
        if (-not $entry) { Fail "el .zip de ONNX Runtime no trae $name" }
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $root "$data/$($wanted[$name])"), $true)
    }
} finally {
    $zip.Dispose()
}
foreach ($item in 'pulse', 'separator', 'voices.json', 'models-snapshot.json') {
    Copy-Item -Recurse "danplay/data/$item" $data
}

# --- ffmpeg, fpcalc, QuickJS y yt-dlp ----------------------------------------
# Con version fija y su SHA-256, las mismas que el instalador de la CI
# (.github/workflows/windows.yml). QuickJS (qjs.exe) es el motor de JavaScript
# que YouTube exige a yt-dlp; yt-dlp, su ejecutable oficial.
$tools = 'desktop/src-tauri/tools'
New-Item -ItemType Directory -Force -Path $tools | Out-Null
if (-not $NoTools) {
    Step '3/4  ffmpeg, fpcalc, QuickJS y yt-dlp (van dentro del instalador)'
    $zips = @(
        @{ Url = 'https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip'
           Sha = '60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba'
           Name = 'ffmpeg-9.0.2.zip'; Exe = 'ffmpeg.exe' },
        @{ Url = 'https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-windows-x86_64.zip'
           Sha = '735d6182b38e9f364b84ce6f4ccd682c75e2851de89735711d6b762d12b92a4e'
           Name = 'fpcalc-1.6.1.zip'; Exe = 'fpcalc.exe' }
    )
    $exes = @(
        @{ Url = 'https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0/qjs-windows-x86_64.exe'
           Sha = '2aeabf0092c3262d6b2609824418f7dd7ed1f1df939f73b2b15645230cac0d77'
           Name = 'qjs-0.17.0.exe'; Exe = 'qjs.exe' },
        @{ Url = 'https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe'
           Sha = '66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a'
           Name = 'yt-dlp-2026.08.19.exe'; Exe = 'yt-dlp.exe' }
    )
    try {
        foreach ($item in $zips) {
            $zip = Download $item.Url $item.Sha $item.Name
            $dir = Join-Path $cache "$($item.Name).d"
            Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
            Expand-Archive $zip -DestinationPath $dir
            Get-ChildItem -Recurse $dir -Include $item.Exe |
                ForEach-Object { Copy-Item $_.FullName $tools -Force }
            Remove-Item -Recurse -Force $dir
        }
        foreach ($item in $exes) {
            # un .exe suelto: no hay nada que descomprimir
            Copy-Item (Download $item.Url $item.Sha $item.Name) (Join-Path $tools $item.Exe) -Force
        }
    } catch {
        Write-Host "   no se pudieron bajar; se sigue sin ellas: $_" -ForegroundColor Yellow
    }
}

# --- la aplicacion -----------------------------------------------------------
Step '4/4  aplicacion de escritorio (Tauri), con el nucleo dentro'
if ($Installer) {
    Push-Location desktop
    # sin empaquetar todavia: antes se saca con ella la foto del catalogo
    npx tauri build --no-bundle
    Pop-Location
    # La de hoy, para que el instalador no salga con una lista de hace meses
    # (sin red, viaja la del repositorio). Con -Wait: es una app de ventanas.
    $p = Start-Process -Wait -PassThru -NoNewWindow -FilePath target/release/danplay-app.exe `
         -ArgumentList '--models-snapshot', "$data/models-snapshot.json"
    if ($p.ExitCode -ne 0) { Write-Host '   sin red: viaja la foto del repositorio' -ForegroundColor Yellow }
    Push-Location desktop
    npx tauri bundle --bundles nsis
    Pop-Location
} else {
    cargo build --release -p danplay-app
}

Write-Host "`nLISTO" -ForegroundColor Green
Write-Host "  app     : target\release\danplay-app.exe"
if ($Installer) {
    $nsis = Get-ChildItem -Recurse target/release/bundle -Include *-setup.exe -ErrorAction SilentlyContinue
    foreach ($f in $nsis) { Write-Host "  instalador: $($f.FullName)" }
}
