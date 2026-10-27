param(
    [Parameter(Mandatory)][string]$InstallerPath,
    [Parameter(Mandatory)][string]$ReportPath,
    [switch]$DisposableEnvironment
)

$ErrorActionPreference = 'Stop'
# Bu test yalnız atılabilir Windows ortamında çalışır; mevcut kuruluma dokunmaz.
if ($env:GITHUB_ACTIONS -ne 'true' -and -not $DisposableEnvironment) {
    throw 'Run this test in a disposable Windows VM or GitHub-hosted runner.'
}
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "lumiadam-install-test-$([guid]::NewGuid())"
$installDir = Join-Path $testRoot 'installed'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\LumiaDAM'
$appData = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'LumiaDAM'
$oldAppData = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'com.opendam.desktop'
$report = [ordered]@{
    revision = $env:GITHUB_SHA
    installer = [System.IO.Path]::GetFileName($installer)
    sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
    os = [Environment]::OSVersion.VersionString
    checks = @()
    status = 'running'
    limitation = 'Isolated GitHub Windows runner smoke; not a pristine Windows VM, DCC or physical NAS certification.'
}
$app = $null
function Assert-Check([bool]$Condition, [string]$Name) {
    if (-not $Condition) { throw $Name }
    $report.checks += $Name
    Write-Output "PASS: $Name"
}
try {
    Assert-Check (-not (Test-Path -LiteralPath $uninstallKey)) 'No existing LumiaDAM registration'
    Assert-Check (-not (Test-Path -LiteralPath 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\LumiaDAM')) 'No existing machine installation'
    Assert-Check (-not (Test-Path -LiteralPath $appData)) 'No existing LumiaDAM user data'
    Assert-Check (-not (Test-Path -LiteralPath $oldAppData)) 'No existing OpenDAM user data'
    Assert-Check (-not (Get-Process -Name 'lumiadam-desktop','lumiadam','opendam-desktop','opendam' -ErrorAction SilentlyContinue)) 'No existing application processes'
    New-Item -ItemType Directory -Path $testRoot, $oldAppData -Force | Out-Null
    $legacySentinel = Join-Path $oldAppData 'preserve-test.txt'
    Set-Content -LiteralPath $legacySentinel -Value 'OpenDAM sentinel'
    $archive = Join-Path $testRoot 'sample-library'
    New-Item -ItemType Directory -Path $archive | Out-Null
    Set-Content -LiteralPath (Join-Path $archive 'source.txt') -Value 'Source archive sentinel'
    $exe = Join-Path $installDir 'lumiadam-desktop.exe'
    for ($round = 1; $round -le 2; $round++) {
        $setup = Start-Process -FilePath $installer -ArgumentList "/S /D=$installDir" -WindowStyle Hidden -PassThru
        Assert-Check ($setup.WaitForExit(180000)) "Installer round $round finished"
        Assert-Check ($setup.ExitCode -eq 0) "Installer round $round returned zero"
        Assert-Check (Test-Path -LiteralPath $exe) "Executable exists after round $round"
        $registration = Get-ItemProperty -LiteralPath $uninstallKey
        Assert-Check ($registration.DisplayName -eq 'LumiaDAM') 'Correct Add/Remove Programs identity'
        Assert-Check ($registration.Publisher -eq 'yuxelmurat') 'Correct publisher'
        $installedHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
        if ($round -eq 1) { $report.executableSha256 = $installedHash }
        else { Assert-Check ($installedHash -eq $report.executableSha256) 'Reinstall preserves the expected executable' }
        Assert-Check (Test-Path -LiteralPath (Join-Path $installDir 'preview-engine/worker.py')) 'Preview worker is packaged'
        Assert-Check (Test-Path -LiteralPath (Join-Path $installDir 'preview-engine/requirements.txt')) 'Preview prerequisites are packaged'
        $start = [System.Diagnostics.ProcessStartInfo]::new($exe)
        $start.UseShellExecute = $false
        $start.RedirectStandardError = $true
        $start.RedirectStandardOutput = $true
        $start.WorkingDirectory = $testRoot
        $start.WindowStyle = 'Normal'
        $start.Environment['PATH'] = "$env:WINDIR\System32;$env:WINDIR"
        $start.Environment['OPENDAM_PYTHON'] = Join-Path $testRoot 'python-not-installed.exe'
        $start.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = '--disable-gpu'
        $start.Environment.Remove('OPENDAM_WORKER') | Out-Null
        $app = [System.Diagnostics.Process]::Start($start)
        $appOutput = $app.StandardOutput.ReadToEndAsync()
        $appError = $app.StandardError.ReadToEndAsync()
        $catalog = Join-Path $appData 'data/opendam.sqlite'
        $deadline = [DateTime]::UtcNow.AddSeconds(45)
        do {
            Start-Sleep -Milliseconds 500
            $app.Refresh()
        } while (-not $app.HasExited -and ($app.MainWindowHandle -eq 0 -or -not (Test-Path -LiteralPath $catalog)) -and [DateTime]::UtcNow -lt $deadline)
        $report.applicationWindowTitle = $app.MainWindowTitle
        $report.catalogPath = $catalog
        $report.applicationExited = $app.HasExited
        Assert-Check (-not $app.HasExited -and $app.MainWindowHandle -ne 0) "Application window opened without developer PATH, round $round"
        Assert-Check ($app.MainWindowTitle -eq 'LumiaDAM') 'Correct application window title'
        Assert-Check (Test-Path -LiteralPath $catalog) 'Local catalog initialized'
        $app.CloseMainWindow() | Out-Null
        Assert-Check ($app.WaitForExit(20000)) 'Application closed normally'
        $report.applicationOutput = $appOutput.GetAwaiter().GetResult()
        $report.applicationError = $appError.GetAwaiter().GetResult()
        $app = $null
    }
    $sentinel = Join-Path $appData 'data/preserve-test.txt'
    Set-Content -LiteralPath $sentinel -Value 'LumiaDAM catalog sentinel'
    $uninstaller = Join-Path $installDir 'uninstall.exe'
    $uninstall = Start-Process -FilePath $uninstaller -ArgumentList "/S _?=$installDir" -WindowStyle Hidden -PassThru
    Assert-Check ($uninstall.WaitForExit(120000)) 'Uninstaller finished'
    Assert-Check ($uninstall.ExitCode -eq 0) 'Uninstaller returned zero'
    Assert-Check (-not (Test-Path -LiteralPath $exe)) 'Installed executable removed'
    Assert-Check (-not (Test-Path -LiteralPath $uninstallKey)) 'Uninstall registration removed'
    Assert-Check (Test-Path -LiteralPath $sentinel) 'LumiaDAM user data preserved'
    Assert-Check (Test-Path -LiteralPath $legacySentinel) 'OpenDAM user data preserved'
    Assert-Check (Test-Path -LiteralPath (Join-Path $archive 'source.txt')) 'Source archive preserved'
    $report.status = 'passed'
} catch {
    $report.status = 'failed'
    $report.error = $_.Exception.Message
    throw
} finally {
    if ($app -and -not $app.HasExited) { $app.Kill($true); $app.WaitForExit() }
    if ($appOutput) { $report.applicationOutput = $appOutput.GetAwaiter().GetResult() }
    if ($appError) { $report.applicationError = $appError.GetAwaiter().GetResult() }
    $parent = Split-Path -Parent ([System.IO.Path]::GetFullPath($ReportPath))
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $ReportPath -Encoding utf8
}
