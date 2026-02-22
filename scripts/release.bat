@echo off
setlocal enabledelayedexpansion

echo Building OCLAWS release...

REM Get version from Cargo.toml
for /f "tokens=2 delims==" %%a in ('findstr /C:"version = " Cargo.toml') do (
    set VERSION=%%a
)
set VERSION=%VERSION:~1,-1%

echo Version: %VERSION%

REM Build release
echo Building release binary...
cargo build --release

REM Create release directory
if not exist release mkdir release

REM Copy binary
copy target\release\oclaws.exe release\oclaws-%VERSION%-windows-x64.exe

echo Release files created:
dir release

echo Done!
