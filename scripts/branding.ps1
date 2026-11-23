# 构建与安装辅助脚本共用同一份品牌配置；内部应用身份保持 gua。
function Get-ProjectBrand([string]$ProjectRoot) {
    $path = Join-Path $ProjectRoot 'branding.json'
    try { $value = Get-Content -LiteralPath $path -Raw -Encoding UTF8 | ConvertFrom-Json }
    catch { throw "无法读取品牌配置 $path：$($_.Exception.Message)" }
    $keys = @($value.PSObject.Properties.Name)
    if ($keys -cnotcontains 'display_name' -or $keys -cnotcontains 'package_name' -or $keys -cnotcontains 'version' -or
        $value.display_name -isnot [string] -or $value.package_name -isnot [string] -or $value.version -isnot [string]) {
        throw 'branding.json 必须包含字符串 display_name、package_name 和 version。'
    }
    $display = $value.display_name
    $package = $value.package_name
    $version = $value.version
    if ([string]::IsNullOrWhiteSpace($display) -or $display.Length -gt 80 -or $display -match '\p{Cc}' -or $display.Contains('${') -or $display.Contains('$%')) {
        throw 'display_name 必须非空、不能包含控制字符或 ${、$% 模板符号，且不能超过 80 个 UTF-16 字符。'
    }
    if ($package -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_-]{0,39}$' -or $package -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9]|UNINSTALL)$') {
        throw 'package_name 须以字母或数字开头，只能包含英文字母、数字、下划线、连字符，最多 40 个字符且不能是 Windows 保留名称或 uninstall。'
    }
    if ($version -cnotmatch '^\d+\.\d+\.\d+$') {
        throw 'version 须匹配 semver 基础格式 x.y.z。'
    }
    # 快捷方式文件名受 Windows 路径规则约束，界面显示名本身保持不变。
    $shortcut = ($display -replace '[<>:"/\\|?*]', '_').TrimEnd([char[]]@(' ', '.'))
    if ([string]::IsNullOrWhiteSpace($shortcut)) { $shortcut = '_' }
    if ($shortcut -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\..*)?$') { $shortcut = '_' + $shortcut }
    [pscustomobject]@{ DisplayName = $display; PackageName = $package; Version = $version; ShortcutName = $shortcut }
}

function ConvertTo-NsisLiteral([string]$Value) {
    $Value.Replace('$', '$$').Replace('"', '$\"')
}

function ConvertTo-NsisMetadataLiteral([string]$Value) {
    $Value.Replace('"', '$\"')
}
