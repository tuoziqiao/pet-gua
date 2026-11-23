$ErrorActionPreference='Stop'
$installDir=[IO.Path]::GetFullPath($PSScriptRoot)
$key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\gua'
$registered=Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
if(-not $registered.InstallLocation -or -not [string]::Equals([IO.Path]::GetFullPath($registered.InstallLocation),$installDir,[StringComparison]::OrdinalIgnoreCase)){
    throw '卸载脚本只能在已登记的安装目录中运行。'
}
$exeName=if($registered.BrandExecutable){[string]$registered.BrandExecutable}else{'gua.exe'}
$shortcut=if($registered.BrandShortcut){[string]$registered.BrandShortcut}else{'呱'}
if($exeName -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_-]{0,39}\.exe$'){throw '已登记的程序文件名无效。'}
$exePath=Join-Path $installDir $exeName
if(-not [string]::Equals([string]$registered.DisplayIcon,($exePath+',0'),[StringComparison]::OrdinalIgnoreCase)){throw '已登记的程序路径无效。'}
if($shortcut -in @('.','..') -or [IO.Path]::GetFileName($shortcut) -ne $shortcut -or $shortcut.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0){throw '已登记的快捷方式名称无效。'}
Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($exeName)) -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $exePath} | ForEach-Object {
    $null=$_.CloseMainWindow();if(-not $_.WaitForExit(3000)){Stop-Process -Id $_.Id}
}
$shell=New-Object -ComObject WScript.Shell
$links=@((Join-Path $shell.SpecialFolders.Item('Desktop') ($shortcut+'.lnk')),(Join-Path $env:APPDATA ('Microsoft\Windows\Start Menu\Programs\'+$shortcut+'.lnk')),(Join-Path $env:APPDATA ('Microsoft\Windows\Start Menu\Programs\'+$shortcut+' - 设置.lnk')))
foreach($path in $links){if((Test-Path -LiteralPath $path) -and [string]::Equals($shell.CreateShortcut($path).TargetPath,$exePath,[StringComparison]::OrdinalIgnoreCase)){Remove-Item -LiteralPath $path}}
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$run=(Get-ItemProperty -LiteralPath $runKey -ErrorAction SilentlyContinue).gua
if($run -and [string]::Equals($run.Trim('"'),$exePath,[StringComparison]::OrdinalIgnoreCase)){Remove-ItemProperty -Path $runKey -Name gua -ErrorAction SilentlyContinue}
Remove-Item -LiteralPath $key -ErrorAction SilentlyContinue
$files=@($exeName,'THIRD_PARTY_NOTICES.txt','LICENSE','DESKTOP-PET-ASSETS.md','uninstall.ps1')
if($registered.BrandBackup -eq 'gua-previous.exe' -and $exeName -ne 'gua-previous.exe'){$files+='gua-previous.exe'}
foreach($name in $files){
    $path=Join-Path $installDir $name;if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path}
}
if(-not(Get-ChildItem -LiteralPath $installDir -Force)){Remove-Item -LiteralPath $installDir}
Write-Output '应用已卸载。待办、饮水记录和设置仍保存在本机用户数据目录中。'
