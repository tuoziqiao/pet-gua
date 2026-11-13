param([switch]$SkipBuild, [string]$Executable, [string]$NsisCompiler, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'branding.ps1')
$brand = Get-ProjectBrand $projectRoot
Push-Location $projectRoot
try {
    $version = $brand.Version
    if (-not $SkipBuild) {
        $env:CARGO_BUILD_JOBS = '1'
        cargo +stable-x86_64-pc-windows-msvc build --locked --release --bin gua --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    }
    if (-not $Executable) {
        $targetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $projectRoot 'target' }
        $Executable = Join-Path $targetDirectory 'x86_64-pc-windows-msvc\release\gua.exe'
    }
    $binary = Get-Item -LiteralPath $Executable
    if ($binary.Length -gt 200MB) { throw 'Executable exceeds the 200 MiB update limit.' }
    & (Join-Path $PSScriptRoot 'check-runtime.ps1') -Executable $binary.FullName
    if ($binary.VersionInfo.FileVersion -ne $version -and $binary.VersionInfo.FileVersion -ne "$version.0") { throw 'Executable version mismatch' }
    if ($binary.VersionInfo.FileDescription -cne $brand.DisplayName -or
        $binary.VersionInfo.OriginalFilename -cne ($brand.PackageName + '.exe') -or
        $binary.VersionInfo.ProductName -cne ($brand.DisplayName + ' ' + $brand.PackageName)) {
        throw 'Executable branding differs from branding.json; rebuild before packaging.'
    }
    if (-not $NsisCompiler) {
        $compiler = Get-Command makensis.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        $candidates = @()
        if ($compiler) { $candidates += $compiler.Source }
        if ($env:NSIS_HOME) { $candidates += Join-Path $env:NSIS_HOME 'makensis.exe' }
        if (${env:ProgramFiles(x86)}) { $candidates += Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe' }
        if ($env:ProgramFiles) { $candidates += Join-Path $env:ProgramFiles 'NSIS\makensis.exe' }
        $toolsDirectory = Join-Path $projectRoot 'target\packaging-tools'
        $candidates += Join-Path $toolsDirectory 'nsis-3.13\makensis.exe'
        foreach ($candidate in $candidates) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                $NsisCompiler = $candidate
                break
            }
        }
        if (-not $NsisCompiler) {
            New-Item -ItemType Directory -Path $toolsDirectory -Force | Out-Null
            Write-Host 'NSIS not found; downloading the verified NSIS 3.13 compiler...'
            try {
                $NsisCompiler = & (Join-Path $PSScriptRoot 'get-nsis.ps1') -Directory $toolsDirectory
            } catch {
                throw "Could not download NSIS: $($_.Exception.Message). Install NSIS 3 or pass -NsisCompiler with the path to makensis.exe."
            }
        }
    }
    if (-not (Test-Path -LiteralPath $NsisCompiler -PathType Leaf)) { throw "NSIS compiler not found: $NsisCompiler. Install NSIS 3 or pass -NsisCompiler with the path to makensis.exe." }
    $packageName = "$($brand.PackageName)-v$version-windows-x64"
    $setupName = "$($brand.PackageName)-Setup-v$version-x64.exe"
    $archiveName = "$packageName.zip"
    if (-not $OutputDirectory) {
        $OutputDirectory = Join-Path $projectRoot 'dist'
        $existing = @($packageName, $setupName, $archiveName, 'SHA256SUMS.txt') | Where-Object {
            Test-Path -LiteralPath (Join-Path $OutputDirectory $_)
        }
        if ($existing) {
            $buildName = 'build-{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), [guid]::NewGuid().ToString('N').Substring(0, 8)
            $OutputDirectory = Join-Path $OutputDirectory $buildName
            Write-Host "Existing package files preserved; writing to $OutputDirectory"
        }
    }
    $outputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
    $stagingDirectory = Join-Path $outputDirectory $packageName
    foreach ($name in @($packageName, $setupName, $archiveName, 'SHA256SUMS.txt')) {
        if (Test-Path -LiteralPath (Join-Path $outputDirectory $name)) {
            throw "Package output already exists: $outputDirectory. Choose another -OutputDirectory."
        }
    }
    New-Item -ItemType Directory -Path $stagingDirectory -Force | Out-Null
    Copy-Item -LiteralPath $binary.FullName -Destination (Join-Path $stagingDirectory ($brand.PackageName + '.exe'))
    # ZIP 保留给免安装使用和应用内更新；普通用户只需下载安装 EXE。
    Copy-Item -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt') -Destination $stagingDirectory
    $projectLicense = Join-Path $projectRoot 'LICENSE'
    if (Test-Path -LiteralPath $projectLicense -PathType Leaf) {
        Copy-Item -LiteralPath $projectLicense -Destination $stagingDirectory
    } else {
        Write-Warning '项目根目录没有 LICENSE，本次安装包和便携包将不包含该文件。'
    }
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs\desktop-pet-assets.md') -Destination (Join-Path $stagingDirectory 'DESKTOP-PET-ASSETS.md')
    $setup = Join-Path $outputDirectory $setupName
    $brandInclude = Join-Path $stagingDirectory 'branding.nsh'
    $nameLiteral = ConvertTo-NsisLiteral $brand.DisplayName
    $metadataLiteral = ConvertTo-NsisMetadataLiteral $brand.DisplayName
    $shortcutLiteral = ConvertTo-NsisLiteral $brand.ShortcutName
    $include = "!define APP_NAME `"$nameLiteral`"`r`n!define APP_NAME_METADATA `"$metadataLiteral`"`r`n!define APP_PACKAGE `"$($brand.PackageName)`"`r`n!define APP_SHORTCUT_NAME `"$shortcutLiteral`"`r`n"
    [IO.File]::WriteAllText($brandInclude, $include, [Text.UTF8Encoding]::new($true))
    & $NsisCompiler /V2 /WX "/DAPP_VERSION=$version" "/DPROJECT_DIR=$projectRoot" "/DSOURCE_EXE=$($binary.FullName)" "/DOUTPUT_FILE=$setup" "/DBRAND_INCLUDE=$brandInclude" (Join-Path $projectRoot 'installer\setup.nsi')
    if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
    Remove-Item -LiteralPath $brandInclude
    $archive = Join-Path $outputDirectory $archiveName
    Compress-Archive -LiteralPath $stagingDirectory -DestinationPath $archive
    if ((Get-Item -LiteralPath $archive).Length -gt 200MB) { throw 'Update ZIP exceeds the 200 MiB update limit.' }
    @($setup, $archive) | ForEach-Object {
        $checksum = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant()
        "$checksum  $([IO.Path]::GetFileName($_))"
    } | Set-Content -LiteralPath (Join-Path $outputDirectory 'SHA256SUMS.txt') -Encoding ascii
    Write-Output $setup
    Write-Output $archive
} finally { Pop-Location }
