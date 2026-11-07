# End-to-end test on the real desktop: hotkey, type, Enter, and the chosen file really opens.
# Uses an isolated data folder, so it does not touch real settings or history.
# Takes over the keyboard for a moment. Do not type while it runs.
param(
    [string]$Exe = "$PSScriptRoot\..\target\release\glint.exe",
    [string]$Shots = "$env:TEMP\glint-e2e-shots"
)
$ErrorActionPreference = "Stop"
if (Get-Process LogonUI -ErrorAction SilentlyContinue) {
    "SKIPPED: the desktop is locked, so nothing can receive a hotkey. Unlock and run again."
    exit 2
}
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Keys2 {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct LASTINPUT { public uint size; public uint time; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LASTINPUT i);
  public static uint IdleMs() { var i = new LASTINPUT(); i.size = 8; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.time; }
  public static string ForegroundTitle() { var s = new System.Text.StringBuilder(256); GetWindowText(GetForegroundWindow(), s, 256); return s.ToString(); }
  public static void AltSpace() {
    keybd_event(0x12,0,0,UIntPtr.Zero); keybd_event(0x20,0,0,UIntPtr.Zero);
    keybd_event(0x20,0,2,UIntPtr.Zero); keybd_event(0x12,0,2,UIntPtr.Zero);
  }
}
'@

if ([Keys2]::IdleMs() -lt 20000) {
    "SKIPPED: the keyboard or mouse was used in the last 20 seconds. Leave them alone and run again."
    exit 3
}

function Shot($name) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save("$Shots\$name.png")
    $g.Dispose(); $bmp.Dispose()
}
# Types only into Glint, so a failed hotkey cannot send keystrokes to another app.
function Type-Text($text) {
    $title = [Keys2]::ForegroundTitle()
    if ($title -ne "Glint") { throw "Glint does not have focus (foreground is '$title'); not typing." }
    [System.Windows.Forms.SendKeys]::SendWait($text)
}

$id = Get-Random -Maximum 99999999
$base = "$env:TEMP\glint-e2e-$id"
$data = "$base\data"
$files = "$base\files"
New-Item -ItemType Directory -Force $data, $files, $Shots | Out-Null
Set-Content "$data\config.toml" 'hotkey = "Alt+Space"' -Encoding ascii

$failed = @()
function Check($name, $ok) {
    if ($ok) { "PASS  $name" } else { "FAIL  $name"; $script:failed += $name }
}

$env:GLINT_DATA_DIR = $data
$env:GLINT_TRACE = "1"
$proc = Start-Process $Exe -PassThru
try {
    Start-Sleep -Seconds 6
    Check "app stays running" (-not $proc.HasExited)

    # A file created after startup must be found through the watcher.
    $name = "glinttestfixture$id"
    $marker = "$base\opened.txt"
    Set-Content "$files\$name.cmd" "@echo opened> `"$marker`"`r`n@exit" -Encoding ascii
    Start-Sleep -Seconds 2

    [Keys2]::AltSpace()
    Start-Sleep -Milliseconds 400
    Shot "1-open"
    Type-Text $name
    Start-Sleep -Milliseconds 900
    Shot "2-results"
    Type-Text "{ENTER}"
    for ($i = 0; $i -lt 60 -and -not (Test-Path $marker); $i++) { Start-Sleep -Milliseconds 100 }
    Check "hotkey, type, Enter opens the new file" (Test-Path $marker)
    Start-Sleep -Milliseconds 300
    Shot "3-after"

    # Calculator: the answer is copied to the clipboard.
    [Keys2]::AltSpace()
    Start-Sleep -Milliseconds 300
    Type-Text "12{*}12"
    Start-Sleep -Milliseconds 500
    Shot "4-calc"
    Type-Text "{ENTER}"
    Start-Sleep -Milliseconds 400
    Check "calculator copies its answer" ((Get-Clipboard) -eq "144")

    # Choosing a result again from memory: it now appears for the empty query.
    [Keys2]::AltSpace()
    Start-Sleep -Milliseconds 500
    Shot "5-home"
    Type-Text "{ESC}"
    Start-Sleep -Milliseconds 200

    Check "history saved" (Test-Path "$data\history.json")

    # Settings opens without hanging (window creation can deadlock if done wrongly).
    [Keys2]::AltSpace()
    Start-Sleep -Milliseconds 300
    Type-Text "^,"
    $opened = $false
    for ($i = 0; $i -lt 50 -and -not $opened; $i++) {
        Start-Sleep -Milliseconds 100
        $opened = [Keys2]::ForegroundTitle() -eq "Glint Settings"
    }
    Check "settings window opens" $opened
    Start-Sleep -Milliseconds 800
    Shot "6-settings"
    if ($opened) { [System.Windows.Forms.SendKeys]::SendWait("%{F4}") }
    Start-Sleep -Milliseconds 500
    Check "app survives closing settings" (-not $proc.HasExited)

    # Show latency over repeated opens, and search latency while typing.
    for ($i = 0; $i -lt 30; $i++) {
        [Keys2]::AltSpace()
        Start-Sleep -Milliseconds 350
        if ($i % 3 -eq 0) {
            # Typed at a human pace: one letter every 90 ms.
            foreach ($c in "report".ToCharArray()) { Type-Text "$c"; Start-Sleep -Milliseconds 90 }
            Start-Sleep -Milliseconds 150
        }
        [Keys2]::AltSpace()
        Start-Sleep -Milliseconds 300
    }
    Check "focus was on Glint for every keystroke" $true
    foreach ($metric in "show", "search") {
        $values = Select-String -Path "$data\logs\glint.log" -Pattern "trace $metric ([\d.]+)" |
            ForEach-Object { [double]$_.Matches[0].Groups[1].Value } | Sort-Object
        if ($values) {
            $p95 = $values[[int](($values.Count - 1) * 0.95)]
            $p50 = $values[[int](($values.Count - 1) * 0.5)]
            "$metric n=$($values.Count) p50=$p50 ms p95=$p95 ms"
        }
    }
    Check "app still running at the end" (-not $proc.HasExited)
} finally {
    if (-not $proc.HasExited) { Stop-Process $proc -Force }
    Start-Sleep -Milliseconds 500
    Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Out-Null
    Remove-Item Env:GLINT_DATA_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:GLINT_TRACE -ErrorAction SilentlyContinue
    Remove-Item $base -Recurse -Force -ErrorAction SilentlyContinue
}
if ($failed.Count) { "FAILED: $($failed -join ', ')"; exit 1 }
"ALL PASSED. Screenshots: $Shots"
