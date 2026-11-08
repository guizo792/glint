# Measures Glint's memory per process, optionally after a scripted workload, and saves a
# screenshot of the search window so variants can be compared visually.
#
# Runs an isolated instance (own data folder, own hotkey), so it works beside your real Glint.
# The workload part takes over the keyboard for about a minute: leave it alone, on an unlocked
# desktop. Without -Workload it needs no input at all.
#
#   scripts\memory.ps1 -Label baseline -Workload
#   scripts\memory.ps1 -Label gpu-off -ExtraArgs "--disable-gpu" -Workload
param(
    [string]$Label = "baseline",
    [string]$ExtraArgs = "",
    [string]$Exe = "$PSScriptRoot\..\target\next\release\glint.exe",
    [switch]$Workload,
    [string]$Out = "$env:TEMP\glint-memory"
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Win {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct LASTINPUT { public uint size; public uint time; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LASTINPUT i);
  public static uint IdleMs() { var i = new LASTINPUT(); i.size = 8; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.time; }
  public static string Title() { var s = new System.Text.StringBuilder(256); GetWindowText(GetForegroundWindow(), s, 256); return s.ToString(); }
  // Ctrl+Alt+Shift+F11, the hotkey this script gives the test instance.
  public static void KeepAlive() { keybd_event(0x7E, 0, 0, UIntPtr.Zero); keybd_event(0x7E, 0, 2, UIntPtr.Zero); }
  public static void Hotkey() {
    byte[] mods = {0x11, 0x12, 0x10};
    foreach (var m in mods) keybd_event(m, 0, 0, UIntPtr.Zero);
    keybd_event(0x7A, 0, 0, UIntPtr.Zero); keybd_event(0x7A, 0, 2, UIntPtr.Zero);
    foreach (var m in mods) keybd_event(m, 0, 2, UIntPtr.Zero);
  }
}
'@
[void][Win]::SetProcessDPIAware()
New-Item -ItemType Directory -Force $Out | Out-Null

$data = "$env:TEMP\glint-memory-$(Get-Random)"
New-Item -ItemType Directory -Force $data | Out-Null
Set-Content "$data\config.toml" 'hotkey = "Ctrl+Alt+Shift+F11"' -Encoding ascii
$env:GLINT_DATA_DIR = $data
$env:GLINT_TRACE = "1"
if ($ExtraArgs) { $env:GLINT_WEBVIEW_ARGS = $ExtraArgs }
$proc = Start-Process $Exe -PassThru
Remove-Item Env:GLINT_DATA_DIR, Env:GLINT_TRACE, Env:GLINT_WEBVIEW_ARGS -ErrorAction SilentlyContinue

# Sleep in short steps, tapping F15 so the session does not lock from idleness mid-run.
function Wait-Alive($seconds) {
    for ($t = 0; $t -lt $seconds; $t += 5) { [Win]::KeepAlive(); Start-Sleep -Seconds ([math]::Min(5, $seconds - $t)) }
}
function Test-Locked { [bool](Get-Process LogonUI -ErrorAction SilentlyContinue) }

function Snapshot($stage) {
    $all = Get-CimInstance Win32_Process
    $ids = @($proc.Id); $queue = @($proc.Id)
    while ($queue.Count) {
        $c = $queue[0]; $queue = @($queue | Select-Object -Skip 1)
        foreach ($k in ($all | Where-Object ParentProcessId -eq $c).ProcessId) { $ids += $k; $queue += $k }
    }
    $perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $ids -contains [int]$_.IDProcess }
    $roles = @{}
    foreach ($id in $ids) {
        $p = $all | Where-Object ProcessId -eq $id
        $pf = $perf | Where-Object { [int]$_.IDProcess -eq $id }
        if (-not $pf) { continue }
        $role = if ($p.Name -eq 'glint.exe') { 'glint' } elseif ($p.CommandLine -match '--type=([\w-]+)') { $Matches[1] } else { 'browser' }
        if ($p.CommandLine -match '--utility-sub-type=network') { $role = 'network' } elseif ($p.CommandLine -match '--utility-sub-type=storage') { $role = 'storage' }
        $roles[$role] = [math]::Round(($roles[$role] + $pf.WorkingSetPrivate / 1MB), 1)
    }
    [pscustomobject]@{
        Stage = $stage; Processes = @($perf).Count
        PrivateMB = [math]::Round((($perf | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB), 1)
        TotalWSMB = [math]::Round((($perf | Measure-Object WorkingSet -Sum).Sum / 1MB), 1)
        Roles = ($roles.GetEnumerator() | Sort-Object Value -Descending | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' '
    }
}

$result = [ordered]@{ label = $Label; args = $ExtraArgs }
$notes = @()
try {
    Start-Sleep -Seconds 12
    if ($proc.HasExited) {
        $why = if (Test-Path "$data\logs\glint.log") { (Get-Content "$data\logs\glint.log" -Tail 3) -join " | " } else { "no log" }
        throw "Glint exited during startup (code $($proc.ExitCode)): $why"
    }
    # A new data folder means a full index build at startup, which inflates our own memory for
    # a while. Measure only once it has finished, then let things settle.
    $log0 = "$data\logs\glint.log"
    for ($i = 0; $i -lt 150; $i++) {
        if ((Test-Path $log0) -and (Select-String -Path $log0 -Pattern "Up to date" -Quiet)) { break }
        if ($proc.HasExited) { throw "Glint exited while indexing" }
        if ($i % 5 -eq 0) { [Win]::KeepAlive() }
        Start-Sleep -Seconds 1
    }
    Wait-Alive 12
    $result.fresh = Snapshot "fresh idle (index built)"

    if ($Workload) {
        $locked = [bool](Get-Process LogonUI -ErrorAction SilentlyContinue)
        # The caller is expected to have checked the desktop is idle. Keys are only typed while
        # the test window has focus, so a user returning mid-run is not typed into.
        if ($locked) {
            $notes += "workload SKIPPED: desktop locked"
        } else {
            for ($i = 1; $i -le 20; $i++) {
                if (Test-Locked) { $notes += "stopped at round $i`: the desktop locked"; break }
                [Win]::Hotkey(); Start-Sleep -Milliseconds 450
                if ([Win]::Title() -ne "Glint") { $notes += "round $i`: Glint did not take focus"; continue }
                try {
                    foreach ($c in "report".ToCharArray()) { [System.Windows.Forms.SendKeys]::SendWait("$c"); Start-Sleep -Milliseconds 90 }
                } catch { $notes += "stopped at round $i`: keystrokes were refused ($($_.Exception.Message))"; break }
                Start-Sleep -Milliseconds 500
                if ($i -eq 10) {
                    $r = New-Object Win+RECT
                    [void][Win]::GetWindowRect([Win]::GetForegroundWindow(), [ref]$r)
                    $w = $r.R - $r.L; $h = $r.B - $r.T
                    if ($w -gt 50 -and $h -gt 20) {
                        $bmp = New-Object System.Drawing.Bitmap $w, $h
                        $g = [System.Drawing.Graphics]::FromImage($bmp)
                        $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $h))
                        $bmp.Save("$Out\$Label.png"); $g.Dispose(); $bmp.Dispose()
                        $result.screenshot = "$Out\$Label.png"
                    }
                }
                try { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") } catch { $notes += "stopped at round $i`: keystrokes were refused"; break }
                Start-Sleep -Milliseconds 350
            }
            Wait-Alive 15
            $result.after_use = Snapshot "after use, hidden"
            $log = "$data\logs\glint.log"
            foreach ($metric in "show", "search") {
                if (-not (Test-Path $log)) { continue }
                $v = @(Select-String -Path $log -Pattern "trace $metric ([\d.]+)" |
                    ForEach-Object { [double]$_.Matches[0].Groups[1].Value } | Sort-Object)
                if ($v.Count) {
                    $result[$metric] = "n=$($v.Count) p50=$($v[[int](($v.Count-1)*0.5)]) p95=$($v[[int](($v.Count-1)*0.95)]) ms"
                }
            }
        }
    }
    $logFile = "$data\logs\glint.log"
    $problems = @()
    if (Test-Path $logFile) {
        $problems = @(Select-String -Path $logFile -Pattern "panic|error|failed" | Select-Object -First 3 | ForEach-Object { $_.Line })
    }
    if ($problems.Count) { $notes += "log problems: " + ($problems -join " | ") }
    $result.alive = -not $proc.HasExited
} finally {
    # Keep the evidence if the app went away on its own: a panic restarts it, which looks like
    # an exit, and the log would otherwise be deleted with the temporary data folder.
    $result.exited_early = $proc.HasExited
    if ($proc.HasExited) { $result.exit_code = $proc.ExitCode }
    if (Test-Path "$data\logs\glint.log") { Copy-Item "$data\logs\glint.log" "$Out\$Label.log" -Force }
    if (-not $proc.HasExited) { Stop-Process $proc -Force }
    Start-Sleep -Milliseconds 800
    Remove-Item $data -Recurse -Force -ErrorAction SilentlyContinue
}
$result.notes = $notes
$result | ConvertTo-Json -Depth 4 | Set-Content "$Out\$Label.json" -Encoding utf8
$result | ConvertTo-Json -Depth 4
