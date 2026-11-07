# Starts Glint with an isolated data folder, waits for the index to build, and reports memory.
# Works on a locked desktop, because it needs no input.
param(
    [string]$Exe = "$PSScriptRoot\..\target\release\glint.exe",
    [int]$TimeoutSeconds = 150
)
$data = "$env:TEMP\glint-measure-$(Get-Random)"
New-Item -ItemType Directory -Force $data | Out-Null
Set-Content "$data\config.toml" 'hotkey = "Ctrl+Alt+Shift+F11"' -Encoding ascii
$env:GLINT_DATA_DIR = $data
$start = Get-Date
$proc = Start-Process $Exe -PassThru
Remove-Item Env:GLINT_DATA_DIR

function Tree($rootId) {
    $all = Get-CimInstance Win32_Process
    $ids = @($rootId); $queue = @($rootId)
    while ($queue.Count) {
        $current = $queue[0]; $queue = @($queue | Select-Object -Skip 1)
        foreach ($kid in ($all | Where-Object ParentProcessId -eq $current).ProcessId) { $ids += $kid; $queue += $kid }
    }
    $ids
}
function Report($label) {
    $ids = Tree $proc.Id
    $perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $ids -contains [int]$_.IDProcess }
    $glint = $perf | Where-Object { [int]$_.IDProcess -eq $proc.Id }
    "{0}: tree private {1:N0} MB, total {2:N0} MB over {3} processes; glint.exe private {4:N1} MB, total {5:N1} MB" -f $label,
        (($perf | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB), (($perf | Measure-Object WorkingSet -Sum).Sum / 1MB),
        $perf.Count, ($glint.WorkingSetPrivate / 1MB), ($glint.WorkingSet / 1MB)
}

Start-Sleep -Seconds 8
Report "after startup"
$log = "$data\logs\glint.log"
$built = $false
for ($i = 0; $i -lt $TimeoutSeconds; $i++) {
    if ((Test-Path $log) -and (Select-String -Path $log -Pattern "Up to date" -Quiet)) { $built = $true; break }
    if ($proc.HasExited) { break }
    Start-Sleep -Seconds 1
}
"index built: $built after {0:N0} s; process alive: {1}" -f ((Get-Date) - $start).TotalSeconds, (-not $proc.HasExited)
Start-Sleep -Seconds 10
Report "idle after index"
if (Test-Path "$data\index\index.glint") { "snapshot: {0:N1} MB" -f ((Get-Item "$data\index\index.glint").Length / 1MB) }
if (Test-Path $log) { "--- log"; Get-Content $log }
if (-not $proc.HasExited) { Stop-Process $proc -Force }
Start-Sleep -Milliseconds 500
Remove-Item $data -Recurse -Force -ErrorAction SilentlyContinue
