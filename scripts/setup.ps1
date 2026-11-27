# Sets up Castable on Windows, then opens the setup wizard.
#
#   powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
#
# Installs what's missing with winget (Rust, Node LTS; VLC on request),
# builds the receiver, and runs `castable setup`. Safe to run again.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Say($text) { Write-Host "`n$text" -ForegroundColor White }
function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
function Ask($question) { $r = Read-Host "$question [Y/n]"; return ($r -eq "" -or $r -match "^[yY]") }
function Refresh-Path {
  $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [Environment]::GetEnvironmentVariable("Path", "User")
}
function Winget-Install($id) {
  if (-not (Have winget)) { throw "winget isn't available. Install $id by hand, then run this again." }
  winget install --id $id --exact --accept-source-agreements --accept-package-agreements
  Refresh-Path
}

Say "1/4 Rust"
if (-not (Have cargo)) {
  if (Ask "Rust isn't installed. Install it now (winget Rustlang.Rustup)?") {
    Winget-Install "Rustlang.Rustup"
    Write-Host "If rustup asks for the Visual Studio C++ Build Tools, let it install them."
  } else { throw "Install Rust from https://rustup.rs, then run this again." }
}
cargo --version

Say "2/4 Node and pnpm"
$nodeOk = (Have node) -and ([int](node -p "process.versions.node.split('.')[0]") -ge 22)
if (-not $nodeOk) {
  if (Ask "Node 22 or newer isn't installed. Install Node LTS now (winget OpenJS.NodeJS.LTS)?") {
    Winget-Install "OpenJS.NodeJS.LTS"
  } else { throw "Install Node 22+ from https://nodejs.org, then run this again." }
}
node --version
if (-not (Have pnpm)) { corepack enable; Refresh-Path }
pnpm install --silent

Say "3/4 A player (optional)"
$vlc = Test-Path "C:\Program Files\VideoLAN\VLC\vlc.exe"
if ((Have mpv) -or $vlc) {
  Write-Host "Found a player."
} elseif (Ask "For full control, install VLC now (winget VideoLAN.VLC)?") {
  Winget-Install "VideoLAN.VLC"
} else {
  Write-Host "You can also pick MPC-HC, PotPlayer or your browser in the wizard (mpv: https://mpv.io)."
}

Say "4/4 The receiver"
cargo build --release -p castable-players --bin castable-receiver

Say "Opening the setup wizard"
Write-Host "Windows may ask to allow castable-receiver on private networks: allow it, so phones can find it."
node packages\cli\bin\castable.mjs setup
