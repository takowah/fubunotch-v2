@echo off
rem Copies the real settings and log (outside Claude's container) next to this file for a look.
copy /y "%APPDATA%\Kanmuri\settings.json" "%~dp0real-settings.log" >nul
copy /y "%APPDATA%\Kanmuri\kanmuri.log" "%~dp0real-log.log" >nul
dir "%APPDATA%\Kanmuri" > "%~dp0real-dir.log"
