@echo off
setlocal EnableDelayedExpansion
chcp 65001 >nul 2>&1
title PitchBrick Installer

:: ================================================================
:: PitchBrick Installer for Windows 10/11 (x86_64)
:: Handles: Rust toolchain, Visual Studio Build Tools, and
::          cargo install pitchbrick with exhaustive error
::          handling for every known Windows edge case.
:: ================================================================

:: ================================================================
::  PHASE 0: BOOTSTRAP
:: ================================================================

:: Self-unblock MOTW (Mark of the Web) to prevent repeated SmartScreen prompts
powershell -NoProfile -Command "Unblock-File -LiteralPath '%~f0'" >nul 2>&1

:: Enable ANSI escape codes (Windows 10 1607+), save old value to restore later
for /f "tokens=3" %%a in ('reg query "HKCU\Console" /v VirtualTerminalLevel 2^>nul') do set "VT_WAS=%%a"
reg add "HKCU\Console" /v VirtualTerminalLevel /t REG_DWORD /d 1 /f >nul 2>&1

:: Generate the ESC character (0x1B) at runtime
for /f %%e in ('powershell -nop -c "[char]27"') do set "ESC=%%e"

:: ANSI color escape sequences
set "R=%ESC%[91m"
set "G=%ESC%[92m"
set "Y=%ESC%[93m"
set "C=%ESC%[96m"
set "W=%ESC%[97m"
set "B=%ESC%[1m"
set "D=%ESC%[2m"
set "N=%ESC%[0m"
set "BG_R=%ESC%[41m"
set "BG_G=%ESC%[42m"

:: Tracking variables
set "ISSUE_COUNT=0"
set "NEEDS_REBOOT=0"
set "IS_ADMIN=0"
set "LOG_FILE=%USERPROFILE%\pitchbrick-install.log"
set "CARGO_JOBS="
set "DEFENDER_ON=0"
set "DEFENDER_EXCLUDED=0"
set "FIX_LOW_DISK=0"
set "FIX_NO_INTERNET=0"
set "FIX_PENDING_REBOOT=0"
set "FIX_UNICODE=0"
set "FIX_LONGPATH=0"
set "FIX_RUST_CONFLICT=0"
set "FIX_BROKEN_RUSTUP=0"

:: Admin check
net session >nul 2>&1
if !ERRORLEVEL! EQU 0 set "IS_ADMIN=1"

:: ================================================================
::  WELCOME SCREEN
:: ================================================================
cls
echo.
echo   %B%%C%======================================================%N%
echo   %B%%C%              PITCHBRICK INSTALLER                    %N%
echo   %B%%C%======================================================%N%
echo.
echo   %B%%W%PitchBrick%N% is a tiny always-on-top window that watches
echo   your voice pitch in real time and tells you at a glance
echo   whether you're hitting your target range.
echo.
echo   When you're practicing a feminine or masculine voice, it's
echo   easy to drift without noticing %D%-- especially in VR and games.%N%
echo   PitchBrick sits in the corner of your screen %D%(over games,%N%
echo   %D%Discord calls, whatever)%N% and shows you a single color:
echo.
echo     %BG_G%%B%       %N%  %G%GREEN%N%  when your pitch is in range
echo     %BG_R%%B%       %N%  %R%RED%N%    when you've drifted out
echo     %W%       %N%  %D%BLACK%N%  when it can't hear you
echo.
echo   No graphs to read. No numbers to remember. Just a color
echo   you can glance at mid-conversation. It also works as a
echo   %C%SteamVR overlay%N% for VR pitch training.
echo.
echo   %D%This installer will set up everything you need:%N%
echo   %D%  - Visual Studio C++ Build Tools  (if missing)%N%
echo   %D%  - Rust programming language       (if missing)%N%
echo   %D%  - PitchBrick application%N%
echo.
echo   %B%%C%======================================================%N%
echo.
echo   %B%Press any key to begin installation...%N%
pause >nul

:: ================================================================
::  PHASE 1: SYSTEM DIAGNOSTICS
:: ================================================================
cls
echo.
echo   %B%%C%PHASE 1: Checking your system...%N%
echo   %C%------------------------------------------------------%N%
echo.

:: --- 1a. Windows version ---
for /f "tokens=4,5,6,7 delims=[]. " %%a in ('ver') do (
    set "VER_MAJOR=%%a"
    set "VER_MINOR=%%b"
    set "VER_BUILD=%%c"
    set "VER_REV=%%d"
)
set "WIN_NAME=Unknown"
if !VER_BUILD! GEQ 22000 (set "WIN_NAME=Windows 11") else if !VER_BUILD! GEQ 10240 (set "WIN_NAME=Windows 10")
echo   %G%[OK]%N%  !WIN_NAME! build !VER_BUILD!

:: --- 1b. Architecture (must be x86_64) ---
set "ARCH=%PROCESSOR_ARCHITECTURE%"
if defined PROCESSOR_ARCHITEW6432 set "ARCH=%PROCESSOR_ARCHITEW6432%"
if /i "!ARCH!"=="AMD64" (
    echo   %G%[OK]%N%  64-bit system ^(x86_64^)
) else (
    echo   %R%[FAIL]%N%  PitchBrick requires a 64-bit x86_64 system.
    echo          %D%Your system architecture: !ARCH!%N%
    echo.
    pause
    goto :cleanup
)

:: --- 1c. Admin status ---
if !IS_ADMIN! EQU 1 (
    echo   %G%[OK]%N%  Running as Administrator
) else (
    echo   %Y%[!!]%N%  Not running as Administrator
    echo          %D%Some fixes may require a permission prompt.%N%
)

:: --- 1d. Disk space ---
set "FREE_GB=0"
for /f %%G in ('powershell -NoProfile -Command "[math]::Floor((Get-CimInstance Win32_LogicalDisk -Filter 'DeviceID=''%SystemDrive%''').FreeSpace/1GB)"') do set "FREE_GB=%%G"
if !FREE_GB! GEQ 10 (
    echo   %G%[OK]%N%  !FREE_GB! GB free on %SystemDrive%
) else if !FREE_GB! GEQ 5 (
    echo   %Y%[!!]%N%  Only !FREE_GB! GB free on %SystemDrive% -- might be tight
) else (
    echo   %R%[!!]%N%  Only !FREE_GB! GB free on %SystemDrive%
    set /a ISSUE_COUNT+=1
    set "ISSUE_!ISSUE_COUNT!_DESC=Low disk space: only !FREE_GB! GB free. Rust and the build tools need around 5-8 GB."
    set "FIX_LOW_DISK=1"
)

:: --- 1e. Internet connectivity ---
set "NET_OK=0"
powershell -NoProfile -Command "[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; try { $null=Invoke-WebRequest -Uri 'https://static.rust-lang.org' -Method Head -UseBasicParsing -TimeoutSec 10; exit 0 } catch { if ($_.Exception.Response) { exit 0 } else { exit 1 } }" >nul 2>&1
if !ERRORLEVEL! EQU 0 (
    set "NET_OK=1"
    echo   %G%[OK]%N%  Internet connection works ^(HTTPS to Rust servers^)
) else (
    echo   %R%[!!]%N%  Cannot reach Rust download servers
    set /a ISSUE_COUNT+=1
    set "ISSUE_!ISSUE_COUNT!_DESC=Cannot connect to static.rust-lang.org over HTTPS. Check your internet, firewall, or proxy."
    set "FIX_NO_INTERNET=1"
)

:: --- 1f. Pending reboot ---
set "REBOOT_PENDING=0"
reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending" >nul 2>&1
if !ERRORLEVEL! EQU 0 set "REBOOT_PENDING=1"
reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired" >nul 2>&1
if !ERRORLEVEL! EQU 0 set "REBOOT_PENDING=1"
if !REBOOT_PENDING! EQU 1 (
    echo   %Y%[!!]%N%  A Windows restart is pending
    set /a ISSUE_COUNT+=1
    set "ISSUE_!ISSUE_COUNT!_DESC=Windows has updates waiting for a restart. This can sometimes cause installers to fail."
    set "FIX_PENDING_REBOOT=1"
) else (
    echo   %G%[OK]%N%  No pending restart
)

:: --- 1g. Unicode username check ---
set "BAD_PATH=0"
powershell -NoProfile -Command "if ($env:USERPROFILE -match '[^\x20-\x7E]') { exit 1 } else { exit 0 }" >nul 2>&1
if !ERRORLEVEL! NEQ 0 set "BAD_PATH=1"
if !BAD_PATH! EQU 1 (
    echo   %Y%[!!]%N%  Username contains special characters
    set /a ISSUE_COUNT+=1
    set "ISSUE_!ISSUE_COUNT!_DESC=Your Windows username has non-English characters. Rust tools can fail with these paths. Rust will be installed to C:\Rust instead."
    set "FIX_UNICODE=1"
) else (
    echo   %G%[OK]%N%  Username path is compatible
)

:: --- 1h. Long path support ---
set "LONGPATH=0"
for /f "tokens=3" %%a in ('reg query "HKLM\SYSTEM\CurrentControlSet\Control\FileSystem" /v LongPathsEnabled 2^>nul') do (
    if "%%a"=="0x1" set "LONGPATH=1"
)
if !LONGPATH! EQU 1 (
    echo   %G%[OK]%N%  Long path support enabled
) else (
    echo   %Y%[!!]%N%  Long path support is disabled
    set /a ISSUE_COUNT+=1
    set "ISSUE_!ISSUE_COUNT!_DESC=Windows limits paths to 260 characters. Some Rust dependencies have deeply nested folders. Enabling long paths prevents build errors."
    set "FIX_LONGPATH=1"
)

:: --- 1i. Existing Rust + CARGO_BIN setup ---
set "RUST_INSTALLED=0"
set "CARGO_BIN=%USERPROFILE%\.cargo\bin"
if !BAD_PATH! EQU 1 (
    if defined CARGO_HOME (
        set "CARGO_BIN=!CARGO_HOME!\bin"
    ) else (
        set "CARGO_BIN=C:\Rust\cargo\bin"
    )
)
where rustup >nul 2>&1
if !ERRORLEVEL! EQU 0 (
    set "RUST_INSTALLED=1"
    for /f "tokens=*" %%v in ('rustup --version 2^>nul') do echo   %G%[OK]%N%  Rust already installed: %%v
) else if exist "!CARGO_BIN!\rustup.exe" (
    set "RUST_INSTALLED=1"
    echo   %G%[OK]%N%  Rust found at !CARGO_BIN!
) else (
    echo   %Y%[--]%N%  Rust not installed %D%^(will be installed^)%N%
)

:: --- 1j. Conflicting Rust installs ---
set "RUSTC_COUNT=0"
for /f "tokens=*" %%p in ('where rustc 2^>nul') do set /a RUSTC_COUNT+=1
if !RUSTC_COUNT! GTR 1 (
    echo   %R%[!!]%N%  Multiple Rust compilers found in PATH
    set /a ISSUE_COUNT+=1
    set "ISSUE_!ISSUE_COUNT!_DESC=Multiple rustc.exe found in PATH. This confuses rustup and cargo. You may have Rust installed via Chocolatey, Scoop, or another method alongside rustup."
    set "FIX_RUST_CONFLICT=1"
) else if !RUST_INSTALLED! EQU 1 (
    echo   %G%[OK]%N%  No conflicting Rust installations
)

:: --- 1k. MSVC / Visual Studio Build Tools ---
set "MSVC_OK=0"
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "!VSWHERE!" (
    for /f "usebackq tokens=*" %%i in (`"!VSWHERE!" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2^>nul`) do (
        set "MSVC_OK=1"
        echo   %G%[OK]%N%  MSVC Build Tools found: %%i
    )
)
if !MSVC_OK! EQU 0 (
    echo   %Y%[--]%N%  Visual Studio C++ Build Tools not found %D%^(will be installed^)%N%
)

:: --- 1l. Existing PitchBrick ---
set "PB_INSTALLED=0"
where pitchbrick >nul 2>&1
if !ERRORLEVEL! EQU 0 (
    set "PB_INSTALLED=1"
    echo   %G%[OK]%N%  PitchBrick is already installed %D%^(will be updated^)%N%
) else if exist "!CARGO_BIN!\pitchbrick.exe" (
    set "PB_INSTALLED=1"
    echo   %G%[OK]%N%  PitchBrick found at !CARGO_BIN! %D%^(will be updated^)%N%
) else (
    echo   %Y%[--]%N%  PitchBrick not installed %D%^(will be installed^)%N%
)

:: --- 1m. Windows Defender ---
powershell -NoProfile -Command "try { if ((Get-MpComputerStatus).RealTimeProtectionEnabled) { exit 1 } else { exit 0 } } catch { exit 0 }" >nul 2>&1
if !ERRORLEVEL! EQU 1 set "DEFENDER_ON=1"
if !DEFENDER_ON! EQU 1 (
    echo   %D%[i ]  Windows Defender is active%N%
)

:: --- 1n. Broken/corrupt rustup ---
set "RUSTUP_DIR=%USERPROFILE%\.rustup"
if defined RUSTUP_HOME set "RUSTUP_DIR=!RUSTUP_HOME!"
if exist "!RUSTUP_DIR!\settings.toml" (
    set "HAS_TOOLCHAIN=0"
    for /d %%d in ("!RUSTUP_DIR!\toolchains\*") do set "HAS_TOOLCHAIN=1"
    if !HAS_TOOLCHAIN! EQU 0 (
        echo   %R%[!!]%N%  Corrupt Rust installation detected
        set /a ISSUE_COUNT+=1
        set "ISSUE_!ISSUE_COUNT!_DESC=Rust config files exist but toolchain files are missing. A previous install was interrupted. The installer will clean up and reinstall."
        set "FIX_BROKEN_RUSTUP=1"
    )
)

echo.

:: ================================================================
::  PHASE 2: ISSUE RESOLUTION
:: ================================================================
if !ISSUE_COUNT! GTR 0 (
    echo   %B%%Y%Found !ISSUE_COUNT! issue^(s^) that should be fixed first:%N%
    echo   %Y%------------------------------------------------------%N%
    echo.

    for /L %%i in (1,1,!ISSUE_COUNT!) do (
        call set "DESC=%%ISSUE_%%i_DESC%%"
        echo   %B%%Y%%%i.%N% !DESC!
        echo.
    )

    echo   %B%%C%What would you like to do?%N%
    echo.
    echo     %B%%G%[F]%N%  Fix all issues automatically and continue
    echo     %B%%R%[C]%N%  Cancel installation
    echo.
    choice /c FC /n /m "  Press F to fix, or C to cancel: "
    if !ERRORLEVEL! EQU 2 (
        echo.
        echo   %Y%Installation cancelled.%N%
        goto :cleanup
    )

    echo.
    echo   %B%%C%Fixing issues...%N%
    echo.

    if !FIX_LOW_DISK! EQU 1 (
        echo   %Y%[!!]%N%  Low disk space: please free up space if the install fails.
    )

    if !FIX_NO_INTERNET! EQU 1 (
        echo   %R%[!!]%N%  No internet connection. The installer needs to download files.
        echo          Please check your connection, firewall, and proxy, then try again.
        echo.
        pause
        goto :cleanup
    )

    if !FIX_PENDING_REBOOT! EQU 1 (
        echo   %Y%[!!]%N%  Pending restart noted. Continuing anyway...
        echo          %D%If the install fails, restart Windows and try again.%N%
    )

    if !FIX_UNICODE! EQU 1 (
        echo   %G%[FIX]%N% Setting Rust install location to C:\Rust
        if not exist "C:\Rust" mkdir "C:\Rust" 2>nul
        if not exist "C:\Rust\cargo" mkdir "C:\Rust\cargo" 2>nul
        if not exist "C:\Rust\rustup" mkdir "C:\Rust\rustup" 2>nul
        set "CARGO_HOME=C:\Rust\cargo"
        set "RUSTUP_HOME=C:\Rust\rustup"
        set "CARGO_BIN=C:\Rust\cargo\bin"
        set "RUSTUP_DIR=C:\Rust\rustup"
        setx CARGO_HOME "C:\Rust\cargo" >nul 2>&1
        setx RUSTUP_HOME "C:\Rust\rustup" >nul 2>&1
        set "LOG_FILE=C:\Rust\pitchbrick-install.log"
        echo          %D%CARGO_HOME  = C:\Rust\cargo%N%
        echo          %D%RUSTUP_HOME = C:\Rust\rustup%N%
    )

    if !FIX_LONGPATH! EQU 1 (
        if !IS_ADMIN! EQU 1 (
            reg add "HKLM\SYSTEM\CurrentControlSet\Control\FileSystem" /v LongPathsEnabled /t REG_DWORD /d 1 /f >nul 2>&1
            if !ERRORLEVEL! EQU 0 (
                echo   %G%[FIX]%N% Long path support enabled
            ) else (
                echo   %Y%[!!]%N%  Could not enable long paths
            )
        ) else (
            echo   %Y%[!!]%N%  Long paths: requesting admin permission...
            powershell -NoProfile -Command "Start-Process cmd -ArgumentList '/c reg add \"HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\" /v LongPathsEnabled /t REG_DWORD /d 1 /f' -Verb RunAs -Wait" >nul 2>&1
            if !ERRORLEVEL! EQU 0 (
                echo   %G%[FIX]%N% Long path support enabled
            ) else (
                echo   %Y%[!!]%N%  Skipped -- you can enable this later in Windows settings.
            )
        )
    )

    if !FIX_RUST_CONFLICT! EQU 1 (
        echo   %Y%[!!]%N%  Multiple Rust compilers found:
        for /f "tokens=*" %%p in ('where rustc 2^>nul') do (
            echo          %D%%%p%N%
        )
        echo          %D%If installed via Chocolatey: choco uninstall rust%N%
        echo          %D%If installed via Scoop:      scoop uninstall rust%N%
        echo          %D%Keep only the rustup-managed installation.%N%
        echo          %Y%Continuing anyway -- this may cause problems.%N%
    )

    if !FIX_BROKEN_RUSTUP! EQU 1 (
        echo   %G%[FIX]%N% Cleaning up corrupt Rust installation...
        if exist "!RUSTUP_DIR!\toolchains" rmdir /s /q "!RUSTUP_DIR!\toolchains" 2>nul
        if exist "!RUSTUP_DIR!\update-hashes" rmdir /s /q "!RUSTUP_DIR!\update-hashes" 2>nul
        echo          %D%Stale files removed. Fresh install will proceed.%N%
        set "RUST_INSTALLED=0"
    )

    echo.
)

:: ================================================================
::  DEFENDER EXCLUSION PROMPT
:: ================================================================
if !DEFENDER_ON! EQU 1 (
    echo   %B%%C%Windows Defender can slow Rust compilation by 3-10x.%N%
    echo   %D%Adding exclusions for Rust folders speeds up builds significantly.%N%
    echo.
    echo     %B%%G%[Y]%N%  Yes, speed up builds ^(add Defender exclusions^)
    echo     %B%%W%[N]%N%  No, keep default security settings
    echo.
    choice /c YN /n /m "  Press Y or N: "
    if !ERRORLEVEL! EQU 1 (
        echo.
        if !IS_ADMIN! EQU 1 (
            echo   %D%Adding antivirus exclusions...%N%
            if defined CARGO_HOME (
                powershell -NoProfile -Command "Add-MpPreference -ExclusionPath '!CARGO_HOME!' -ErrorAction SilentlyContinue" >nul 2>&1
                powershell -NoProfile -Command "Add-MpPreference -ExclusionPath '!RUSTUP_DIR!' -ErrorAction SilentlyContinue" >nul 2>&1
            ) else (
                powershell -NoProfile -Command "Add-MpPreference -ExclusionPath \"$env:USERPROFILE\.cargo\" -ErrorAction SilentlyContinue" >nul 2>&1
                powershell -NoProfile -Command "Add-MpPreference -ExclusionPath \"$env:USERPROFILE\.rustup\" -ErrorAction SilentlyContinue" >nul 2>&1
            )
            powershell -NoProfile -Command "Add-MpPreference -ExclusionProcess 'cargo.exe','rustc.exe','link.exe' -ErrorAction SilentlyContinue" >nul 2>&1
            :: Verify (Tamper Protection can silently block)
            powershell -NoProfile -Command "if ((Get-MpPreference).ExclusionProcess -contains 'cargo.exe') { exit 0 } else { exit 1 }" >nul 2>&1
            if !ERRORLEVEL! EQU 0 (
                echo   %G%[OK]%N%  Defender exclusions added
                set "DEFENDER_EXCLUDED=1"
            ) else (
                echo   %Y%[!!]%N%  Exclusions may not have applied ^(Tamper Protection can block this^)
                echo          %D%Builds will work but may be slower.%N%
            )
        ) else (
            echo   %Y%[!!]%N%  Defender exclusions require admin. Requesting elevation...
            powershell -NoProfile -Command "Start-Process powershell -ArgumentList '-NoProfile -Command \"Add-MpPreference -ExclusionProcess cargo.exe,rustc.exe,link.exe -ErrorAction SilentlyContinue; Add-MpPreference -ExclusionPath $env:USERPROFILE\.cargo -ErrorAction SilentlyContinue; Add-MpPreference -ExclusionPath $env:USERPROFILE\.rustup -ErrorAction SilentlyContinue\"' -Verb RunAs -Wait" >nul 2>&1
            if !ERRORLEVEL! EQU 0 (
                echo   %G%[OK]%N%  Defender exclusions added
                set "DEFENDER_EXCLUDED=1"
            ) else (
                echo   %Y%[!!]%N%  Skipped. Builds will work but may be slower.
            )
        )
    ) else (
        echo.
        echo   %D%Keeping default Defender settings.%N%
    )
    echo.
)

:: ================================================================
::  PHASE 3: INSTALL VISUAL STUDIO BUILD TOOLS
:: ================================================================
if !MSVC_OK! EQU 0 (
    echo   %B%%C%PHASE 3: Installing Visual Studio C++ Build Tools...%N%
    echo   %C%------------------------------------------------------%N%
    echo.
    echo   %D%This is the C++ compiler that Rust needs to build programs%N%
    echo   %D%on Windows. It's made by Microsoft and is free to use.%N%
    echo   %D%The download is large ^(~1-2 GB^) and may take 10-30 minutes.%N%
    echo.

    echo   %W%Downloading installer...%N%
    set "VS_URL=https://aka.ms/vs/17/release/vs_buildtools.exe"
    set "VS_EXE=%TEMP%\vs_buildtools.exe"
    call :download "!VS_URL!" "!VS_EXE!"
    if !ERRORLEVEL! NEQ 0 (
        echo   %R%[FAIL]%N%  Could not download Visual Studio Build Tools.
        echo          %D%Check your internet connection and try again.%N%
        pause
        goto :cleanup
    )
    echo   %G%[OK]%N%  Downloaded.

    powershell -NoProfile -Command "Unblock-File -Path '!VS_EXE!'" >nul 2>&1

    echo   %W%Installing... %D%^(this will take a while, please be patient^)%N%
    echo.
    echo   %D%  A Visual Studio Installer window may appear -- that's normal.%N%
    echo   %D%  If you see a permissions prompt, click Yes.%N%
    echo.

    "!VS_EXE!" --quiet --wait --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools;includeRecommended
    set "VS_EXIT=!ERRORLEVEL!"

    del /q "!VS_EXE!" 2>nul

    if !VS_EXIT! EQU 0 (
        echo   %G%[OK]%N%  Visual Studio Build Tools installed successfully!
        set "MSVC_OK=1"
    ) else if !VS_EXIT! EQU 3010 (
        echo   %G%[OK]%N%  Build Tools installed! %Y%A restart will be needed later.%N%
        set "MSVC_OK=1"
        set "NEEDS_REBOOT=1"
    ) else if !VS_EXIT! EQU 740 (
        echo   %Y%[!!]%N%  The installer needs admin permission. Requesting elevation...
        powershell -NoProfile -Command "Start-Process '!VS_EXE!' -ArgumentList '--quiet --wait --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools;includeRecommended' -Verb RunAs -Wait" >nul 2>&1
        if !ERRORLEVEL! EQU 0 (
            echo   %G%[OK]%N%  Build Tools installed with elevation!
            set "MSVC_OK=1"
        ) else (
            echo   %R%[FAIL]%N%  Could not install Build Tools.
            echo          %D%Try downloading manually from:%N%
            echo          %C%https://visualstudio.microsoft.com/visual-cpp-build-tools/%N%
            echo          %D%Select "Desktop development with C++" and install.%N%
            echo.
            echo   %B%Press any key to try continuing anyway...%N%
            pause >nul
        )
    ) else if !VS_EXIT! EQU 1618 (
        echo   %Y%[!!]%N%  Another installer is already running.
        echo          %D%Close any Windows installers or updates and try again.%N%
        pause
        goto :cleanup
    ) else if !VS_EXIT! EQU 1602 (
        echo   %Y%[!!]%N%  Installation was cancelled.
        echo          %D%Continuing -- PitchBrick may fail to compile without Build Tools.%N%
    ) else if !VS_EXIT! EQU 1603 (
        echo   %R%[FAIL]%N%  A fatal error occurred during installation.
        echo          %D%Check logs in: %TEMP%\dd_setup_*.log%N%
        echo          %D%Try downloading manually from:%N%
        echo          %C%https://visualstudio.microsoft.com/visual-cpp-build-tools/%N%
        echo.
        echo   %B%Press any key to try continuing anyway...%N%
        pause >nul
    ) else (
        echo   %R%[FAIL]%N%  Build Tools installation failed. Exit code: !VS_EXIT!
        echo          %D%Try downloading manually from:%N%
        echo          %C%https://visualstudio.microsoft.com/visual-cpp-build-tools/%N%
        echo.
        echo   %B%Press any key to try continuing anyway...%N%
        pause >nul
    )
    echo.
) else (
    echo   %B%%C%PHASE 3: Visual Studio Build Tools%N%
    echo   %G%[OK]%N%  Already installed -- skipping.
    echo.
)

:: ================================================================
::  PHASE 4: INSTALL RUST
:: ================================================================
if !RUST_INSTALLED! EQU 0 (
    echo   %B%%C%PHASE 4: Installing Rust...%N%
    echo   %C%------------------------------------------------------%N%
    echo.
    echo   %D%Rust is the programming language PitchBrick is written in.%N%
    echo   %D%This installs the Rust compiler and the cargo package manager.%N%
    echo.

    echo   %W%Downloading Rust installer...%N%
    set "RUSTUP_URL=https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
    set "RUSTUP_EXE=%TEMP%\rustup-init.exe"
    call :download "!RUSTUP_URL!" "!RUSTUP_EXE!"
    if !ERRORLEVEL! NEQ 0 (
        echo   %R%[FAIL]%N%  Could not download Rust installer.
        echo          %D%Check your internet connection and try again.%N%
        pause
        goto :cleanup
    )

    powershell -NoProfile -Command "Unblock-File -Path '!RUSTUP_EXE!'" >nul 2>&1

    echo   %G%[OK]%N%  Downloaded.
    echo   %W%Installing Rust...%N%

    "!RUSTUP_EXE!" -y --default-toolchain stable --default-host x86_64-pc-windows-msvc --profile default
    set "RUST_EXIT=!ERRORLEVEL!"

    del /q "!RUSTUP_EXE!" 2>nul

    if !RUST_EXIT! EQU 0 (
        echo.
        echo   %G%[OK]%N%  Rust installed successfully!
        set "RUST_INSTALLED=1"
        set "PATH=!CARGO_BIN!;!PATH!"
    ) else (
        echo.
        echo   %R%[FAIL]%N%  Rust installation failed. Exit code: !RUST_EXIT!
        echo.
        echo          %D%Common causes:%N%
        echo          %D%- "existing installation" -- another Rust install is conflicting%N%
        echo          %D%- "could not download"    -- network or firewall issue%N%
        echo          %D%- "permission denied"     -- antivirus blocking the installer%N%
        echo.
        echo          %D%Try visiting https://rustup.rs and installing manually.%N%
        pause
        goto :cleanup
    )
    echo.
) else (
    echo   %B%%C%PHASE 4: Rust%N%
    echo   %G%[OK]%N%  Already installed -- updating to latest stable...
    set "PATH=!CARGO_BIN!;!PATH!"
    rustup update stable >nul 2>&1
    echo   %G%[OK]%N%  Rust is up to date.
    echo.
)

:: ================================================================
::  PHASE 5: VERIFY TOOLCHAIN
:: ================================================================
echo   %B%%C%PHASE 5: Verifying build tools...%N%
echo   %C%------------------------------------------------------%N%
echo.

:: Verify cargo is accessible
where cargo >nul 2>&1
if !ERRORLEVEL! NEQ 0 (
    if exist "!CARGO_BIN!\cargo.exe" (
        set "PATH=!CARGO_BIN!;!PATH!"
        echo   %G%[OK]%N%  Found cargo at !CARGO_BIN!
    ) else (
        echo   %R%[FAIL]%N%  cargo not found in PATH.
        echo          %D%Try closing this window, opening a new one, and running:%N%
        echo          %C%cargo install pitchbrick%N%
        pause
        goto :cleanup
    )
) else (
    echo   %G%[OK]%N%  cargo is available
)

:: Quick compile test -- use fn main(){} to avoid ! escaping issues
echo   %D%Running a quick build test...%N%
set "TEST_DIR=%TEMP%\pb_test_%RANDOM%"
mkdir "!TEST_DIR!" 2>nul
> "!TEST_DIR!\main.rs" echo fn main() {}
rustc "!TEST_DIR!\main.rs" -o "!TEST_DIR!\test.exe" >nul 2>&1
set "COMPILE_OK=!ERRORLEVEL!"
rmdir /s /q "!TEST_DIR!" 2>nul

if !COMPILE_OK! EQU 0 (
    echo   %G%[OK]%N%  Rust compiler and linker working correctly
    echo.
) else (
    echo   %Y%[!!]%N%  Compile test failed -- the C++ linker may not be ready.
    echo.
    echo          %D%This usually means the C++ Build Tools need a restart to activate.%N%
    if !MSVC_OK! EQU 0 (
        echo          %R%Visual Studio Build Tools are not installed.%N%
        echo          %D%Install them from: https://visualstudio.microsoft.com/visual-cpp-build-tools/%N%
    ) else (
        echo          %D%The Build Tools were just installed -- a restart should fix this.%N%
    )
    echo.
    echo   %B%%C%What would you like to do?%N%
    echo.
    echo     %B%%G%[C]%N%  Continue anyway ^(may work, especially after a fresh install^)
    echo     %B%%Y%[R]%N%  Exit and restart Windows first
    echo.
    choice /c CR /n /m "  Press C to continue, or R to restart later: "
    if !ERRORLEVEL! EQU 2 (
        echo.
        echo   %Y%Please restart Windows, then run this installer again.%N%
        set "NEEDS_REBOOT=1"
        goto :cleanup
    )
    echo.
    set "NEEDS_REBOOT=1"
)

:: ================================================================
::  PHASE 6: INSTALL PITCHBRICK
:: ================================================================
:install_pitchbrick
echo   %B%%C%PHASE 6: Installing PitchBrick...%N%
echo   %C%------------------------------------------------------%N%
echo.
echo   %B%%W%Downloading and compiling PitchBrick from source.%N%
echo   %D%This compiles ~200 packages and may take %B%5-20 minutes%N% %D%depending%N%
echo   %D%on your computer. You will see compilation progress below.%N%
echo.
echo   %D%Install log: !LOG_FILE!%N%
echo.

:: Show live output AND write UTF-8 log file
cargo install pitchbrick !CARGO_JOBS! 2>&1 | powershell -NoProfile -Command "$sw=[IO.StreamWriter]::new('!LOG_FILE!',$false,[Text.Encoding]::UTF8); foreach($line in $input){$sw.WriteLine($line);[Console]::WriteLine($line)}; $sw.Close()"

:: Determine success by parsing the log (pipe loses cargo's exit code)
set "PB_SUCCESS=0"
set "PB_ALREADY=0"
if exist "!LOG_FILE!" (
    findstr /i /c:"Installed package" "!LOG_FILE!" >nul 2>&1 && set "PB_SUCCESS=1"
    findstr /i /c:"Replacing" "!LOG_FILE!" >nul 2>&1 && set "PB_SUCCESS=1"
    findstr /i /c:"is already installed" "!LOG_FILE!" >nul 2>&1 && set "PB_ALREADY=1"
)

echo.
if !PB_SUCCESS! EQU 1 (
    echo   %G%[OK]%N%  PitchBrick installed successfully!
) else if !PB_ALREADY! EQU 1 (
    echo   %G%[OK]%N%  PitchBrick is already installed and up to date.
    echo.
    echo   %B%%C%Would you like to reinstall it?%N%
    echo   %D%This will recompile from source and may take 5-20 minutes.%N%
    echo.
    echo     %B%%G%[R]%N%  Reinstall ^(recompile from source^)
    echo     %B%%W%[S]%N%  Skip, keep the current version
    echo.
    choice /c RS /n /m "  Press R to reinstall, or S to skip: "
    if !ERRORLEVEL! EQU 1 (
        echo.
        echo   %D%Removing current binary...%N%
        where pitchbrick >nul 2>&1
        if !ERRORLEVEL! EQU 0 (
            for /f "tokens=*" %%p in ('where pitchbrick') do del /q "%%p" 2>nul
        )
        if exist "!CARGO_BIN!\pitchbrick.exe" del /q "!CARGO_BIN!\pitchbrick.exe" 2>nul
        echo   %G%[OK]%N%  Removed. Recompiling...
        echo.
        goto :install_pitchbrick
    )
) else (
    echo   %R%[FAIL]%N%  PitchBrick installation failed.
    echo.

    :: Diagnose specific errors from the log
    set "DIAG=0"
    set "RETRYABLE=0"

    if exist "!LOG_FILE!" (
        findstr /i /c:"link.exe" /c:"linker" "!LOG_FILE!" | findstr /i /c:"not found" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% The C++ compiler tools aren't set up yet.
            echo   %D%This usually means you need to restart Windows so the Build Tools can activate.%N%
            set "DIAG=1"
            set "NEEDS_REBOOT=1"
        )

        findstr /i /c:"LNK1181" "!LOG_FILE!" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% The linker can't find Windows system libraries.
            echo   %D%The Build Tools may need a restart to fully activate.%N%
            set "DIAG=1"
            set "NEEDS_REBOOT=1"
        )

        findstr /i /c:"LNK1104" "!LOG_FILE!" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% The linker can't find a required library ^(e.g. kernel32.lib^).
            echo   %D%The Build Tools may need a restart to fully activate.%N%
            set "DIAG=1"
            set "NEEDS_REBOOT=1"
        )

        findstr /i /c:"out of memory" /c:"0xc0000409" "!LOG_FILE!" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% Your computer ran out of memory while compiling.
            echo   %D%Try closing other programs ^(browsers, games, editors^).%N%
            echo   %D%The installer will retry with fewer parallel compile jobs.%N%
            set "DIAG=1"
            set "RETRYABLE=1"
            set "CARGO_JOBS=--jobs 2"
        )

        findstr /i /c:"Blocking waiting for file lock" "!LOG_FILE!" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% Another program is using Rust's files.
            echo   %D%Close any code editors ^(VS Code, RustRover^) or other terminals%N%
            echo   %D%that might be using cargo, then retry.%N%
            set "DIAG=1"
            set "RETRYABLE=1"
        )

        findstr /i /c:"could not download" "!LOG_FILE!" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% A download failed during compilation.
            echo   %D%Check your internet connection and try again.%N%
            set "DIAG=1"
            set "RETRYABLE=1"
        )

        findstr /i /c:"Access is denied" "!LOG_FILE!" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% A file is locked, possibly by antivirus software.
            if !DEFENDER_EXCLUDED! EQU 0 (
                echo   %D%Consider re-running this installer as admin and adding Defender exclusions.%N%
            )
            set "DIAG=1"
            set "RETRYABLE=1"
        )

        findstr /i /c:"certificate verify failed" "!LOG_FILE!" >nul 2>&1
        if !ERRORLEVEL! EQU 0 if !DIAG! EQU 0 (
            echo   %Y%Diagnosis:%N% HTTPS certificate verification failed.
            echo   %D%Your network may use a corporate proxy that intercepts HTTPS.%N%
            echo   %D%Try: set CARGO_HTTP_CHECK_REVOKE=false%N%
            echo   %D%Or configure your proxy in .cargo/config.toml%N%
            set "DIAG=1"
        )
    )

    if !DIAG! EQU 0 (
        echo   %D%No specific error pattern recognized.%N%
        echo   %D%Check the log file for details: !LOG_FILE!%N%
    )

    echo.
    if !RETRYABLE! EQU 1 (
        echo   %B%%C%Would you like to retry?%N%
        echo.
        echo     %B%%G%[R]%N%  Retry installation
        echo     %B%%R%[C]%N%  Cancel
        echo.
        choice /c RC /n /m "  Press R to retry, or C to cancel: "
        if !ERRORLEVEL! EQU 1 (
            echo.
            goto :install_pitchbrick
        )
    )

    echo.
    echo   %D%You can try again later by running:%N%
    echo   %C%cargo install pitchbrick%N%
    echo.
    pause
    goto :cleanup
)

:: ================================================================
::  PHASE 7: FINISH
:: ================================================================
echo.
echo   %B%%C%PHASE 7: Finishing up...%N%
echo   %C%------------------------------------------------------%N%
echo.

:: Find pitchbrick.exe
set "PB_PATH="
where pitchbrick >nul 2>&1
if !ERRORLEVEL! EQU 0 (
    for /f "tokens=*" %%p in ('where pitchbrick') do set "PB_PATH=%%p"
)
if not defined PB_PATH (
    if exist "!CARGO_BIN!\pitchbrick.exe" set "PB_PATH=!CARGO_BIN!\pitchbrick.exe"
)

if defined PB_PATH (
    echo   %G%[OK]%N%  PitchBrick is at: !PB_PATH!
) else (
    echo   %Y%[!!]%N%  Could not locate pitchbrick.exe
    echo          %D%Open a new terminal and type: pitchbrick%N%
)

if !NEEDS_REBOOT! EQU 1 (
    echo.
    echo   %Y%[NOTE]%N%  Some changes need a restart to take full effect.
    echo          %D%PitchBrick should work now, but restart Windows soon.%N%
)

if exist "!LOG_FILE!" (
    echo.
    echo   %D%Installation log saved to: !LOG_FILE!%N%
)

:: Smart App Control note for Win11 24H2+
if defined VER_BUILD (
    if !VER_BUILD! GEQ 26100 (
        echo.
        echo   %D%Note: On first launch, Windows Smart App Control may show a prompt.%N%
        echo   %D%This is normal for newly compiled programs -- click "Run anyway".%N%
    )
)

echo.
echo   %B%%C%======================================================%N%
echo   %B%%G%        INSTALLATION COMPLETE!                        %N%
echo   %B%%C%======================================================%N%
echo.
echo   %W%To launch PitchBrick:%N%
echo.
echo     %B%Option 1:%N%  Press %B%Win+R%N%, type %C%pitchbrick%N%, press Enter
echo     %B%Option 2:%N%  Search %C%PitchBrick%N% in the Start Menu
echo     %B%Option 3:%N%  Open a terminal and type %C%pitchbrick%N%
echo.
echo   %D%PitchBrick will create a tiny always-on-top window.%N%
echo   %D%Right-click the tray icon ^(bottom-right of taskbar^) for settings.%N%
echo   %D%Config file: %USERPROFILE%\pitchbrick.toml%N%
echo.
echo   %B%%C%======================================================%N%
echo.

:: Ask to launch
echo   %B%Would you like to launch PitchBrick now?%N%
echo.
echo     %B%%G%[Y]%N%  Yes, launch it
echo     %B%%W%[N]%N%  No, I'll launch it later
echo.
choice /c YN /n /m "  Press Y or N: "
if !ERRORLEVEL! EQU 1 (
    if defined PB_PATH (
        echo.
        echo   %G%Launching PitchBrick...%N%
        start "" "!PB_PATH!"
    ) else (
        echo.
        echo   %Y%Could not find pitchbrick.exe. Open a new terminal and type: pitchbrick%N%
    )
)

goto :cleanup

:: ================================================================
::  SUBROUTINES
:: ================================================================

:download
:: %~1 = URL, %~2 = output file path
:: Returns: ERRORLEVEL 0 on success
where curl.exe >nul 2>&1
if !ERRORLEVEL! EQU 0 (
    curl.exe -L --retry 3 --retry-delay 5 -o "%~2" "%~1"
    if !ERRORLEVEL! EQU 0 exit /b 0
)
:: Fallback to PowerShell
powershell -NoProfile -Command "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri '%~1' -OutFile '%~2' -UseBasicParsing"
exit /b !ERRORLEVEL!

:: ================================================================
::  CLEANUP
:: ================================================================
:cleanup
echo.

:: Restore VirtualTerminalLevel if it wasn't set before
if not defined VT_WAS (
    reg delete "HKCU\Console" /v VirtualTerminalLevel /f >nul 2>&1
)

endlocal
pause
exit /b 0
