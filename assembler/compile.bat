@echo off
rem Drag-and-drop assembler: drop one or more .asm files onto this script,
rem or run: compile.bat a.asm b.asm ...
rem Each input X.asm produces X.hex next to it.
setlocal
if "%~1"=="" (
  echo Usage: drag one or more .asm files onto compile.bat, or: compile.bat ^<a.asm^> [b.asm ...]
  pause
  exit /b 1
)
cd /d "%~dp0"
set "BIN=target\release\assembler.exe"
if not exist "%BIN%" (
  echo Building release binary...
  cargo build --release
  if errorlevel 1 ( echo cargo build failed. & pause & exit /b 1 )
)
set "FAILED=0"
:loop
if "%~1"=="" goto :done
call :compile_one "%~1"
if errorlevel 1 set "FAILED=1"
shift
goto :loop
:done
echo.
if "%FAILED%"=="1" ( echo Some files FAILED. ) else ( echo All files compiled OK. )
pause
exit /b %FAILED%

:compile_one
set "IN=%~f1"
set "OUT=%~dpn1.hex"
echo --- %IN%
"%BIN%" "%IN%" "%OUT%"
if errorlevel 1 ( echo FAILED: %IN% & exit /b 1 )
echo OK -^> %OUT%
exit /b 0
