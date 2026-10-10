# Установка Radar на Windows 10/11 (x64).
#
#   irm https://raw.githubusercontent.com/off-art/radar/main/install.ps1 | iex
#
# Скачивает radar.exe из GitHub Releases в %LOCALAPPDATA%\radar и добавляет папку в PATH пользователя.
# Права администратора не нужны. Переменные: RADAR_VERSION (например v0.7.0-win.1; по умолчанию — последний
# релиз), RADAR_REPO (по умолчанию off-art/radar), RADAR_BIN_DIR (папка установки).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # без индикатора загрузка в Windows PowerShell 5.1 в разы быстрее
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Say($m) { Write-Host "==> $m" -ForegroundColor Cyan }
function Die($m) { Write-Host "Ошибка: $m" -ForegroundColor Red; throw $m }

if (-not [Environment]::Is64BitOperatingSystem) { Die 'нужна 64-разрядная Windows' }

$repo = if ($env:RADAR_REPO) { $env:RADAR_REPO } else { 'off-art/radar' }
$dir = if ($env:RADAR_BIN_DIR) { $env:RADAR_BIN_DIR } else { Join-Path $env:LOCALAPPDATA 'radar' }
$asset = 'radar-x86_64-pc-windows-msvc.zip'
$url = if ($env:RADAR_VERSION) {
  "https://github.com/$repo/releases/download/$($env:RADAR_VERSION)/$asset"
} else {
  "https://github.com/$repo/releases/latest/download/$asset"
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("radar-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  Say "Скачиваю $url"
  try { Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $asset) -UseBasicParsing }
  catch { Die "не удалось скачать архив. Для Windows нужен релиз со сборкой $asset; пробный выпуск: `$env:RADAR_VERSION='v0.7.0-win.1'" }
  Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force
  $new = Join-Path $tmp 'radar.exe'
  if (-not (Test-Path $new)) { Die 'в архиве нет radar.exe' }

  New-Item -ItemType Directory -Path $dir -Force | Out-Null
  $target = Join-Path $dir 'radar.exe'
  # Работающий radar.exe перезаписать нельзя, но можно переименовать: старый файл уберём при следующем запуске.
  if (Test-Path $target) {
    $old = "$target.old"
    Remove-Item $old -Force -ErrorAction SilentlyContinue
    try { Move-Item $target $old -Force } catch { Die 'radar.exe занят: закройте все окна Radar и повторите' }
  }
  Copy-Item $new $target -Force
  Unblock-File $target -ErrorAction SilentlyContinue
  Remove-Item "$target.old" -Force -ErrorAction SilentlyContinue
} finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$parts = if ($userPath) { $userPath.Split(';') | Where-Object { $_ } } else { @() }
if ($parts -notcontains $dir) {
  [Environment]::SetEnvironmentVariable('Path', (($parts + $dir) -join ';'), 'User')
  $env:Path = "$env:Path;$dir"
  Say "Папка $dir добавлена в PATH (в уже открытых окнах PowerShell откроется после перезапуска)"
}

Say 'Готово:'
& $target --version
Write-Host ''
Write-Host 'Дальше:  radar doctor   — какие агенты найдены;  radar   — запуск (лучше в Windows Terminal).'
