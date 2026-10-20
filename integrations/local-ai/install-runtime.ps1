# Yalnız LumiaDAM'in özel CLI dosyalarını kurar; model, servis veya süreç başlatmaz.
[CmdletBinding()]
param(
    [ValidateSet('keep', 'bundled', 'ollama', 'lmstudio')]
    [string]$Provider = 'keep',
    [ValidateSet('keep', 'disabled', 'manual')]
    [string]$Mode = 'keep',
    [ValidateSet('en', 'tr')]
    [string]$Language,
    [switch]$ConfigureOnly,
    [switch]$LanguageOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Net.Http

function Get-SetupText([string]$English, [string]$Turkish) {
    if ($Language -eq 'tr') { return $Turkish }
    return $English
}

# Resmî sabit yayın ve GitHub Release asset SHA256 özeti (2026-10-15 doğrulandı):
# https://api.github.com/repos/ollama/ollama/releases/tags/v0.40.4
# Standalone paket: https://docs.ollama.com/windows#standalone-cli
$script:ArchiveUri = 'https://github.com/ollama/ollama/releases/download/v0.40.4/ollama-windows-amd64.zip'
$script:ArchiveSha256 = '7485a1172f8527db4342a63521b9342fc466185ff0855da378cc864ecf6354cf'
if (-not $env:LOCALAPPDATA -or -not [IO.Path]::IsPathRooted($env:LOCALAPPDATA)) {
    throw (Get-SetupText 'LOCALAPPDATA must be an absolute Windows directory.' 'LOCALAPPDATA mutlak bir Windows dizini olmalıdır.')
}
$script:ApplicationRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'LumiaDAM'))
$script:LocalAIRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'LumiaDAM\local-ai'))

function Assert-LocalAIPath([string]$Path, [string]$Root = $script:LocalAIRoot) {
    $full = [IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($Root + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw (Get-SetupText 'Unsafe path outside the LumiaDAM data directory.' 'Güvensiz yol: LumiaDAM veri dizininin dışına çıkılamaz.')
    }
    # Üst dizinlerdeki junction/symlink hedefleri de sınırın dışına çıkabilir.
    $current = $full
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw (Get-SetupText 'Unsafe reparse point in the LumiaDAM data directory.' 'LumiaDAM veri dizininde güvensiz bağlantı noktası bulundu.')
            }
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
    return $full
}

function Remove-LocalAITree([string]$Path) {
    $full = Assert-LocalAIPath $Path
    if (Test-Path -LiteralPath $full) {
        foreach ($item in (Get-ChildItem -LiteralPath $full -Recurse -Force)) {
            Assert-LocalAIPath $item.FullName | Out-Null
        }
        Remove-Item -LiteralPath $full -Recurse -Force
    }
}

function Write-LocalAIRequest {
    $marker = Assert-LocalAIPath (Join-Path $script:LocalAIRoot 'install-requested.json')
    Write-SettingsJson $marker ([pscustomobject]@{ requested = $true })
}

function Read-SettingsJson([string]$Path, [int]$Limit = 16384, [string]$Root = $script:LocalAIRoot) {
    $full = Assert-LocalAIPath $Path $Root
    if (-not (Test-Path -LiteralPath $full)) { return [pscustomobject]@{} }
    if ((Get-Item -LiteralPath $full).Length -gt $Limit) {
        throw (Get-SetupText 'The settings file is too large; it was preserved.' 'Ayar dosyası çok büyük; dosya değiştirilmedi.')
    }
    $json = [IO.File]::ReadAllText($full)
    try { $settings = $json | ConvertFrom-Json } catch {
        throw (Get-SetupText 'The settings file contains invalid JSON; it was preserved.' 'Ayar dosyasında geçersiz JSON var; dosya değiştirilmedi.')
    }
    if (-not $json.TrimStart().StartsWith('{') -or $null -eq $settings -or
        $settings.GetType() -ne [System.Management.Automation.PSCustomObject]) {
        throw (Get-SetupText 'The settings file must contain a JSON object; it was preserved.' 'Ayar dosyası bir JSON nesnesi içermelidir; dosya değiştirilmedi.')
    }
    return $settings
}

function Write-SettingsJson([string]$Path, [pscustomobject]$Settings, [string]$Root = $script:LocalAIRoot) {
    $full = Assert-LocalAIPath $Path $Root
    $temporary = Assert-LocalAIPath ($full + '.' + [guid]::NewGuid().ToString('N') + '.tmp') $Root
    $json = $Settings | ConvertTo-Json -Depth 64
    $bytes = (New-Object Text.UTF8Encoding($false)).GetBytes($json)
    if ($bytes.Length -gt 16384) {
        throw (Get-SetupText 'The settings file is too large; it was preserved.' 'Ayar dosyası çok büyük; dosya değiştirilmedi.')
    }
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($full)) | Out-Null
    try {
        $file = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $file.Write($bytes, 0, $bytes.Length)
            $file.Flush($true)
        } finally { $file.Dispose() }
        # Aynı dizindeki atomik değişim başarısız olursa eski dosyayı silmeyiz.
        Assert-LocalAIPath $full $Root | Out-Null
        if (Test-Path -LiteralPath $full) { [IO.File]::Replace($temporary, $full, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $full) }
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    }
}

function Save-InstallerLanguage {
    $path = Join-Path $script:ApplicationRoot 'preferences.json'
    $preferences = Read-SettingsJson $path 16384 $script:ApplicationRoot
    $preferences | Add-Member -NotePropertyName 'language' -NotePropertyValue $Language -Force
    Write-SettingsJson $path $preferences $script:ApplicationRoot
}

function Save-LocalAISettings {
    $path = Join-Path $script:LocalAIRoot 'settings.json'
    $settings = Read-SettingsJson $path
    # Varsayılan güncelleme var olan dosyanın baytlarını bile değiştirmez.
    if ($Provider -eq 'keep' -and $Mode -eq 'keep' -and (Test-Path -LiteralPath $path)) { return }
    $defaults = [ordered]@{
        mode = 'disabled'; provider = 'bundled'; endpoint = 'http://127.0.0.1:11434';
        model = 'qwen3.5:0.8b'; idleMinutes = 10; startHour = 1; endHour = 6;
        cpuOnly = $true; batchSize = 100; setupComplete = $false
    }
    foreach ($name in $defaults.Keys) {
        if ($null -eq $settings.PSObject.Properties[$name]) {
            $settings | Add-Member -NotePropertyName $name -NotePropertyValue $defaults[$name]
        }
    }
    if ($Provider -ne 'keep' -and $settings.provider -ne $Provider) {
        $settings.provider = $Provider
        $settings.endpoint = if ($Provider -eq 'lmstudio') { 'http://127.0.0.1:1234' } else { 'http://127.0.0.1:11434' }
        $settings.model = 'qwen3.5:0.8b'
        # Motor değişince yeni model ve adres uygulamada doğrulanmalıdır.
        $settings.setupComplete = $false
    }
    if ($Mode -ne 'keep') { $settings.mode = $Mode }
    Write-SettingsJson $path $settings
}

function Install-LocalAIArchive([string]$ArchivePath, [string]$ExpectedSha256) {
    if ($ExpectedSha256 -notmatch '^[a-fA-F0-9]{64}$' -or
        (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash -ne $ExpectedSha256) {
        throw (Get-SetupText 'The Ollama archive failed SHA256 verification. No runtime was installed.' 'Ollama arşivi SHA256 doğrulamasından geçemedi. Runtime kurulmadı.')
    }
    $stage = Assert-LocalAIPath (Join-Path $script:LocalAIRoot ('.extract-' + [guid]::NewGuid().ToString('N')))
    $runtime = Assert-LocalAIPath (Join-Path $script:LocalAIRoot 'runtime')
    $backup = Assert-LocalAIPath (Join-Path $script:LocalAIRoot ('.previous-' + [guid]::NewGuid().ToString('N')))
    $zip = [IO.Compression.ZipFile]::OpenRead($ArchivePath)
    $previousMoved = $false
    $installed = $false
    try {
        $paths = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        [long]$totalBytes = 0
        $hasExecutable = $false
        if ($zip.Entries.Count -gt 10000) { throw (Get-SetupText 'Unsafe archive: too many entries.' 'Güvensiz arşiv: çok fazla dosya içeriyor.') }
        # Tüm merkez dizini çıkarmadan önce doğrula; Windows ad eşlemesi de hesaba katılır.
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('\', '/')
            $parts = $name.TrimEnd('/').Split('/')
            $type = ($entry.ExternalAttributes -shr 16) -band 0xF000
            if (-not $name -or $name.StartsWith('/') -or $name -match '[<>:"|?*\x00-\x1f]' -or
                $type -notin @(0, 0x8000, 0x4000) -or ($entry.ExternalAttributes -band 0x400) -ne 0) {
                throw ((Get-SetupText 'Unsafe archive entry: ' 'Güvensiz arşiv girdisi: ') + $name)
            }
            foreach ($part in $parts) {
                if (-not $part -or $part -eq '.' -or $part -eq '..' -or $part -match '[. ]$' -or
                    $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])($|\.)') {
                    throw ((Get-SetupText 'Unsafe archive entry: ' 'Güvensiz arşiv girdisi: ') + $name)
                }
            }
            $destination = [IO.Path]::GetFullPath((Join-Path $stage ($name.Replace('/', '\'))))
            if (-not $destination.StartsWith($stage + '\', [StringComparison]::OrdinalIgnoreCase) -or
                -not $paths.Add($destination.TrimEnd('\'))) {
                throw ((Get-SetupText 'Unsafe archive destination: ' 'Güvensiz arşiv hedefi: ') + $name)
            }
            $totalBytes += $entry.Length
            if ($name -ieq 'ollama.exe' -and $entry.Length -gt 0) { $hasExecutable = $true }
            if ($totalBytes -gt 16GB) { throw (Get-SetupText 'Unsafe archive: extracted size exceeds the limit.' 'Güvensiz arşiv: açılan dosyaların boyutu sınırı aşıyor.') }
        }
        if (-not $hasExecutable) { throw (Get-SetupText 'The archive does not contain a non-empty ollama.exe file.' 'Arşivde geçerli bir ollama.exe dosyası bulunamadı.') }
        $drive = New-Object IO.DriveInfo([IO.Path]::GetPathRoot($stage))
        if ($drive.AvailableFreeSpace -lt ($totalBytes + 512MB)) { throw (Get-SetupText 'Not enough free disk space to extract the Ollama runtime.' 'Ollama runtime dosyalarını açmak için yeterli boş disk alanı yok.') }
        [IO.Directory]::CreateDirectory($stage) | Out-Null
        foreach ($entry in $zip.Entries) {
            $destination = Assert-LocalAIPath (Join-Path $stage $entry.FullName.Replace('/', '\'))
            if ($entry.FullName.EndsWith('/') -or $entry.FullName.EndsWith('\')) {
                [IO.Directory]::CreateDirectory($destination) | Out-Null
                continue
            }
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
            $inputStream = $entry.Open()
            try {
                $outputStream = [IO.File]::Open($destination, [IO.FileMode]::CreateNew)
                try { $inputStream.CopyTo($outputStream) } finally { $outputStream.Dispose() }
            } finally { $inputStream.Dispose() }
        }
        # Resmî v0.40.4 LICENSE metni; arşivdeki diğer tüm lisanslar da aynen korunur.
        # https://github.com/ollama/ollama/blob/v0.40.4/LICENSE
        $license = @'
MIT License

Copyright (c) Ollama

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
'@
        [IO.File]::WriteAllText((Join-Path $stage 'LICENSE-ollama.txt'), $license)
        if (Test-Path -LiteralPath $runtime) {
            Assert-LocalAIPath $runtime | Out-Null
            Assert-LocalAIPath $backup | Out-Null
            Move-Item -LiteralPath $runtime -Destination $backup
            $previousMoved = $true
        }
        Assert-LocalAIPath $stage | Out-Null
        Assert-LocalAIPath $runtime | Out-Null
        Move-Item -LiteralPath $stage -Destination $runtime
        $installed = $true
        Write-LocalAIRequest
        if ($previousMoved) {
            # Başarılı kurulumdan sonraki temizlik hatası çalışan runtime'ı geri almaz.
            try { Remove-LocalAITree $backup } catch { Write-Warning ((Get-SetupText 'The previous runtime backup could not be removed: ' 'Önceki runtime yedeği kaldırılamadı: ') + $_.Exception.Message) }
        }
    } catch {
        if ($installed) { Remove-LocalAITree $runtime }
        if ($previousMoved -and (Test-Path -LiteralPath $backup)) {
            Assert-LocalAIPath $backup | Out-Null
            Assert-LocalAIPath $runtime | Out-Null
            Move-Item -LiteralPath $backup -Destination $runtime
        }
        throw
    } finally {
        $zip.Dispose()
        Remove-LocalAITree $stage
    }
}

# Dot-source yalnız çevrimdışı fixture testlerinin işlevleri çağırmasını sağlar.
if ($MyInvocation.InvocationName -ne '.') {
    $download = $null
    try {
        if ($LanguageOnly -and -not $Language) {
            throw (Get-SetupText 'Choose en or tr for the installation language.' 'Kurulum dili için en veya tr seçin.')
        }
        if ($Language) { Save-InstallerLanguage }
        if ($LanguageOnly) {
            Write-Output (Get-SetupText 'Application language saved.' 'Uygulama dili kaydedildi.')
            exit 0
        }
        # Ağ yalnız açık bundled seçimiyle kullanılabilir; diğer yollar yalnız ayar yazar.
        if ($ConfigureOnly -or $Provider -ne 'bundled') {
            Save-LocalAISettings
            Write-LocalAIRequest
            Write-Output (Get-SetupText 'Local AI preferences saved. No runtime or model was downloaded or started.' 'Yerel AI tercihleri kaydedildi. Runtime veya model indirilmedi ve başlatılmadı.')
            exit 0
        }
        $runtimeExe = Assert-LocalAIPath (Join-Path $script:LocalAIRoot 'runtime\ollama.exe')
        if (Test-Path -LiteralPath $runtimeExe -PathType Leaf) {
            Save-LocalAISettings
            Write-LocalAIRequest
            Write-Output (Get-SetupText 'The existing private Ollama runtime was preserved. No model was started.' 'Mevcut özel Ollama runtime korundu. Hiçbir model başlatılmadı.')
            exit 0
        }
        if (-not [Environment]::Is64BitOperatingSystem) { throw (Get-SetupText 'The optional local runtime requires 64-bit Windows.' 'İsteğe bağlı yerel runtime, 64 bit Windows gerektirir.') }
        $drive = New-Object IO.DriveInfo([IO.Path]::GetPathRoot($script:LocalAIRoot))
        if ($drive.AvailableFreeSpace -lt 8GB) { throw (Get-SetupText 'The optional Ollama download needs at least 8 GB of free disk space during installation.' 'İsteğe bağlı Ollama indirmesi, kurulum sırasında en az 8 GB boş disk alanı gerektirir.') }
        $download = Assert-LocalAIPath (Join-Path $script:LocalAIRoot ('.download-' + [guid]::NewGuid().ToString('N')))
        [IO.Directory]::CreateDirectory($download) | Out-Null
        $archive = Join-Path $download 'ollama-windows-amd64.zip'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $client = New-Object Net.Http.HttpClient
        $client.Timeout = [TimeSpan]::FromMinutes(40)
        $cancellation = New-Object Threading.CancellationTokenSource
        $cancellation.CancelAfter(2400000)
        try {
            $response = $client.GetAsync($script:ArchiveUri, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancellation.Token).GetAwaiter().GetResult()
            try {
                $response.EnsureSuccessStatusCode() | Out-Null
                $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
                try {
                    $file = [IO.File]::Open($archive, [IO.FileMode]::CreateNew)
                    try { $stream.CopyToAsync($file, 81920, $cancellation.Token).GetAwaiter().GetResult() } finally { $file.Dispose() }
                } finally { $stream.Dispose() }
            } finally { $response.Dispose() }
        } finally { $client.Dispose(); $cancellation.Dispose() }
        Install-LocalAIArchive $archive $script:ArchiveSha256
        Save-LocalAISettings
        Write-Output (Get-SetupText 'Ollama runtime installed. No model, service or startup task was started. Review Local AI in the application menu.' 'Ollama runtime kuruldu. Model, servis veya başlangıç görevi çalıştırılmadı. Uygulama menüsünden Yerel AI ayarlarını inceleyin.')
    } catch {
        Write-Output ((Get-SetupText 'Setup could not complete: ' 'Kurulum tamamlanamadı: ') + $_.Exception.Message)
        exit 1
    } finally {
        if ($download) { Remove-LocalAITree $download }
    }
}
