@echo off
rem Copies the real settings and log (outside Claude's container) next to this file for a look.
copy /y "%APPDATA%\FubuNotch v2\settings.json" "%~dp0real-settings.log" >nul
copy /y "%APPDATA%\FubuNotch v2\fubunotch-v2.log" "%~dp0real-log.log" >nul
dir "%APPDATA%\FubuNotch v2" > "%~dp0real-dir.log"
