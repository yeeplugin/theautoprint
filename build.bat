@echo off
echo ============================================================
echo YeePrint Desktop Client - Windows Auto Builder
echo ============================================================
echo.

:: Check for Node.js
where node >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] Node.js is not installed!
    echo Please download and install Node.js from: https://nodejs.org/
    echo.
    pause
    exit /b 1
)

:: Check for Rust
where cargo >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] Rust is not installed!
    echo Please download and install Rust from: https://rustup.rs/
    echo.
    pause
    exit /b 1
)

echo [1/3] Installing dependencies...
call npm install
if %errorlevel% neq 0 (
    echo [ERROR] npm install failed!
    pause
    exit /b 1
)

echo.
echo [2/3] Building YeePrint Desktop Client for Windows...
call npm run tauri build
if %errorlevel% neq 0 (
    echo [ERROR] Tauri build failed!
    pause
    exit /b 1
)

echo.
echo [3/3] Build completed successfully!
echo.
echo Location of installer (.exe):
echo src-tauri\target\release\bundle\nsis\
echo.
pause
