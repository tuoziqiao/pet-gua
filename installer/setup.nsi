; -*- coding: utf-8 -*-
Unicode true
!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "FileFunc.nsh"
!include "Win\COM.nsh"

!ifdef BRAND_INCLUDE
  !include "${BRAND_INCLUDE}"
!endif
!ifndef APP_ID
  !define APP_ID "gua"
!endif
!ifndef APP_NAME
  !error "请通过 scripts/package.ps1 提供品牌配置（APP_NAME）。"
!endif
!ifndef APP_PACKAGE
  !define APP_PACKAGE "gua"
!endif
!ifndef APP_NAME_METADATA
  !define APP_NAME_METADATA "${APP_NAME}"
!endif
!ifndef APP_SHORTCUT_NAME
  !define APP_SHORTCUT_NAME "${APP_NAME}"
!endif
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
Name "${APP_NAME}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_ID}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
ManifestDPIAware true
SetCompressor /SOLID lzma
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "ProductName" "${APP_NAME_METADATA}"
VIAddVersionKey /LANG=2052 "FileDescription" "${APP_NAME_METADATA}安装程序"
VIAddVersionKey /LANG=2052 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "LegalCopyright" "${APP_NAME_METADATA}贡献者"
!define MUI_ICON "${PROJECT_DIR}\assets\gua.ico"
!define MUI_UNICON "${PROJECT_DIR}\assets\gua.ico"
!define MUI_ABORTWARNING
Var BrandIcon
Var PathInput
Var DesktopCheckbox
Var AutoStartCheckbox
Var DesktopChoice
Var AutoStartChoice
Var PreviousDir
Var PreviousExe
Var PreviousShortcut
Var PreviousPath
Var PreviousUninstall
Var OwnedTarget
Var NewPath
Page custom InstallPage InstallPageLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "${APP_NAME}已安装"
!define MUI_FINISHPAGE_TEXT "音乐、提醒和日常小事，现在都在桌面上的小岛里。"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_PACKAGE}.exe"
!define MUI_FINISHPAGE_RUN_TEXT "打开${APP_NAME}"
!insertmacro MUI_PAGE_FINISH
!define MUI_UNCONFIRMPAGE_TEXT_TOP "将卸载${APP_NAME}。设置、待办和连接记录会保留，重新安装后仍可使用。"
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
BrandingText "${APP_NAME} · ${APP_PACKAGE}"
SetFont "Microsoft YaHei UI" 9

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "${APP_NAME}需要 Windows 10 或 Windows 11 的 64 位系统。" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "${APP_NAME}需要 Windows 10 或 Windows 11。" /SD IDOK
    SetErrorLevel 1
    Quit
  ${EndIf}
  SetShellVarContext current
  SetRegView 64
  StrCpy $DesktopChoice 1
  StrCpy $AutoStartChoice 0
  ReadRegStr $PreviousDir HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ReadRegStr $PreviousUninstall HKCU "${UNINSTALL_KEY}" "UninstallString"
  StrCpy $PreviousPath ""
  ${If} $PreviousDir != ""
    GetFullPathName $PreviousDir "$PreviousDir"
    ReadRegStr $PreviousExe HKCU "${UNINSTALL_KEY}" "BrandExecutable"
    ReadRegStr $PreviousShortcut HKCU "${UNINSTALL_KEY}" "BrandShortcut"
    ${If} $PreviousExe == ""
      StrCpy $PreviousExe "gua.exe"
    ${EndIf}
    ${If} $PreviousShortcut == ""
      StrCpy $PreviousShortcut "呱"
    ${EndIf}
    ${GetFileName} "$PreviousExe" $0
    ${GetFileExt} "$PreviousExe" $1
    ReadRegStr $2 HKCU "${UNINSTALL_KEY}" "DisplayIcon"
    ${If} $0 == $PreviousExe
    ${AndIf} $1 == "exe"
    ${AndIf} $2 == "$PreviousDir\$PreviousExe,0"
      GetFullPathName $PreviousPath "$PreviousDir\$PreviousExe"
    ${EndIf}
    ${GetFileName} "$PreviousShortcut" $0
    ${If} $0 != $PreviousShortcut
      StrCpy $PreviousShortcut ""
    ${EndIf}
    StrCpy $DesktopChoice 0
    ${If} $PreviousPath != ""
    ${AndIf} $PreviousShortcut != ""
      Push "$DESKTOP\$PreviousShortcut.lnk"
      Call ShortcutTarget
      Pop $0
      ${If} $0 == $PreviousPath
        StrCpy $DesktopChoice 1
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $PreviousPath != ""
  ${AndIf} $0 == '"$PreviousPath"'
    StrCpy $AutoStartChoice 1
  ${EndIf}
  ; 静默安装使用同一组选项；/D 路径由 NSIS 自身处理。
  ${GetParameters} $0
  ${GetOptions} $0 "/DESKTOP=" $1
  ${If} $1 == "0"
  ${OrIf} $1 == "1"
    StrCpy $DesktopChoice $1
  ${EndIf}
  ${GetOptions} $0 "/AUTOSTART=" $1
  ${If} $1 == "0"
  ${OrIf} $1 == "1"
    StrCpy $AutoStartChoice $1
  ${EndIf}
FunctionEnd

Function InstallPage
  !insertmacro MUI_HEADER_TEXT "欢迎使用${APP_NAME}" "Windows 顶部的桌面伙伴"
  nsDialogs::Create 1018
  Pop $0
  InitPluginsDir
  File /oname=$PLUGINSDIR\brand.ico "${PROJECT_DIR}\assets\gua.ico"
  ${NSD_CreateIcon} 0 6u 44u 44u ""
  Pop $0
  System::Call 'user32::LoadImageW(p 0, w "$PLUGINSDIR\brand.ico", i 1, i 64, i 64, i 0x10) p .r1'
  StrCpy $BrandIcon $1
  SendMessage $0 0x170 $1 0
  ${NSD_CreateLabel} 58u 8u 240u 22u "${APP_NAME}"
  Pop $0
  CreateFont $1 "Microsoft YaHei UI" 18 700
  SendMessage $0 ${WM_SETFONT} $1 0
  ${NSD_CreateLabel} 112u 14u 188u 14u "v${APP_VERSION} · 升级保留设置和记录"
  Pop $0
  ${NSD_CreateLabel} 58u 34u 240u 20u "让重要的信息，轻轻出现在屏幕顶部。"
  Pop $0
  ${NSD_CreateLabel} 0 58u 300u 16u "音乐歌词 · 邮件验证码 · 日程提醒 · AI 额度"
  Pop $0
  ${NSD_CreateLabel} 0 80u 300u 12u "安装位置"
  Pop $0
  ${NSD_CreateDirRequest} 0 96u 245u 16u "$INSTDIR"
  Pop $PathInput
  ${NSD_CreateBrowseButton} 250u 96u 50u 16u "浏览…"
  Pop $0
  ${NSD_OnClick} $0 BrowseDirectory
  ${NSD_CreateCheckbox} 0 122u 150u 14u "创建桌面快捷方式"
  Pop $DesktopCheckbox
  ${NSD_SetState} $DesktopCheckbox $DesktopChoice
  ${NSD_CreateCheckbox} 165u 122u 135u 14u "开机自启"
  Pop $AutoStartCheckbox
  ${NSD_SetState} $AutoStartCheckbox $AutoStartChoice
  GetDlgItem $0 $HWNDPARENT 1
  SendMessage $0 ${WM_SETTEXT} 0 "STR:一键安装"
  nsDialogs::Show
FunctionEnd

Function BrowseDirectory
  Pop $0
  ${NSD_GetText} $PathInput $0
  nsDialogs::SelectFolderDialog "选择${APP_NAME}安装文件夹" "$0"
  Pop $0
  ${If} $0 != "error"
    ${NSD_SetText} $PathInput "$0"
  ${EndIf}
FunctionEnd

Function ValidateDirectory
  System::Call 'shlwapi::PathIsRelativeW(w "$INSTDIR") i .r0'
  ${If} $0 != 0
    Goto invalid
  ${EndIf}
  System::Call 'shlwapi::PathIsRootW(w "$INSTDIR") i .r0'
  ${If} $0 != 0
    Goto invalid
  ${EndIf}
  ClearErrors
  CreateDirectory "$INSTDIR"
  IfErrors invalid
  GetFullPathName $INSTDIR "$INSTDIR"
  IfErrors invalid
  GetTempFileName $0 "$INSTDIR"
  IfErrors invalid
  Delete "$0"
  SetErrorLevel 0
  Return
  invalid:
    MessageBox MB_OK|MB_ICONEXCLAMATION "请选择可写入的完整安装文件夹，例如 D:\Apps\${APP_PACKAGE}。不要选择磁盘根目录或需要管理员权限的文件夹。" /SD IDOK
    SetErrorLevel 1
    Abort
FunctionEnd

Function InstallPageLeave
  ${NSD_GetText} $PathInput $INSTDIR
  ${NSD_GetState} $DesktopCheckbox $DesktopChoice
  ${NSD_GetState} $AutoStartCheckbox $AutoStartChoice
  Call ValidateDirectory
FunctionEnd

Function .onGUIEnd
  System::Call 'user32::DestroyIcon(p $BrandIcon)'
FunctionEnd

; 程序运行时请用户正常退出，不强制结束进程或写入正在使用的程序。
!macro ClosedCheck Prefix
Function ${Prefix}CheckClosed
  retry:
  System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "Local\${APP_ID}.SingleInstance") p .r0'
  ${If} $0 != 0
    System::Call 'kernel32::CloseHandle(p r0)'
    MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "请先从系统托盘退出${APP_NAME}，然后点击重试。" /SD IDCANCEL IDRETRY retry
    SetErrorLevel 2
    Abort
  ${EndIf}
FunctionEnd
!macroend
!insertmacro ClosedCheck ""
!insertmacro ClosedCheck "un."

; 只移除目标指向已登记程序的快捷方式，避免误删用户的同名链接。
!macro ShortcutFunctions Prefix
Function ${Prefix}ShortcutTarget
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  System::Call 'ole32::CoInitialize(p 0)i.r5'
  StrCpy $1 0
  StrCpy $2 0
  !insertmacro ComHlpr_CreateInProcInstance "${CLSID_ShellLink}" "${IID_IShellLink}" r1 ".r3"
  StrCpy $4 0
  ${If} $1 P<> 0
    ${IUnknown::QueryInterface} $1 '("${IID_IPersistFile}",.r2)'
    ${If} $2 P<> 0
      ${IPersistFile::Load} $2 '("$0",0).r3'
      ${If} $3 == 0
        IntOp $3 ${NSIS_MAX_STRLEN} * 2
        System::Alloc $3
        Pop $4
        ${If} $4 P<> 0
          System::Call '$1->3(p r4,i ${NSIS_MAX_STRLEN},p 0,i 4)i.r3'
          ${If} $3 == 0
            System::Call '*$4(&w${NSIS_MAX_STRLEN} .r0)'
          ${Else}
            StrCpy $0 ""
          ${EndIf}
        ${Else}
          StrCpy $0 ""
        ${EndIf}
      ${Else}
        StrCpy $0 ""
      ${EndIf}
      ${IUnknown::Release} $2 ""
    ${Else}
      StrCpy $0 ""
    ${EndIf}
    ${IUnknown::Release} $1 ""
  ${Else}
    StrCpy $0 ""
  ${EndIf}
  ${If} $4 P<> 0
    System::Free $4
  ${EndIf}
  ${If} $5 >= 0
    System::Call 'ole32::CoUninitialize()'
  ${EndIf}
  ${If} $0 != ""
    GetFullPathName $0 "$0"
  ${EndIf}
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd

Function ${Prefix}RemoveOwnedShortcut
  Exch $0
  Push $1
  Push "$0"
  Call ${Prefix}ShortcutTarget
  Pop $1
  ${If} $1 != ""
  ${AndIf} $1 == $OwnedTarget
    Delete "$0"
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd
!macroend
!insertmacro ShortcutFunctions ""
!insertmacro ShortcutFunctions "un."

Function CheckShortcutAvailable
  Exch $0
  Push $1
  ${If} ${FileExists} "$0"
    Push "$0"
    Call ShortcutTarget
    Pop $1
    ${If} $1 == ""
    ${OrIf} $1 != $PreviousPath
    ${AndIf} $1 != $NewPath
      MessageBox MB_OK|MB_ICONSTOP "已有同名快捷方式指向其他程序，请先调整快捷方式名称。" /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd

Section "安装"
  Call CheckClosed
  Call ValidateDirectory
  GetFullPathName $NewPath "$INSTDIR\${APP_PACKAGE}.exe"
  ${If} ${FileExists} "$NewPath"
  ${AndIf} $NewPath != $PreviousPath
    MessageBox MB_OK|MB_ICONSTOP "安装位置已有同名程序文件，请选择其他文件夹。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\Uninstall.exe"
    ${If} $PreviousDir != $INSTDIR
    ${OrIf} $PreviousUninstall != '"$PreviousDir\Uninstall.exe"'
      MessageBox MB_OK|MB_ICONSTOP "安装位置已有其他卸载程序，请选择其他文件夹。" /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
  ${If} $DesktopChoice == 1
    Push "$DESKTOP\${APP_SHORTCUT_NAME}.lnk"
    Call CheckShortcutAvailable
  ${EndIf}
  Push "$SMPROGRAMS\${APP_SHORTCUT_NAME}.lnk"
  Call CheckShortcutAvailable
  Push "$SMPROGRAMS\${APP_SHORTCUT_NAME} - 设置.lnk"
  Call CheckShortcutAvailable
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File /oname=${APP_PACKAGE}.exe "${SOURCE_EXE}"
  !if /FileExists "${PROJECT_DIR}\LICENSE"
    File "${PROJECT_DIR}\LICENSE"
  !endif
  File "${PROJECT_DIR}\THIRD_PARTY_NOTICES.txt"
  File /oname=DESKTOP-PET-ASSETS.md "${PROJECT_DIR}\docs\desktop-pet-assets.md"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  ${If} $PreviousPath != ""
  ${AndIf} $PreviousShortcut != ""
    StrCpy $OwnedTarget "$PreviousPath"
    Push "$DESKTOP\$PreviousShortcut.lnk"
    Call RemoveOwnedShortcut
    Push "$SMPROGRAMS\$PreviousShortcut.lnk"
    Call RemoveOwnedShortcut
    Push "$SMPROGRAMS\$PreviousShortcut - 设置.lnk"
    Call RemoveOwnedShortcut
  ${EndIf}
  GetFullPathName $0 "$INSTDIR\${APP_PACKAGE}.exe"
  ${If} $PreviousPath != ""
  ${AndIf} $PreviousPath != $0
    Delete "$PreviousPath"
  ${EndIf}
  ${If} $PreviousPath != ""
  ${AndIf} $PreviousDir != $INSTDIR
  ${AndIf} $PreviousUninstall == '"$PreviousDir\Uninstall.exe"'
    Delete "$PreviousDir\Uninstall.exe"
  ${EndIf}
  ${If} $DesktopChoice == 1
    CreateShortcut "$DESKTOP\${APP_SHORTCUT_NAME}.lnk" "$INSTDIR\${APP_PACKAGE}.exe"
  ${Else}
    StrCpy $OwnedTarget "$0"
    Push "$DESKTOP\${APP_SHORTCUT_NAME}.lnk"
    Call RemoveOwnedShortcut
  ${EndIf}
  ${If} $AutoStartChoice == 1
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}" '"$INSTDIR\${APP_PACKAGE}.exe"'
  ${Else}
    ReadRegStr $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
    ${If} $1 == '"$PreviousPath"'
    ${OrIf} $1 == '"$0"'
      DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
    ${EndIf}
  ${EndIf}
  CreateShortcut "$SMPROGRAMS\${APP_SHORTCUT_NAME}.lnk" "$INSTDIR\${APP_PACKAGE}.exe"
  CreateShortcut "$SMPROGRAMS\${APP_SHORTCUT_NAME} - 设置.lnk" "$INSTDIR\${APP_PACKAGE}.exe" "--settings"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME} ${APP_PACKAGE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_PACKAGE}.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "BrandExecutable" "${APP_PACKAGE}.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "BrandShortcut" "${APP_SHORTCUT_NAME}"
  ${If} $PreviousDir != $INSTDIR
    DeleteRegValue HKCU "${UNINSTALL_KEY}" "BrandBackup"
  ${EndIf}
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${If} $PreviousDir == $INSTDIR
  ${AndIf} $PreviousUninstall == '"$WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$PreviousDir\uninstall.ps1"'
    Delete "$INSTDIR\uninstall.ps1"
  ${EndIf}
SectionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Section "Uninstall"
  Call un.CheckClosed
  GetFullPathName $OwnedTarget "$INSTDIR\${APP_PACKAGE}.exe"
  Push "$DESKTOP\${APP_SHORTCUT_NAME}.lnk"
  Call un.RemoveOwnedShortcut
  Push "$SMPROGRAMS\${APP_SHORTCUT_NAME}.lnk"
  Call un.RemoveOwnedShortcut
  Push "$SMPROGRAMS\${APP_SHORTCUT_NAME} - 设置.lnk"
  Call un.RemoveOwnedShortcut
  Delete "$INSTDIR\${APP_PACKAGE}.exe"
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "BrandBackup"
  ${If} $0 == "gua-previous.exe"
  ${AndIf} "${APP_PACKAGE}.exe" != "gua-previous.exe"
    Delete "$INSTDIR\gua-previous.exe"
  ${EndIf}
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\DESKTOP-PET-ASSETS.md"
  Delete "$INSTDIR\Uninstall.exe"
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${If} $0 == '"$INSTDIR\${APP_PACKAGE}.exe"'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
  ${EndIf}
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ; 只移除空的安装目录，用户数据和用户自行放入的文件都保留。
  RMDir "$INSTDIR"
SectionEnd
