Unicode true
ManifestDPIAware true
; Add in `dpiAwareness` `PerMonitorV2` to manifest for Windows 10 1607+ (note this should not affect lower versions since they should be able to ignore this and pick up `dpiAware` `true` set by `ManifestDPIAware true`)
; Currently undocumented on NSIS's website but is in the Docs folder of source tree, see
; https://github.com/kichik/nsis/blob/5fc0b87b819a9eec006df4967d08e522ddd651c9/Docs/src/attributes.but#L286-L300
; https://github.com/tauri-apps/tauri/pull/10106
ManifestDPIAwareness PerMonitorV2

!if "{{compression}}" == "none"
  SetCompress off
!else
  ; Set the compression algorithm. We default to LZMA.
  SetCompressor /SOLID "{{compression}}"
!endif

; Keep above !include to stay ahead of any plugin command
; see https://github.com/tauri-apps/tauri/pull/15422#discussion_r3289239624
{{#if signed_plugins_path}}
!addplugindir "{{signed_plugins_path}}"
{{/if}}

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "utils.nsh"
!include "FileAssociation.nsh"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "StrFunc.nsh"
${StrCase}
${StrLoc}

{{#if installer_hooks}}
!include "{{installer_hooks}}"
{{/if}}

!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define HOMEPAGE "{{homepage}}"
!define INSTALLMODE "{{install_mode}}"
!define LICENSE "{{license}}"
!define INSTALLERICON "{{installer_icon}}"
!define SIDEBARIMAGE "{{sidebar_image}}"
!define HEADERIMAGE "{{header_image}}"
!define UNINSTALLERICON "{{uninstaller_icon}}"
!define UNINSTALLERHEADERIMAGE "{{uninstaller_header_image}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define MAINBINARYSRCPATH "{{main_binary_path}}"
!define BUNDLEID "{{bundle_id}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define ARCH "{{arch}}"
!define ADDITIONALPLUGINSPATH "{{additional_plugins_path}}"
!define ALLOWDOWNGRADES "{{allow_downgrades}}"
!define DISPLAYLANGUAGESELECTOR "{{display_language_selector}}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2BOOTSTRAPPERPATH "{{webview2_bootstrapper_path}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
!define MINIMUMWEBVIEW2VERSION "{{minimum_webview2_version}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUKEY "Software\${MANUFACTURER}"
!define MANUPRODUCTKEY "${MANUKEY}\${PRODUCTNAME}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"
!define STARTMENUFOLDER "{{start_menu_folder}}"

Var PassiveMode
Var UpdateMode
Var NoShortcutMode
Var WixMode
Var OldMainBinaryName
Var InstallMaxPlugin
Var MaxPluginCheckbox
Var MaxPluginDialog
Var InstallLocalAI
Var LocalAIConfirmed
Var LocalAICheckbox
Var LocalAIDialog
Var LocalAIResult
Var SetupDialog
Var SetupTitleFont
Var SetupHeadingFont
Var SetupBodyFont
Var SetupIcon
Var AIExpanded
Var MaxExpanded
Var AIDetails
Var MaxDetails
Var AIToggle
Var MaxToggle
Var AIProvider
Var AIProviderControl
Var AIMode
Var AIModeControl
Var AIHint
Var MaxAutoLoad
Var MaxAutomaticControl
Var MaxManualControl
Var InstallLanguage
Var LanguageConfirmed
Var AICard
Var MaxCard
Var AISummary
Var MaxSummary

; Aynı tipografi ve renkler standart sayfalarda da kullanılır; ek bir UI bağımlılığı yoktur.
SetFont "Segoe UI" 10
!define MUI_BGCOLOR "171B22"
!define MUI_TEXTCOLOR "EDF2F7"
!define MUI_INSTFILESPAGE_COLORS "AAB8C9 171B22"
!define MUI_CUSTOMFUNCTION_GUIINIT LumiaSetupThemeInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.LumiaSetupThemeInit

; Her kontrol kendi erişilebilir Windows davranışını ve klavye odağını korur.
!macro LumiaThemeFunctions PREFIX
Function ${PREFIX}LumiaSetupThemeInit
  CreateFont $SetupTitleFont "Segoe UI" 22 600
  CreateFont $SetupHeadingFont "Segoe UI" 11 600
  CreateFont $SetupBodyFont "Segoe UI" 10 400
  System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 20, *i 1, i 4)'
  Call ${PREFIX}LumiaThemePage
FunctionEnd
Function ${PREFIX}LumiaThemePage
  Push $0
  StrCpy $0 $HWNDPARENT
  Call ${PREFIX}LumiaThemeChildren
  SendMessage $mui.Header.Text ${WM_SETFONT} $SetupHeadingFont 1
  SetCtlColors $mui.Header.Text "55D8F2" "171B22"
  Pop $0
FunctionEnd
Function ${PREFIX}LumiaThemeChildren
  Push $1
  Push $2
  SetCtlColors $0 "EDF2F7" "171B22"
  System::Call 'user32::GetWindow(p r0, i 5) p.r1'
  ${While} $1 <> 0
    System::Call 'user32::GetClassNameW(p r1, w .r2, i ${NSIS_MAX_STRLEN})'
    ${If} $2 == "Button"
      System::Call 'uxtheme::SetWindowTheme(p r1, w "", w "")'
    ${EndIf}
    Push $0
    StrCpy $0 $1
    Call ${PREFIX}LumiaThemeChildren
    Pop $0
    System::Call 'user32::GetWindow(p r1, i 2) p.r1'
  ${EndWhile}
  Pop $2
  Pop $1
FunctionEnd
!macroend
!insertmacro LumiaThemeFunctions ""
!insertmacro LumiaThemeFunctions "un."

Name "${PRODUCTNAME}"
BrandingText "$(lumiaBranding)"
OutFile "${OUTFILE}"

; We don't actually use this value as default install path,
; it's just for nsis to append the product name folder in the directory selector
; https://nsis.sourceforge.io/Reference/InstallDir
!define PLACEHOLDER_INSTALL_DIR "placeholder\${PRODUCTNAME}"
InstallDir "${PLACEHOLDER_INSTALL_DIR}"

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

# additional plugins
!addplugindir "${ADDITIONALPLUGINSPATH}"

; Uninstaller signing command
!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

; Handle install mode, `perUser`, `perMachine` or `both`
!if "${INSTALLMODE}" == "perMachine"
  RequestExecutionLevel admin
!endif

!if "${INSTALLMODE}" == "currentUser"
  RequestExecutionLevel user
!endif

!if "${INSTALLMODE}" == "both"
  !define MULTIUSER_MUI
  !define MULTIUSER_INSTALLMODE_INSTDIR "${PRODUCTNAME}"
  !define MULTIUSER_INSTALLMODE_COMMANDLINE
  !if "${ARCH}" == "x64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !else if "${ARCH}" == "arm64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !endif
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "${UNINSTKEY}"
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "CurrentUser"
  !define MULTIUSER_INSTALLMODEPAGE_SHOWUSERNAME
  !define MULTIUSER_INSTALLMODE_FUNCTION RestorePreviousInstallLocation
  !define MULTIUSER_EXECUTIONLEVEL Highest
  !include MultiUser.nsh
!endif

; Installer icon
!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
!endif

; Installer sidebar image
!if "${SIDEBARIMAGE}" != ""
  !define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
!endif

; Enable header images for installer and uninstaller pages when either image is configured.
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!else if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!endif

; Installer header image
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_BITMAP "${HEADERIMAGE}"
!endif

; Uninstaller header image
!if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_UNBITMAP "${UNINSTALLERHEADERIMAGE}"
!endif

; Uninstaller icon
!if "${UNINSTALLERICON}" != ""
  !define MUI_UNICON "${UNINSTALLERICON}"
!endif

; Define registry key to store installer language
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "${MANUPRODUCTKEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"
!define MUI_LANGDLL_ALWAYSSHOW
!define MUI_LANGDLL_WINDOWTITLE "LumiaDAM / Language / Dil"
!define MUI_LANGDLL_INFO "Choose your language. / Dilinizi seçin."

; Installer pages, must be ordered as they appear
; 1. Welcome Page
Page custom PageLumiaWelcome
Function PageLumiaWelcome
  Call SkipIfPassive
  !insertmacro MUI_HEADER_TEXT "$(lumiaWelcomeTitle)" "$(lumiaWelcomeSubtitle)"
  nsDialogs::Create 1018
  Pop $SetupDialog
  ${NSD_CreateIcon} 0u 3u 32u 32u ""
  Pop $0
  ${NSD_SetIconFromInstaller} $0 $SetupIcon
  ${NSD_CreateLabel} 43u 0u 250u 30u "LumiaDAM"
  Pop $0
  SendMessage $0 ${WM_SETFONT} $SetupTitleFont 0
  ${NSD_CreateLabel} 43u 32u 250u 24u "$(lumiaWelcomeTagline)"
  Pop $0
  ${NSD_CreateLabel} 0u 67u 98u 13u "$(lumiaOrganize)"
  Pop $0
  SendMessage $0 ${WM_SETFONT} $SetupHeadingFont 0
  ${NSD_CreateLabel} 104u 67u 98u 13u "$(lumiaCreate)"
  Pop $0
  SendMessage $0 ${WM_SETFONT} $SetupHeadingFont 0
  ${NSD_CreateLabel} 208u 67u 92u 13u "$(lumiaConnect)"
  Pop $0
  SendMessage $0 ${WM_SETFONT} $SetupHeadingFont 0
  ${NSD_CreateLabel} 0u 84u 93u 37u "$(lumiaOrganizeText)"
  Pop $0
  ${NSD_CreateLabel} 104u 84u 93u 37u "$(lumiaCreateText)"
  Pop $0
  ${NSD_CreateLabel} 208u 84u 92u 37u "$(lumiaConnectText)"
  Pop $0
  ${NSD_CreateLabel} 0u 130u 100% 10u "$(lumiaVersion)"
  Pop $0
  Call LumiaThemePage
  nsDialogs::Show
FunctionEnd

; 2. License Page (if defined)
!if "${LICENSE}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !define MUI_PAGE_CUSTOMFUNCTION_SHOW LumiaThemePage
  !insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!endif

; 3. Install mode (if it is set to `both`)
!if "${INSTALLMODE}" == "both"
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MULTIUSER_PAGE_INSTALLMODE
!endif

; 4. Custom page to ask user if he wants to reinstall/uninstall
;    only if a previous installation was detected
Var ReinstallPageCheck
Page custom PageReinstall PageLeaveReinstall
Function PageReinstall
  ; Uninstall previous WiX installation if exists.
  ;
  ; A WiX installer stores the installation info in registry
  ; using a UUID and so we have to loop through all keys under
  ; `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
  ; and check if `DisplayName` and `Publisher` keys match ${PRODUCTNAME} and ${MANUFACTURER}
  ;
  ; This has a potential issue that there maybe another installation that matches
  ; our ${PRODUCTNAME} and ${MANUFACTURER} but wasn't installed by our WiX installer,
  ; however, this should be fine since the user will have to confirm the uninstallation
  ; and they can chose to abort it if doesn't make sense.
  StrCpy $0 0
  wix_loop:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" wix_loop_done ; Exit loop if there is no more keys to loop on
    IntOp $0 $0 + 1
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "Publisher"
    StrCmp "$R0$R1" "${PRODUCTNAME}${MANUFACTURER}" 0 wix_loop
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    ${StrCase} $R1 $R0 "L"
    ${StrLoc} $R0 $R1 "msiexec" ">"
    StrCmp $R0 0 0 wix_loop_done
    StrCpy $WixMode 1
    StrCpy $R6 "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1"
    Goto compare_version
  wix_loop_done:

  ; Check if there is an existing installation, if not, abort the reinstall page
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" ""
  ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
  ${IfThen} "$R0$R1" == "" ${|} Abort ${|}

  ; Compare this installar version with the existing installation
  ; and modify the messages presented to the user accordingly
  compare_version:
  StrCpy $R4 "$(older)"
  ${If} $WixMode = 1
    ReadRegStr $R0 HKLM "$R6" "DisplayVersion"
  ${Else}
    ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${EndIf}
  ${IfThen} $R0 == "" ${|} StrCpy $R4 "$(unknown)" ${|}

  nsis_tauri_utils::SemverCompare "${VERSION}" $R0
  Pop $R0

  ; Upgrading ($R0 = 1) or Reinstalling same version ($R0 = 0):
  ; Seamlessly replace older version in-place without prompting to uninstall or messing up data
  ${If} $R0 >= 0
    ReadRegStr $INSTDIR SHCTX "${MANUPRODUCTKEY}" ""
    ${If} $INSTDIR == ""
      ReadRegStr $INSTDIR SHCTX "${UNINSTKEY}" "InstallLocation"
    ${EndIf}
    StrCpy $UpdateMode 1
    Abort
  ${EndIf}

  ; Downgrading
  ${If} $R0 = -1
    StrCpy $R1 "$(newerVersionInstalled)"
    StrCpy $R2 "$(uninstallBeforeInstalling)"
    !if "${ALLOWDOWNGRADES}" == "true"
      StrCpy $R3 "$(dontUninstall)"
    !else
      StrCpy $R3 "$(dontUninstallDowngrade)"
    !endif
    !insertmacro MUI_HEADER_TEXT "$(alreadyInstalled)" "$(choowHowToInstall)"
  ${Else}
    Abort
  ${EndIf}

  ; Skip showing the page if passive
  ;
  ; Note that we don't call this earlier at the beginning
  ; of this function because we need to populate some variables
  ; related to current installed version if detected and whether
  ; we are downgrading or not.
  ${If} $PassiveMode = 1
    Call PageLeaveReinstall
  ${Else}
    nsDialogs::Create 1018
    Pop $R4
    ${IfThen} $(^RTL) = 1 ${|} nsDialogs::SetRTL $(^RTL) ${|}

    ${NSD_CreateLabel} 0 0 100% 24u $R1
    Pop $R1

    ${NSD_CreateRadioButton} 30u 50u -30u 8u $R2
    Pop $R2
    ${NSD_OnClick} $R2 PageReinstallUpdateSelection

    ${NSD_CreateRadioButton} 30u 70u -30u 8u $R3
    Pop $R3
    ; Disable this radio button if downgrading and downgrades are disabled
    !if "${ALLOWDOWNGRADES}" == "false"
      ${IfThen} $R0 = -1 ${|} EnableWindow $R3 0 ${|}
    !endif
    ${NSD_OnClick} $R3 PageReinstallUpdateSelection

    ; Check the first radio button if this the first time
    ; we enter this page or if the second button wasn't
    ; selected the last time we were on this page
    ${If} $ReinstallPageCheck <> 2
      SendMessage $R2 ${BM_SETCHECK} ${BST_CHECKED} 0
    ${Else}
      SendMessage $R3 ${BM_SETCHECK} ${BST_CHECKED} 0
    ${EndIf}

    ${NSD_SetFocus} $R2
    nsDialogs::Show
  ${EndIf}
FunctionEnd
Function PageReinstallUpdateSelection
  ${NSD_GetState} $R2 $R1
  ${If} $R1 == ${BST_CHECKED}
    StrCpy $ReinstallPageCheck 1
  ${Else}
    StrCpy $ReinstallPageCheck 2
  ${EndIf}
FunctionEnd
Function PageLeaveReinstall
  ${NSD_GetState} $R2 $R1

  ; If migrating from Wix, always uninstall
  ${If} $WixMode = 1
    Goto reinst_uninstall
  ${EndIf}

  ; In update mode, always proceeds without uninstalling
  ${If} $UpdateMode = 1
    Goto reinst_done
  ${EndIf}

  ; $R0 holds whether same(0)/upgrading(1)/downgrading(-1) version
  ; $R1 holds the radio buttons state:
  ;   1 => first choice was selected
  ;   0 => second choice was selected
  ${If} $R0 = 0 ; Same version, proceed
    ${If} $R1 = 1              ; User chose to add/reinstall
      Goto reinst_done
    ${Else}                    ; User chose to uninstall
      Goto reinst_uninstall
    ${EndIf}
  ${ElseIf} $R0 = 1 ; Upgrading
    ${If} $R1 = 1              ; User chose to uninstall
      Goto reinst_uninstall
    ${Else}
      Goto reinst_done         ; User chose NOT to uninstall
    ${EndIf}
  ${ElseIf} $R0 = -1 ; Downgrading
    ${If} $R1 = 1              ; User chose to uninstall
      Goto reinst_uninstall
    ${Else}
      Goto reinst_done         ; User chose NOT to uninstall
    ${EndIf}
  ${EndIf}

  reinst_uninstall:
    HideWindow
    ClearErrors

    ${If} $WixMode = 1
      ReadRegStr $R1 HKLM "$R6" "UninstallString"
      ExecWait '$R1' $0
    ${Else}
      ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
      ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
      ${IfThen} $UpdateMode = 1 ${|} StrCpy $R1 "$R1 /UPDATE" ${|} ; append /UPDATE
      ${IfThen} $PassiveMode = 1 ${|} StrCpy $R1 "$R1 /P" ${|} ; append /P
      StrCpy $R1 "$R1 _?=$4" ; append uninstall directory
      ExecWait '$R1' $0
    ${EndIf}

    BringToFront

    ${IfThen} ${Errors} ${|} StrCpy $0 2 ${|} ; ExecWait failed, set fake exit code

    ${If} $0 <> 0
    ${OrIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      ; User cancelled wix uninstaller? return to select un/reinstall page
      ${If} $WixMode = 1
      ${AndIf} $0 = 1602
        Abort
      ${EndIf}

      ; User cancelled NSIS uninstaller? return to select un/reinstall page
      ${If} $0 = 1
        Abort
      ${EndIf}

      ; Other errors? show generic error message and return to select un/reinstall page
      MessageBox MB_ICONEXCLAMATION "$(unableToUninstall)"
      Abort
    ${EndIf}
  reinst_done:
FunctionEnd

; 5. Choose install directory page
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!define MUI_PAGE_CUSTOMFUNCTION_SHOW LumiaThemePage
!insertmacro MUI_PAGE_DIRECTORY

Function InstallSelectedMaxPlugin
  SetShellVarContext current
  WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "MaxPluginRequested" $InstallMaxPlugin
  WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "MaxPluginAutoLoad" $MaxAutoLoad
  ${If} $InstallMaxPlugin == ${BST_CHECKED}
  ${AndIf} $MaxAutoLoad == 1
    ClearErrors
    CreateDirectory "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents"
    CopyFiles /SILENT "$INSTDIR\plugins\3dsmax\LumiaDAM.bundle\PackageContents.xml" "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle"
    CopyFiles /SILENT "$INSTDIR\plugins\3dsmax\LumiaDAM.bundle\Contents\startup.ms" "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents"
    CopyFiles /SILENT "$INSTDIR\plugins\3dsmax\LumiaDAM.bundle\Contents\lumiadam_max.py" "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents"
    ${If} ${Errors}
      MessageBox MB_ICONSTOP "$(lumiaMaxInstallError)"
      Abort
    ${EndIf}
    WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "MaxPluginInstalled" 1
  ${Else}
    ReadRegDWORD $0 HKCU "${MANUPRODUCTKEY}" "MaxPluginInstalled"
    ${If} $0 == 1
      ; Yalnız LumiaDAM'in kurduğu dosyalar kaldırılır; diğer eklentilere dokunulmaz.
      Delete "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents\startup.ms"
      Delete "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents\lumiadam_max.py"
      Delete "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\PackageContents.xml"
      RMDir "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents"
      RMDir "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle"
    ${EndIf}
    WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "MaxPluginInstalled" 0
  ${EndIf}
  !insertmacro SetContext
FunctionEnd

; 6. Start menu shortcut page
Var AppStartMenuFolder
!if "${STARTMENUFOLDER}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !define MUI_STARTMENUPAGE_DEFAULTFOLDER "${STARTMENUFOLDER}"
!else
  !define MUI_PAGE_CUSTOMFUNCTION_PRE Skip
!endif
!define MUI_PAGE_CUSTOMFUNCTION_SHOW LumiaThemePage
!insertmacro MUI_PAGE_STARTMENU Application $AppStartMenuFolder

; Temel kurulumdan sonra tek seçim ekranı; ayrıntılar yalnız seçilen bileşenler için açılır.
Page custom PageOptionalComponents PageLeaveOptionalComponents
Page custom PageLocalAI PageLeaveLocalAI
Page custom PageMaxPlugin PageLeaveMaxPlugin

Function PageOptionalComponents
  Call SkipIfPassive
  !insertmacro MUI_HEADER_TEXT "$(lumiaComponentsTitle)" "$(lumiaComponentsSubtitle)"
  nsDialogs::Create 1018
  Pop $SetupDialog
  ${If} $SetupDialog == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0u 0u 145u 132u ""
  Pop $AICard
  ${NSD_CreateLabel} 153u 0u 145u 132u ""
  Pop $MaxCard
  ${NSD_CreateCheckbox} 9u 9u 126u 17u "$(lumiaLocalAI)"
  Pop $LocalAICheckbox
  ${NSD_SetState} $LocalAICheckbox $InstallLocalAI
  SendMessage $LocalAICheckbox ${WM_SETFONT} $SetupHeadingFont 0
  ${NSD_CreateLabel} 9u 33u 126u 29u "$(lumiaAISummary)"
  Pop $AISummary
  ${NSD_CreateButton} 9u 65u 126u 17u "$(lumiaDetailsMore)"
  Pop $AIToggle
  ${NSD_OnClick} $AIToggle ToggleAIDetails
  nsDialogs::CreateControl EDIT ${DEFAULT_STYLES}|${WS_TABSTOP}|${WS_VSCROLL}|${ES_MULTILINE}|${ES_READONLY} ${WS_EX_CLIENTEDGE} 9u 87u 126u 38u "$(lumiaAIDescription)"
  Pop $AIDetails
  ${NSD_CreateCheckbox} 162u 9u 126u 17u "$(lumiaMaxPlugin)"
  Pop $MaxPluginCheckbox
  ${NSD_SetState} $MaxPluginCheckbox $InstallMaxPlugin
  SendMessage $MaxPluginCheckbox ${WM_SETFONT} $SetupHeadingFont 0
  ${NSD_CreateLabel} 162u 33u 126u 29u "$(lumiaMaxSummary)"
  Pop $MaxSummary
  ${NSD_CreateButton} 162u 65u 126u 17u "$(lumiaDetailsMore)"
  Pop $MaxToggle
  ${NSD_OnClick} $MaxToggle ToggleMaxDetails
  nsDialogs::CreateControl EDIT ${DEFAULT_STYLES}|${WS_TABSTOP}|${WS_VSCROLL}|${ES_MULTILINE}|${ES_READONLY} ${WS_EX_CLIENTEDGE} 162u 87u 126u 38u "$(lumiaMaxDescription)"
  Pop $MaxDetails
  Call LumiaThemePage
  SetCtlColors $AICard "EDF2F7" "232A35"
  SetCtlColors $MaxCard "EDF2F7" "232A35"
  SetCtlColors $AISummary "EDF2F7" "232A35"
  SetCtlColors $MaxSummary "EDF2F7" "232A35"
  SetCtlColors $AIDetails "EDF2F7" "232A35"
  SetCtlColors $MaxDetails "EDF2F7" "232A35"
  SetCtlColors $LocalAICheckbox "55D8F2" "232A35"
  SetCtlColors $MaxPluginCheckbox "55D8F2" "232A35"
  Call UpdateComponentDetails
  nsDialogs::Show
FunctionEnd

Function ToggleAIDetails
  Pop $0
  IntOp $AIExpanded $AIExpanded ^ 1
  Call UpdateComponentDetails
FunctionEnd
Function ToggleMaxDetails
  Pop $0
  IntOp $MaxExpanded $MaxExpanded ^ 1
  Call UpdateComponentDetails
FunctionEnd
Function UpdateComponentDetails
  ${If} $AIExpanded == 1
    ShowWindow $AIDetails ${SW_SHOW}
    ${NSD_SetText} $AIToggle "$(lumiaDetailsLess)"
  ${Else}
    ShowWindow $AIDetails ${SW_HIDE}
    ${NSD_SetText} $AIToggle "$(lumiaDetailsMore)"
  ${EndIf}
  ${If} $MaxExpanded == 1
    ShowWindow $MaxDetails ${SW_SHOW}
    ${NSD_SetText} $MaxToggle "$(lumiaDetailsLess)"
  ${Else}
    ShowWindow $MaxDetails ${SW_HIDE}
    ${NSD_SetText} $MaxToggle "$(lumiaDetailsMore)"
  ${EndIf}
FunctionEnd
Function PageLeaveOptionalComponents
  ${NSD_GetState} $LocalAICheckbox $InstallLocalAI
  ${NSD_GetState} $MaxPluginCheckbox $InstallMaxPlugin
  ; Geri dönüşte kaldırılan seçim eski bir indirme onayını kullanamaz.
  StrCpy $LocalAIConfirmed 0
FunctionEnd

Function PageLocalAI
  Call SkipIfPassive
  ${If} $InstallLocalAI <> ${BST_CHECKED}
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "$(lumiaAITitle)" "$(lumiaAISubtitle)"
  nsDialogs::Create 1018
  Pop $LocalAIDialog
  ${If} $LocalAIDialog == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0u 0u 100% 13u "$(lumiaAIEngineQuestion)"
  Pop $0
  ${NSD_CreateDropList} 0u 17u 100% 65u ""
  Pop $AIProviderControl
  ${NSD_CB_AddString} $AIProviderControl "$(lumiaAIPrivate)"
  ${NSD_CB_AddString} $AIProviderControl "$(lumiaAIExistingOllama)"
  ${NSD_CB_AddString} $AIProviderControl "$(lumiaAIExistingLMStudio)"
  ${NSD_CB_AddString} $AIProviderControl "$(lumiaAIKeepEngine)"
  SendMessage $AIProviderControl ${CB_SETCURSEL} $AIProvider 0
  ${NSD_OnChange} $AIProviderControl UpdateAIHint
  ${NSD_CreateLabel} 0u 40u 100% 30u ""
  Pop $AIHint
  ${NSD_CreateLabel} 0u 75u 100% 13u "$(lumiaAIModeQuestion)"
  Pop $0
  ${NSD_CreateDropList} 0u 92u 100% 55u ""
  Pop $AIModeControl
  ${NSD_CB_AddString} $AIModeControl "$(lumiaAIDisabled)"
  ${NSD_CB_AddString} $AIModeControl "$(lumiaAIManual)"
  ${NSD_CB_AddString} $AIModeControl "$(lumiaAIKeepSchedule)"
  SendMessage $AIModeControl ${CB_SETCURSEL} $AIMode 0
  ${NSD_CreateLabel} 0u 116u 100% 24u "$(lumiaAINoModel)"
  Pop $0
  Call LumiaThemePage
  Push 0
  Call UpdateAIHint
  nsDialogs::Show
FunctionEnd
Function UpdateAIHint
  Pop $0
  SendMessage $AIProviderControl ${CB_GETCURSEL} 0 0 $0
  ${If} $0 == 0
    ${NSD_SetText} $AIHint "$(lumiaAIDownloadHint)"
  ${ElseIf} $0 == 3
    ${NSD_SetText} $AIHint "$(lumiaAIKeepHint)"
  ${Else}
    ${NSD_SetText} $AIHint "$(lumiaAIExistingHint)"
  ${EndIf}
FunctionEnd
Function PageLeaveLocalAI
  SendMessage $AIProviderControl ${CB_GETCURSEL} 0 0 $AIProvider
  SendMessage $AIModeControl ${CB_GETCURSEL} 0 0 $AIMode
  StrCpy $LocalAIConfirmed 1
FunctionEnd

Function PageMaxPlugin
  Call SkipIfPassive
  ${If} $InstallMaxPlugin <> ${BST_CHECKED}
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "$(lumiaMaxTitle)" "$(lumiaMaxSubtitle)"
  nsDialogs::Create 1018
  Pop $MaxPluginDialog
  ${If} $MaxPluginDialog == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0u 0u 100% 14u "$(lumiaMaxQuestion)"
  Pop $0
  ${NSD_CreateRadioButton} 0u 22u 100% 17u "$(lumiaMaxAutomatic)"
  Pop $MaxAutomaticControl
  ${NSD_CreateLabel} 12u 43u 286u 23u "$(lumiaMaxAutomaticHint)"
  Pop $0
  ${NSD_CreateRadioButton} 0u 73u 100% 17u "$(lumiaMaxManual)"
  Pop $MaxManualControl
  ${NSD_CreateLabel} 12u 94u 286u 27u "$(lumiaMaxManualHint)"
  Pop $0
  ${If} $MaxAutoLoad == 1
    ${NSD_Check} $MaxAutomaticControl
  ${Else}
    ${NSD_Check} $MaxManualControl
  ${EndIf}
  ${NSD_CreateLabel} 0u 128u 100% 12u "$(lumiaMaxRenderHint)"
  Pop $0
  Call LumiaThemePage
  nsDialogs::Show
FunctionEnd
Function PageLeaveMaxPlugin
  ${NSD_GetState} $MaxAutomaticControl $MaxAutoLoad
FunctionEnd

Function InstallSelectedLocalAI
  ${If} ${Silent}
  ${OrIf} $PassiveMode == 1
    ; Etkileşimsiz güncelleme kayıtlı AI seçimini ve bütün dosyaları korur.
    Return
  ${EndIf}
  SetShellVarContext current
  ${If} $InstallLocalAI <> ${BST_CHECKED}
    ; Ret seçimi AI yardımcısını veya ağı çalıştırmaz; model/ayar/runtime korunur.
    WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "LocalAIRequested" 0
    Delete "$LOCALAPPDATA\LumiaDAM\local-ai\install-requested.json"
  ${ElseIf} $LocalAIConfirmed == 1
    local_ai_retry:
    StrCpy $0 "keep"
    StrCpy $1 "keep"
    StrCpy $2 "-ConfigureOnly"
    ${If} $AIProvider == 0
      StrCpy $0 "bundled"
      StrCpy $2 ""
      DetailPrint "$(lumiaAIDownloadProgress)"
    ${ElseIf} $AIProvider == 1
      StrCpy $0 "ollama"
    ${ElseIf} $AIProvider == 2
      StrCpy $0 "lmstudio"
    ${EndIf}
    ${If} $AIMode == 0
      StrCpy $1 "disabled"
    ${ElseIf} $AIMode == 1
      StrCpy $1 "manual"
    ${EndIf}
    DetailPrint "$(lumiaAISaving)"
    ; Yalnız paketin içerdiği sabit script çalışır; uzaktan script veya Setup.exe yürütülmez.
    nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "$INSTDIR\integrations\local-ai\install-runtime.ps1" -Provider $0 -Mode $1 -Language $InstallLanguage $2'
    Pop $LocalAIResult
    ${If} $LocalAIResult == 0
      WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "LocalAIRequested" 1
      DetailPrint "$(lumiaAISaved)"
    ${Else}
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(lumiaAIError)" /SD IDCANCEL IDRETRY local_ai_retry
      StrCpy $InstallLocalAI 0
      WriteRegDWORD HKCU "${MANUPRODUCTKEY}" "LocalAIRequested" 0
      Delete "$LOCALAPPDATA\LumiaDAM\local-ai\install-requested.json"
      DetailPrint "$(lumiaAIContinue)"
    ${EndIf}
  ${EndIf}
  ; Sessiz/pasif güncelleme saklanan tercihi korur; eksik runtime için indirme başlatmaz.
  !insertmacro SetContext
FunctionEnd

Function SaveInstallerLanguage
  ${If} $LanguageConfirmed != 1
    Return
  ${EndIf}
  language_retry:
  DetailPrint "$(lumiaLanguageSaving)"
  ; Dil, AI seçilmese de kaydedilir; bu yol runtime/model dizinine dokunmaz.
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "$INSTDIR\integrations\local-ai\install-runtime.ps1" -LanguageOnly -Language $InstallLanguage'
  Pop $LocalAIResult
  ${If} $LocalAIResult != 0
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(lumiaLanguageError)" /SD IDCANCEL IDRETRY language_retry
    Abort
  ${EndIf}
FunctionEnd

; 7. Installation page
!define MUI_PAGE_CUSTOMFUNCTION_SHOW LumiaThemePage
!insertmacro MUI_PAGE_INSTFILES

; 8. Finish page
;
; Don't auto jump to finish page after installation page,
; because the installation page has useful info that can be used debug any issues with the installer.
!define MUI_FINISHPAGE_NOAUTOCLOSE
; Use show readme button in the finish page as a button create a desktop shortcut
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(createDesktop)"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateOrUpdateDesktopShortcut
; Show run app after installation.
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION RunMainBinary
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!define MUI_PAGE_CUSTOMFUNCTION_SHOW LumiaThemePage
!define MUI_FINISHPAGE_TITLE "$(lumiaFinishTitle)"
!define MUI_FINISHPAGE_TEXT "$(lumiaFinishText)"
!insertmacro MUI_PAGE_FINISH

Function RunMainBinary
  nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" ""
FunctionEnd

; Uninstaller Pages
; 1. Confirm uninstall page (No delete app data checkbox to protect library databases)
!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.LumiaThemePage
!insertmacro MUI_UNPAGE_CONFIRM

; 2. Uninstalling Page
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.LumiaThemePage
!insertmacro MUI_UNPAGE_INSTFILES

;Languages
{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}
!insertmacro MUI_RESERVEFILE_LANGDLL
; Tauri yardımcılarının metinleri de kurulum diliyle birlikte değişir.
LangString addOrReinstall ${LANG_ENGLISH} "Add/Reinstall components"
LangString addOrReinstall ${LANG_TURKISH} "Bileşen ekle / yeniden kur"
LangString alreadyInstalled ${LANG_ENGLISH} "Already installed"
LangString alreadyInstalled ${LANG_TURKISH} "Zaten kurulu"
LangString alreadyInstalledLong ${LANG_ENGLISH} "${PRODUCTNAME} ${VERSION} is already installed. Choose an operation and click Next."
LangString alreadyInstalledLong ${LANG_TURKISH} "${PRODUCTNAME} ${VERSION} zaten kurulu. Bir işlem seçip İleri düğmesine tıklayın."
LangString appRunning ${LANG_ENGLISH} "${PRODUCTNAME} is running. Close it and try again."
LangString appRunning ${LANG_TURKISH} "${PRODUCTNAME} çalışıyor. Uygulamayı kapatıp yeniden deneyin."
LangString appRunningOkKill ${LANG_ENGLISH} "${PRODUCTNAME} is running.$\nClick OK to close it."
LangString appRunningOkKill ${LANG_TURKISH} "${PRODUCTNAME} çalışıyor.$\nKapatmak için Tamam düğmesine tıklayın."
LangString chooseMaintenanceOption ${LANG_ENGLISH} "Choose a maintenance operation."
LangString chooseMaintenanceOption ${LANG_TURKISH} "Bir bakım işlemi seçin."
LangString choowHowToInstall ${LANG_ENGLISH} "Choose how to install ${PRODUCTNAME}."
LangString choowHowToInstall ${LANG_TURKISH} "${PRODUCTNAME} uygulamasının nasıl kurulacağını seçin."
LangString createDesktop ${LANG_ENGLISH} "Create desktop shortcut"
LangString createDesktop ${LANG_TURKISH} "Masaüstü kısayolu oluştur"
LangString dontUninstall ${LANG_ENGLISH} "Do not uninstall"
LangString dontUninstall ${LANG_TURKISH} "Mevcut kurulumu kaldırma"
LangString dontUninstallDowngrade ${LANG_ENGLISH} "Do not uninstall (downgrading without uninstalling is disabled)"
LangString dontUninstallDowngrade ${LANG_TURKISH} "Kaldırma (kaldırmadan eski sürüme geçiş devre dışı)"
LangString failedToKillApp ${LANG_ENGLISH} "${PRODUCTNAME} could not be closed. Close it and try again."
LangString failedToKillApp ${LANG_TURKISH} "${PRODUCTNAME} kapatılamadı. Uygulamayı kapatıp yeniden deneyin."
LangString installingWebview2 ${LANG_ENGLISH} "Installing WebView2..."
LangString installingWebview2 ${LANG_TURKISH} "WebView2 kuruluyor..."
LangString newerVersionInstalled ${LANG_ENGLISH} "A newer version of ${PRODUCTNAME} is installed. To install this older version, choose an operation and click Next."
LangString newerVersionInstalled ${LANG_TURKISH} "${PRODUCTNAME} uygulamasının daha yeni bir sürümü kurulu. Bu eski sürümü kurmak için bir işlem seçip İleri düğmesine tıklayın."
LangString older ${LANG_ENGLISH} "older"
LangString older ${LANG_TURKISH} "eski"
LangString olderOrUnknownVersionInstalled ${LANG_ENGLISH} "An $R4 version of ${PRODUCTNAME} is installed. Choose an operation and click Next."
LangString olderOrUnknownVersionInstalled ${LANG_TURKISH} "${PRODUCTNAME} uygulamasının $R4 bir sürümü kurulu. Bir işlem seçip İleri düğmesine tıklayın."
LangString silentDowngrades ${LANG_ENGLISH} "Silent downgrades are disabled. Run the installer with its graphical interface.$\n"
LangString silentDowngrades ${LANG_TURKISH} "Sessiz kurulumda eski sürüme geçilemez. Kurulumu grafik arayüzüyle çalıştırın.$\n"
LangString unableToUninstall ${LANG_ENGLISH} "The previous installation could not be removed."
LangString unableToUninstall ${LANG_TURKISH} "Önceki kurulum kaldırılamadı."
LangString uninstallApp ${LANG_ENGLISH} "Uninstall ${PRODUCTNAME}"
LangString uninstallApp ${LANG_TURKISH} "${PRODUCTNAME} uygulamasını kaldır"
LangString uninstallBeforeInstalling ${LANG_ENGLISH} "Uninstall before installing"
LangString uninstallBeforeInstalling ${LANG_TURKISH} "Kurulumdan önce mevcut sürümü kaldır"
LangString unknown ${LANG_ENGLISH} "unknown"
LangString unknown ${LANG_TURKISH} "bilinmeyen"
LangString webview2AbortError ${LANG_ENGLISH} "WebView2 could not be installed. The application requires it. Try running setup again."
LangString webview2AbortError ${LANG_TURKISH} "WebView2 kurulamadı. Uygulamanın çalışması için gereklidir. Kurulumu yeniden çalıştırın."
LangString webview2DownloadError ${LANG_ENGLISH} "WebView2 download failed: $0"
LangString webview2DownloadError ${LANG_TURKISH} "WebView2 indirilemedi: $0"
LangString webview2DownloadSuccess ${LANG_ENGLISH} "WebView2 installer downloaded."
LangString webview2DownloadSuccess ${LANG_TURKISH} "WebView2 kurulum dosyası indirildi."
LangString webview2Downloading ${LANG_ENGLISH} "Downloading WebView2 installer..."
LangString webview2Downloading ${LANG_TURKISH} "WebView2 kurulum dosyası indiriliyor..."
LangString webview2InstallError ${LANG_ENGLISH} "WebView2 installation failed with exit code $1."
LangString webview2InstallError ${LANG_TURKISH} "WebView2 kurulumu $1 hata koduyla başarısız oldu."
LangString webview2InstallSuccess ${LANG_ENGLISH} "WebView2 installed."
LangString webview2InstallSuccess ${LANG_TURKISH} "WebView2 kuruldu."
LangString deleteAppData ${LANG_ENGLISH} "Delete application data"
LangString deleteAppData ${LANG_TURKISH} "Uygulama verilerini sil"

; Özel kurulum metinleri; standart sayfalar NSIS dil paketlerinden gelir.
LangString lumiaBranding ${LANG_ENGLISH} "LumiaDAM  /  Creative assets, connected."
LangString lumiaBranding ${LANG_TURKISH} "LumiaDAM  /  Yaratıcı varlıklar bir arada."
LangString lumiaWelcomeTitle ${LANG_ENGLISH} "LumiaDAM Setup"
LangString lumiaWelcomeTitle ${LANG_TURKISH} "LumiaDAM Kurulumu"
LangString lumiaWelcomeSubtitle ${LANG_ENGLISH} "Your creative workspace starts here."
LangString lumiaWelcomeSubtitle ${LANG_TURKISH} "Yaratıcı çalışma alanınız burada başlıyor."
LangString lumiaWelcomeTagline ${LANG_ENGLISH} "A home for your creative library."
LangString lumiaWelcomeTagline ${LANG_TURKISH} "Yaratıcı arşiviniz için bir çalışma alanı."
LangString lumiaOrganize ${LANG_ENGLISH} "01  ORGANIZE"
LangString lumiaOrganize ${LANG_TURKISH} "01  DÜZENLE"
LangString lumiaCreate ${LANG_ENGLISH} "02  CREATE"
LangString lumiaCreate ${LANG_TURKISH} "02  ÜRET"
LangString lumiaConnect ${LANG_ENGLISH} "03  CONNECT"
LangString lumiaConnect ${LANG_TURKISH} "03  BAĞLAN"
LangString lumiaOrganizeText ${LANG_ENGLISH} "Find assets across your libraries."
LangString lumiaOrganizeText ${LANG_TURKISH} "Kütüphanelerinizdeki varlıkları bulun."
LangString lumiaCreateText ${LANG_ENGLISH} "Open previews and built-in editors."
LangString lumiaCreateText ${LANG_TURKISH} "Önizlemeleri ve yerleşik düzenleyicileri açın."
LangString lumiaConnectText ${LANG_ENGLISH} "Add optional AI and 3ds Max tools."
LangString lumiaConnectText ${LANG_TURKISH} "İsteğe bağlı AI ve 3ds Max araçlarını ekleyin."
LangString lumiaVersion ${LANG_ENGLISH} "WINDOWS 64-BIT     /     VERSION ${VERSION}"
LangString lumiaVersion ${LANG_TURKISH} "WINDOWS 64 BİT     /     SÜRÜM ${VERSION}"
LangString lumiaMaxInstallError ${LANG_ENGLISH} "The 3ds Max plugin could not be installed. Check access to your Autodesk ApplicationPlugins folder and run the installer again."
LangString lumiaMaxInstallError ${LANG_TURKISH} "3ds Max eklentisi kurulamadı. Autodesk ApplicationPlugins klasörüne erişimi kontrol edip kurulumu yeniden çalıştırın."
LangString lumiaComponentsTitle ${LANG_ENGLISH} "Customize your workspace"
LangString lumiaComponentsTitle ${LANG_TURKISH} "Çalışma alanınızı özelleştirin"
LangString lumiaComponentsSubtitle ${LANG_ENGLISH} "Choose your tools. Open Details to learn what each one adds."
LangString lumiaComponentsSubtitle ${LANG_TURKISH} "Araçlarınızı seçin. Neler sunduklarını Ayrıntılar bölümünde inceleyin."
LangString lumiaLocalAI ${LANG_ENGLISH} "Local AI"
LangString lumiaLocalAI ${LANG_TURKISH} "Yerel AI"
LangString lumiaAISummary ${LANG_ENGLISH} "Private suggestions for a more organized library."
LangString lumiaAISummary ${LANG_TURKISH} "Daha düzenli bir kütüphane için yerel öneriler."
LangString lumiaDetailsMore ${LANG_ENGLISH} "Details +"
LangString lumiaDetailsMore ${LANG_TURKISH} "Ayrıntılar +"
LangString lumiaDetailsLess ${LANG_ENGLISH} "Details -"
LangString lumiaDetailsLess ${LANG_TURKISH} "Ayrıntılar -"
LangString lumiaAIDescription ${LANG_ENGLISH} "Analyzes local previews and proposes organization changes for your approval. Choose a private Ollama runtime or your existing Ollama / LM Studio. Optional runtime download: 1.47 GB; 8 GB free space during setup. Models are chosen and downloaded later."
LangString lumiaAIDescription ${LANG_TURKISH} "Yerel önizlemeleri analiz eder ve onayınıza sunulan düzenleme önerileri oluşturur. Özel Ollama runtime veya mevcut Ollama / LM Studio kullanılabilir. İsteğe bağlı indirme: 1,47 GB; kurulumda 8 GB boş alan gerekir. Modeller daha sonra seçilip indirilir."
LangString lumiaMaxPlugin ${LANG_ENGLISH} "3ds Max plugin"
LangString lumiaMaxPlugin ${LANG_TURKISH} "3ds Max eklentisi"
LangString lumiaMaxSummary ${LANG_ENGLISH} "Bring library models into your working scene."
LangString lumiaMaxSummary ${LANG_TURKISH} "Kütüphanedeki modelleri sahnenize aktarın."
LangString lumiaMaxDescription ${LANG_ENGLISH} "Merge MAX models and ZIP/RAR archives, with available textures copied to your project. Supports 3ds Max 2022-2027. RAR requires WinRAR/UnRAR. Render engines are not included. Restart Max after installing the plugin."
LangString lumiaMaxDescription ${LANG_TURKISH} "MAX modellerini ve ZIP/RAR arşivlerini sahneye ekler; bulunan dokuları projenize kopyalar. 3ds Max 2022-2027 desteklenir. RAR için WinRAR/UnRAR gerekir. Render motorları dahil değildir. Eklentiden sonra Max uygulamasını yeniden başlatın."
LangString lumiaAITitle ${LANG_ENGLISH} "Set up Local AI"
LangString lumiaAITitle ${LANG_TURKISH} "Yerel AI kurulumu"
LangString lumiaAISubtitle ${LANG_ENGLISH} "Choose the engine and when it may work."
LangString lumiaAISubtitle ${LANG_TURKISH} "Motoru ve ne zaman çalışabileceğini seçin."
LangString lumiaAIEngineQuestion ${LANG_ENGLISH} "Which AI engine would you like to use?"
LangString lumiaAIEngineQuestion ${LANG_TURKISH} "Hangi AI motorunu kullanmak istersiniz?"
LangString lumiaAIPrivate ${LANG_ENGLISH} "Private Ollama runtime (download if needed)"
LangString lumiaAIPrivate ${LANG_TURKISH} "Özel Ollama runtime (gerekirse indirilir)"
LangString lumiaAIExistingOllama ${LANG_ENGLISH} "Use my existing Ollama"
LangString lumiaAIExistingOllama ${LANG_TURKISH} "Mevcut Ollama kurulumumu kullan"
LangString lumiaAIExistingLMStudio ${LANG_ENGLISH} "Use my existing LM Studio"
LangString lumiaAIExistingLMStudio ${LANG_TURKISH} "Mevcut LM Studio kurulumumu kullan"
LangString lumiaAIKeepEngine ${LANG_ENGLISH} "Keep my current AI engine settings"
LangString lumiaAIKeepEngine ${LANG_TURKISH} "Mevcut AI motoru ayarlarımı koru"
LangString lumiaAIModeQuestion ${LANG_ENGLISH} "When may Local AI run?"
LangString lumiaAIModeQuestion ${LANG_TURKISH} "Yerel AI ne zaman çalışabilsin?"
LangString lumiaAIDisabled ${LANG_ENGLISH} "Keep disabled until I enable it"
LangString lumiaAIDisabled ${LANG_TURKISH} "Ben etkinleştirene kadar kapalı kalsın"
LangString lumiaAIManual ${LANG_ENGLISH} "Only when I click Run now (manual)"
LangString lumiaAIManual ${LANG_TURKISH} "Yalnız Şimdi çalıştır dediğimde (manuel)"
LangString lumiaAIKeepSchedule ${LANG_ENGLISH} "Keep my current schedule"
LangString lumiaAIKeepSchedule ${LANG_TURKISH} "Mevcut çalışma planımı koru"
LangString lumiaAINoModel ${LANG_ENGLISH} "No model runs during setup. Configure models and idle/scheduled work in Menu > Local AI."
LangString lumiaAINoModel ${LANG_TURKISH} "Kurulumda model çalıştırılmaz. Model ve çalışma planını Menü > Yerel AI bölümünden ayarlayın."
LangString lumiaAIDownloadHint ${LANG_ENGLISH} "1.47 GB download if needed. Allow 8 GB free during setup; models need additional space. No Windows service or startup task is added."
LangString lumiaAIDownloadHint ${LANG_TURKISH} "Gerekirse 1,47 GB indirilir. Kurulumda 8 GB, modeller için ek boş alan gerekir. Windows servisi veya başlangıç görevi eklenmez."
LangString lumiaAIKeepHint ${LANG_ENGLISH} "Preserve the current engine, model and server address. No runtime download."
LangString lumiaAIKeepHint ${LANG_TURKISH} "Mevcut motor, model ve sunucu adresi korunur. Runtime indirilmez."
LangString lumiaAIExistingHint ${LANG_ENGLISH} "No runtime download. Start your existing local server when needed and choose its model in Menu > Local AI."
LangString lumiaAIExistingHint ${LANG_TURKISH} "Runtime indirilmez. Yerel sunucunuzu gerektiğinde başlatın; modeli Menü > Yerel AI bölümünden seçin."
LangString lumiaMaxTitle ${LANG_ENGLISH} "Connect to 3ds Max"
LangString lumiaMaxTitle ${LANG_TURKISH} "3ds Max bağlantısı"
LangString lumiaMaxSubtitle ${LANG_ENGLISH} "Choose how the plugin is installed for your Windows account."
LangString lumiaMaxSubtitle ${LANG_TURKISH} "Windows hesabınız için eklentinin kurulum biçimini seçin."
LangString lumiaMaxQuestion ${LANG_ENGLISH} "How would you like to set up the connection?"
LangString lumiaMaxQuestion ${LANG_TURKISH} "Bağlantıyı nasıl kurmak istersiniz?"
LangString lumiaMaxAutomatic ${LANG_ENGLISH} "Install and load automatically in 3ds Max"
LangString lumiaMaxAutomatic ${LANG_TURKISH} "Kur ve 3ds Max ile otomatik yükle"
LangString lumiaMaxAutomaticHint ${LANG_ENGLISH} "Registers the plugin for Max 2022-2027. Restart Max after setup."
LangString lumiaMaxAutomaticHint ${LANG_TURKISH} "Eklentiyi Max 2022-2027 için kaydeder. Kurulumdan sonra Max uygulamasını yeniden başlatın."
LangString lumiaMaxManual ${LANG_ENGLISH} "Keep plugin files for manual installation later"
LangString lumiaMaxManual ${LANG_TURKISH} "Eklenti dosyalarını daha sonra manuel kurmak için sakla"
LangString lumiaMaxManualHint ${LANG_ENGLISH} "Files stay in LumiaDAM's plugins folder. An existing automatic registration is removed. No other Max plugins are changed."
LangString lumiaMaxManualHint ${LANG_TURKISH} "Dosyalar LumiaDAM plugins klasöründe tutulur. Varsa önceki otomatik kayıt kaldırılır. Diğer Max eklentileri değiştirilmez."
LangString lumiaMaxRenderHint ${LANG_ENGLISH} "V-Ray / Corona and other render plugins are installed separately."
LangString lumiaMaxRenderHint ${LANG_TURKISH} "V-Ray / Corona ve diğer render eklentileri ayrıca kurulur."
LangString lumiaAIDownloadProgress ${LANG_ENGLISH} "Installing the private Ollama runtime (1.47 GB download if needed)..."
LangString lumiaAIDownloadProgress ${LANG_TURKISH} "Özel Ollama runtime kuruluyor (gerekirse 1,47 GB indirilir)..."
LangString lumiaAISaving ${LANG_ENGLISH} "Saving Local AI preferences..."
LangString lumiaAISaving ${LANG_TURKISH} "Yerel AI tercihleri kaydediliyor..."
LangString lumiaAISaved ${LANG_ENGLISH} "Local AI preferences saved. No model was started by setup. Open Menu > Local AI to review your configuration."
LangString lumiaAISaved ${LANG_TURKISH} "Yerel AI tercihleri kaydedildi. Kurulumda model başlatılmadı. Ayarları Menü > Yerel AI bölümünden inceleyin."
LangString lumiaAIError ${LANG_ENGLISH} "The optional Local AI runtime could not be installed. Check the installation details, network connection and free disk space.$\r$\n$\r$\nRetry downloads again. Cancel continues installing LumiaDAM without Local AI. You can run this installer later to add it."
LangString lumiaAIError ${LANG_TURKISH} "İsteğe bağlı Yerel AI kurulumu tamamlanamadı. Kurulum ayrıntılarını, bağlantıyı ve boş disk alanını kontrol edin.$\r$\n$\r$\nYeniden Dene işlemi tekrarlar. İptal, LumiaDAM kurulumunu Yerel AI eklemeden sürdürür. Daha sonra bu kurulumu tekrar çalıştırabilirsiniz."
LangString lumiaAIContinue ${LANG_ENGLISH} "Continuing LumiaDAM installation without the optional Local AI runtime."
LangString lumiaAIContinue ${LANG_TURKISH} "LumiaDAM kurulumuna isteğe bağlı Yerel AI eklenmeden devam ediliyor."
LangString lumiaFinishTitle ${LANG_ENGLISH} "Your workspace is ready."
LangString lumiaFinishTitle ${LANG_TURKISH} "Çalışma alanınız hazır."
LangString lumiaFinishText ${LANG_ENGLISH} "LumiaDAM is installed.$\r$\n$\r$\nAdd a library to get started. Configure models and scheduling from Menu > Local AI.$\r$\n$\r$\nIf you installed the 3ds Max plugin, restart Max to connect."
LangString lumiaFinishText ${LANG_TURKISH} "LumiaDAM kuruldu.$\r$\n$\r$\nBaşlamak için bir kütüphane ekleyin. Modelleri ve çalışma planını Menü > Yerel AI bölümünden ayarlayın.$\r$\n$\r$\n3ds Max eklentisini kurduysanız bağlantı için Max uygulamasını yeniden başlatın."
LangString lumiaAIKept ${LANG_ENGLISH} "Local AI runtime, downloaded models and settings are kept in $LOCALAPPDATA\LumiaDAM\local-ai."
LangString lumiaAIKept ${LANG_TURKISH} "Yerel AI runtime, indirilen modeller ve ayarlar $LOCALAPPDATA\LumiaDAM\local-ai dizininde korunur."
LangString lumiaOpenWith ${LANG_ENGLISH} "Open with ${PRODUCTNAME}"
LangString lumiaOpenWith ${LANG_TURKISH} "${PRODUCTNAME} ile aç"
LangString lumiaLanguageSaving ${LANG_ENGLISH} "Saving application language..."
LangString lumiaLanguageSaving ${LANG_TURKISH} "Uygulama dili kaydediliyor..."
LangString lumiaLanguageError ${LANG_ENGLISH} "The application language could not be saved. Existing preferences were preserved. Close LumiaDAM, check access to your local app data folder, and retry."
LangString lumiaLanguageError ${LANG_TURKISH} "Uygulama dili kaydedilemedi. Mevcut tercihler korundu. LumiaDAM uygulamasını kapatıp yerel uygulama verisi klasörüne erişimi kontrol edin ve yeniden deneyin."

Function .onInit
  StrCpy $LanguageConfirmed 0
  StrCpy $AIExpanded 0
  StrCpy $MaxExpanded 0
  StrCpy $AIProvider 0
  StrCpy $AIMode 0
  StrCpy $MaxAutoLoad 1
  ReadRegDWORD $MaxAutoLoad HKCU "${MANUPRODUCTKEY}" "MaxPluginAutoLoad"
  ${If} ${Errors}
    StrCpy $MaxAutoLoad 1
  ${EndIf}
  StrCpy $InstallMaxPlugin 0
  ReadRegDWORD $InstallMaxPlugin HKCU "${MANUPRODUCTKEY}" "MaxPluginRequested"
  ${If} ${Errors}
    ReadRegDWORD $InstallMaxPlugin HKCU "${MANUPRODUCTKEY}" "MaxPluginInstalled"
    ${If} ${Errors}
      StrCpy $InstallMaxPlugin 0
    ${EndIf}
  ${EndIf}
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/NS" $NoShortcutMode
  ${IfNot} ${Errors}
    StrCpy $NoShortcutMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}

  StrCpy $InstallLocalAI 0
  StrCpy $LocalAIConfirmed 0
  ClearErrors
  ReadRegDWORD $InstallLocalAI HKCU "${MANUPRODUCTKEY}" "LocalAIRequested"
  ${If} ${Errors}
  ${OrIf} $InstallLocalAI <> ${BST_CHECKED}
    StrCpy $InstallLocalAI 0
  ${EndIf}
  ${If} ${FileExists} "$LOCALAPPDATA\LumiaDAM\local-ai\settings.json"
    ; Güncellemede model, endpoint ve çalışma saati kullanıcı değiştirmedikçe korunur.
    StrCpy $AIProvider 3
    StrCpy $AIMode 2
  ${EndIf}

  !if "${DISPLAYLANGUAGESELECTOR}" == "true"
    ${IfNot} ${Silent}
    ${AndIf} $PassiveMode != 1
    ${AndIf} $UpdateMode != 1
      !insertmacro MUI_LANGDLL_DISPLAY
      StrCpy $LanguageConfirmed 1
    ${Else}
      ReadRegStr $0 HKCU "${MANUPRODUCTKEY}" "Installer Language"
      ${If} $0 != ""
        StrCpy $LANGUAGE $0
      ${EndIf}
    ${EndIf}
  !endif
  StrCpy $InstallLanguage "en"
  ${If} $LANGUAGE == ${LANG_TURKISH}
    StrCpy $InstallLanguage "tr"
  ${EndIf}

  !insertmacro SetContext

  ${If} $INSTDIR == "${PLACEHOLDER_INSTALL_DIR}"
    ; Set default install location
    !if "${INSTALLMODE}" == "perMachine"
      ${If} ${RunningX64}
        !if "${ARCH}" == "x64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else if "${ARCH}" == "arm64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else
          StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
        !endif
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
    !else if "${INSTALLMODE}" == "currentUser"
      StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
    !endif

    Call RestorePreviousInstallLocation
  ${EndIf}


  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_INIT
  !endif
FunctionEnd


Section EarlyChecks
  ; Abort silent installer if downgrades is disabled
  !if "${ALLOWDOWNGRADES}" == "false"
  ${If} ${Silent}
    ; If downgrading
    ${If} $R0 = -1
      System::Call 'kernel32::AttachConsole(i -1)i.r0'
      ${If} $0 <> 0
        System::Call 'kernel32::GetStdHandle(i -11)i.r0'
        System::call 'kernel32::SetConsoleTextAttribute(i r0, i 0x0004)' ; set red color
        FileWrite $0 "$(silentDowngrades)"
      ${EndIf}
      Abort
    ${EndIf}
  ${EndIf}
  !endif

SectionEnd

Section WebView2
  ; Check if Webview2 is already installed and skip this section
  ${If} ${RunningX64}
    ReadRegStr $4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${Else}
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $4 == ""
    ReadRegStr $4 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}

  ${If} $4 == ""
    ; Webview2 installation
    ;
    ; Skip if updating
    ${If} $UpdateMode <> 1
      !if "${INSTALLWEBVIEW2MODE}" == "downloadBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        DetailPrint "$(webview2Downloading)"
        NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Pop $0
        ${If} $0 == "success"
          DetailPrint "$(webview2DownloadSuccess)"
        ${Else}
          DetailPrint "$(webview2DownloadError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "embedBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
        Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        Goto install_webview2
      !endif

      Goto webview2_done

      install_webview2:
        DetailPrint "$(installingWebview2)"
        ; $6 holds the path to the webview2 installer
        ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1
        ${If} $1 = 0
          DetailPrint "$(webview2InstallSuccess)"
        ${Else}
          DetailPrint "$(webview2InstallError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
      webview2_done:
    ${EndIf}
  ${Else}
    !if "${MINIMUMWEBVIEW2VERSION}" != ""
      ${VersionCompare} "${MINIMUMWEBVIEW2VERSION}" "$4" $R0
      ${If} $R0 = 1
        update_webview:
          DetailPrint "$(installingWebview2)"
          ${If} ${RunningX64}
            ReadRegStr $R1 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate" "path"
          ${Else}
            ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 == ""
            ReadRegStr $R1 HKCU "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 != ""
            ; Chromium updater docs: https://source.chromium.org/chromium/chromium/src/+/main:docs/updater/user_manual.md
            ; Modified from "HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Microsoft EdgeWebView\ModifyPath"
            ExecWait `"$R1" /install appguid=${WEBVIEW2APPGUID}&needsadmin=true` $1
            ${If} $1 = 0
              DetailPrint "$(webview2InstallSuccess)"
            ${Else}
              MessageBox MB_ICONEXCLAMATION|MB_ABORTRETRYIGNORE "$(webview2InstallError)" IDIGNORE ignore IDRETRY update_webview
              Quit
              ignore:
            ${EndIf}
          ${EndIf}
      ${EndIf}
    !endif
  ${EndIf}
SectionEnd

Section Install
  SetOutPath $INSTDIR

  !ifmacrodef NSIS_HOOK_PREINSTALL
    !insertmacro NSIS_HOOK_PREINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Copy main executable
  File "${MAINBINARYSRCPATH}"

  ; Copy resources
  {{#each resources_dirs}}
    CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
    File /a "/oname={{this.[1]}}" "{{no-escape @key}}"
  {{/each}}

  ; Copy external binaries
  Call SaveInstallerLanguage
  Call InstallSelectedMaxPlugin
  Call InstallSelectedLocalAI
  {{#each binaries}}
    File /a "/oname={{this}}" "{{no-escape @key}}"
  {{/each}}

  ; Create file associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
       !insertmacro APP_ASSOCIATE "{{ext}}" "{{or association.name ext}}" "{{association-description association.description ext}}" "$INSTDIR\${MAINBINARYNAME}.exe,0" "$(lumiaOpenWith)" "$INSTDIR\${MAINBINARYNAME}.exe $\"%1$\""
    {{/each}}
  {{/each}}

  ; Register deep links
  {{#each deep_link_protocols as |protocol| ~}}
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "URL Protocol" ""
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "" "URL:${BUNDLEID} protocol"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  {{/each}}

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Save $INSTDIR in registry for future installations
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR

  !if "${INSTALLMODE}" == "both"
    ; Save install mode to be selected by default for the next installation such as updating
    ; or when uninstalling
    WriteRegStr SHCTX "${UNINSTKEY}" $MultiUser.InstallMode 1
  !endif

  ; Remove old main binary if it doesn't match new main binary name
  ReadRegStr $OldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"
  ${If} $OldMainBinaryName != ""
  ${AndIf} $OldMainBinaryName != "${MAINBINARYNAME}.exe"
    Delete "$INSTDIR\$OldMainBinaryName"
  ${EndIf}

  ; Save current MAINBINARYNAME for future updates
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"

  ; Registry information for add/remove programs
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"

  ${GetSize} "$INSTDIR" "/M=uninstall.exe /S=0K /G=0" $0 $1 $2
  IntOp $0 $0 + ${ESTIMATEDSIZE}
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "$0"

  !if "${HOMEPAGE}" != ""
    WriteRegStr SHCTX "${UNINSTKEY}" "URLInfoAbout" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "URLUpdateInfo" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "HelpLink" "${HOMEPAGE}"
  !endif

  ; Create start menu shortcut
  !insertmacro MUI_STARTMENU_WRITE_BEGIN Application
    Call CreateOrUpdateStartMenuShortcut
  !insertmacro MUI_STARTMENU_WRITE_END

  ; Create desktop shortcut for silent and passive installers
  ; because finish page will be skipped
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    Call CreateOrUpdateDesktopShortcut
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTINSTALL
    !insertmacro NSIS_HOOK_POSTINSTALL
  !endif

  ; Auto close this page for passive mode
  ${If} $PassiveMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd

Function .onInstSuccess
  ; Check for `/R` flag only in silent and passive installers because
  ; GUI installer has a toggle for the user to (re)start the app
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetOptions} $CMDLINE "/R" $R0
    ${IfNot} ${Errors}
      ${GetOptions} $CMDLINE "/ARGS" $R0
      nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "$R0"
    ${EndIf}
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro SetContext

  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_UNINIT
  !endif

  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
  ${If} $PassiveMode == 1
  ${OrIf} $UpdateMode == 1
    ReadRegStr $0 HKCU "${MANUPRODUCTKEY}" "Installer Language"
    ${If} $0 != ""
      StrCpy $LANGUAGE $0
    ${EndIf}
  ${Else}
    !insertmacro MUI_UNGETLANGUAGE
  ${EndIf}
FunctionEnd

Section Uninstall

  !ifmacrodef NSIS_HOOK_PREUNINSTALL
    !insertmacro NSIS_HOOK_PREUNINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Delete the app directory and its content from disk
  ; Copy main executable
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"

  ; Delete resources
  DetailPrint "$(lumiaAIKept)"
  SetShellVarContext current
  ReadRegDWORD $0 HKCU "${MANUPRODUCTKEY}" "MaxPluginInstalled"
  ${If} $0 == 1
    Delete "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents\startup.ms"
    Delete "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents\lumiadam_max.py"
    Delete "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\PackageContents.xml"
    RMDir "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle\Contents"
    RMDir "$APPDATA\Autodesk\ApplicationPlugins\LumiaDAM.bundle"
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "MaxPluginInstalled"
  ${EndIf}
  !insertmacro SetContext
  {{#each resources}}
    Delete "$INSTDIR\\{{this.[1]}}"
  {{/each}}

  ; Delete external binaries
  {{#each binaries}}
    Delete "$INSTDIR\\{{this}}"
  {{/each}}

  ; Delete app associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
      !insertmacro APP_UNASSOCIATE "{{ext}}" "{{or association.name ext}}"
    {{/each}}
  {{/each}}

  ; Delete deep links
  {{#each deep_link_protocols as |protocol| ~}}
    ReadRegStr $R7 SHCTX "Software\Classes\\{{protocol}}\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey SHCTX "Software\Classes\\{{protocol}}"
    ${EndIf}
  {{/each}}


  ; Delete uninstaller
  Delete "$INSTDIR\uninstall.exe"

  {{#each resources_ancestors}}
  RMDir /REBOOTOK "$INSTDIR\\{{this}}"
  {{/each}}
  RMDir "$INSTDIR"

  ; Remove shortcuts if not updating
  ${If} $UpdateMode <> 1
    !insertmacro DeleteAppUserModelId

    ; Remove start menu shortcut
    !insertmacro MUI_STARTMENU_GETFOLDER Application $AppStartMenuFolder
    !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      RMDir "$SMPROGRAMS\$AppStartMenuFolder"
    ${EndIf}
    !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${EndIf}

    ; Remove desktop shortcuts
    !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$DESKTOP\${PRODUCTNAME}.lnk"
      Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}

  ; Remove registry information for add/remove programs
  !if "${INSTALLMODE}" == "both"
    DeleteRegKey SHCTX "${UNINSTKEY}"
  !else if "${INSTALLMODE}" == "perMachine"
    DeleteRegKey HKLM "${UNINSTKEY}"
  !else
    DeleteRegKey HKCU "${UNINSTKEY}"
  !endif

  ; Removes the Autostart entry for ${PRODUCTNAME} from the HKCU Run key if it exists.
  ; This ensures the program does not launch automatically after uninstallation if it exists.
  ; If it doesn't exist, it does nothing.
  ; We do this when not updating (to preserve the registry value on updates)
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  ${EndIf}

  ; Clear registry uninstall keys when not updating, but PRESERVE user databases and app data in %APPDATA%
  ${If} $UpdateMode <> 1
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"

    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTUNINSTALL
    !insertmacro NSIS_HOOK_POSTUNINSTALL
  !endif

  ; Auto close if passive mode or updating
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd

Function RestorePreviousInstallLocation
  ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $4 "" +2 0
    StrCpy $INSTDIR $4
FunctionEnd

Function Skip
  Abort
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd
Function un.SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd

Function CreateOrUpdateStartMenuShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  StrCpy $R0 0

  !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  ${If} $R0 = 1
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  !if "${STARTMENUFOLDER}" != ""
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !else
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !endif
FunctionEnd

Function CreateOrUpdateDesktopShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
FunctionEnd
