param([string]$SourceExecutable)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
. (Join-Path $projectRoot 'scripts\branding.ps1')
$brand=Get-ProjectBrand $projectRoot
if(-not $SourceExecutable){
    $packaged=Join-Path $projectRoot ($brand.PackageName+'.exe')
    $SourceExecutable=if(Test-Path -LiteralPath $packaged){$packaged}else{Join-Path $projectRoot 'target\release\gua.exe'}
}
$sourcePath=[IO.Path]::GetFullPath($SourceExecutable)
if(-not(Test-Path -LiteralPath $sourcePath -PathType Leaf)){throw "找不到$($brand.DisplayName)程序，请先完成构建。"}
$sourceVersion=(Get-Item -LiteralPath $sourcePath).VersionInfo
if($sourceVersion.FileDescription -cne $brand.DisplayName -or $sourceVersion.OriginalFilename -cne ($brand.PackageName+'.exe')){
    throw '程序名称与 branding.json 不一致，请先重新构建。'
}
$key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\gua'
$registered=Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
$installDir=if($registered.InstallLocation){$registered.InstallLocation}else{Join-Path $env:LOCALAPPDATA 'Programs\gua'}
$installDir=[IO.Path]::GetFullPath($installDir)
$exePath=Join-Path $installDir ($brand.PackageName+'.exe')
$oldExeName=if($registered.BrandExecutable){[string]$registered.BrandExecutable}else{'gua.exe'}
$oldShortcut=if($registered.BrandShortcut){[string]$registered.BrandShortcut}else{'呱'}
$oldPath=$null
if($registered.InstallLocation -and $oldExeName -cmatch '^[A-Za-z0-9][A-Za-z0-9_-]{0,39}\.exe$'){
    $candidate=Join-Path $installDir $oldExeName
    if([string]::Equals([string]$registered.DisplayIcon,($candidate+',0'),[StringComparison]::OrdinalIgnoreCase)){$oldPath=$candidate}
}
if($oldShortcut -in @('.','..') -or [IO.Path]::GetFileName($oldShortcut) -ne $oldShortcut -or $oldShortcut.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0){$oldShortcut=''}
if((Test-Path -LiteralPath $exePath) -and -not [string]::Equals($exePath,$oldPath,[StringComparison]::OrdinalIgnoreCase)){
    throw '安装位置已有同名程序文件，请选择其他文件夹。'
}
$psExe=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$uninstallScript=Join-Path $installDir 'uninstall.ps1'
$expectedUninstall='"'+$psExe+'" -NoProfile -ExecutionPolicy Bypass -File "'+$uninstallScript+'"'
if((Test-Path -LiteralPath $uninstallScript) -and -not [string]::Equals([string]$registered.UninstallString,$expectedUninstall,[StringComparison]::OrdinalIgnoreCase)){
    throw '安装位置已有其他卸载脚本，请选择其他文件夹。'
}
New-Item -ItemType Directory -Path $installDir -Force | Out-Null
$shell=New-Object -ComObject WScript.Shell
$desktop=$shell.SpecialFolders.Item('Desktop')
$programs=Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$oldLinks=if($oldShortcut){@((Join-Path $desktop ($oldShortcut+'.lnk')),(Join-Path $programs ($oldShortcut+'.lnk')),(Join-Path $programs ($oldShortcut+' - 设置.lnk')))}else{@()}
$desktopChoice=-not $registered.InstallLocation
if($oldPath -and $oldLinks.Count -gt 0 -and (Test-Path -LiteralPath $oldLinks[0])){
    $desktopChoice=[string]::Equals($shell.CreateShortcut($oldLinks[0]).TargetPath,$oldPath,[StringComparison]::OrdinalIgnoreCase)
}
$entries=@(@((Join-Path $programs ($brand.ShortcutName+'.lnk')),''),@((Join-Path $programs ($brand.ShortcutName+' - 设置.lnk')),'--settings'))
if($desktopChoice){$entries+=,@((Join-Path $desktop ($brand.ShortcutName+'.lnk')),'')}
foreach($entry in $entries){
    if(Test-Path -LiteralPath $entry[0]){
        $target=$shell.CreateShortcut($entry[0]).TargetPath
        if(-not $target -or (-not [string]::Equals($target,$oldPath,[StringComparison]::OrdinalIgnoreCase) -and -not [string]::Equals($target,$exePath,[StringComparison]::OrdinalIgnoreCase))){
            throw '已有同名快捷方式指向其他程序，请先调整快捷方式名称。'
        }
    }
}
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$run=(Get-ItemProperty -LiteralPath $runKey -ErrorAction SilentlyContinue).gua
$autoStart=$oldPath -and $run -and [string]::Equals($run.Trim('"'),$oldPath,[StringComparison]::OrdinalIgnoreCase)
foreach($processName in @([IO.Path]::GetFileNameWithoutExtension($exePath),[IO.Path]::GetFileNameWithoutExtension($oldExeName)) | Select-Object -Unique){
    Get-Process -Name $processName -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $exePath -or ($oldPath -and $_.Path -eq $oldPath)} | ForEach-Object {
        $null=$_.CloseMainWindow();if(-not $_.WaitForExit(3000)){Stop-Process -Id $_.Id; $_.WaitForExit()}
    }
}
$backup=Join-Path $installDir 'gua-previous.exe'
$ownedBackup=if($registered.BrandBackup -eq 'gua-previous.exe'){'gua-previous.exe'}else{''}
if($oldPath -and (Test-Path -LiteralPath $oldPath) -and $oldPath -ne $backup -and $exePath -ne $backup -and
    ($ownedBackup -or -not (Test-Path -LiteralPath $backup))){
    Copy-Item -LiteralPath $oldPath -Destination $backup -Force
    $ownedBackup='gua-previous.exe'
}
Copy-Item -LiteralPath $sourcePath -Destination $exePath -Force
if(Test-Path -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt')){Copy-Item -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_NOTICES.txt') -Destination $installDir -Force}
if(Test-Path -LiteralPath (Join-Path $projectRoot 'LICENSE')){Copy-Item -LiteralPath (Join-Path $projectRoot 'LICENSE') -Destination $installDir -Force}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'uninstall.ps1') -Destination $uninstallScript -Force
foreach($path in $oldLinks){
    if($oldPath -and (Test-Path -LiteralPath $path) -and [string]::Equals($shell.CreateShortcut($path).TargetPath,$oldPath,[StringComparison]::OrdinalIgnoreCase)){Remove-Item -LiteralPath $path}
}
if($oldPath -and $oldPath -ne $exePath -and (Test-Path -LiteralPath $oldPath)){Remove-Item -LiteralPath $oldPath}
foreach($entry in $entries){
    $link=$shell.CreateShortcut($entry[0]);$link.TargetPath=$exePath;$link.Arguments=$entry[1]
    $link.WorkingDirectory=$installDir;$link.IconLocation=$exePath+',0';$link.Description=$brand.DisplayName+' '+$brand.PackageName;$link.Save()
}
if($autoStart){Set-ItemProperty -LiteralPath $runKey -Name gua -Value ('"'+$exePath+'"')}
New-Item -Path $key -Force | Out-Null
$version=(Get-Item -LiteralPath $exePath).VersionInfo.FileVersion
$values=@{DisplayName=($brand.DisplayName+' '+$brand.PackageName);DisplayVersion=$version;Publisher=$brand.DisplayName;InstallLocation=$installDir;DisplayIcon=($exePath+',0');BrandExecutable=($brand.PackageName+'.exe');BrandShortcut=$brand.ShortcutName;BrandBackup=$ownedBackup;UninstallString=$expectedUninstall}
foreach($name in $values.Keys){New-ItemProperty -Path $key -Name $name -Value $values[$name] -PropertyType String -Force | Out-Null}
foreach($name in @('NoModify','NoRepair')){New-ItemProperty -Path $key -Name $name -Value 1 -PropertyType DWord -Force | Out-Null}
Start-Process -FilePath $exePath -WorkingDirectory $installDir -WindowStyle Hidden
Write-Output ($brand.DisplayName+' '+$version+' 已安装：'+$exePath)
