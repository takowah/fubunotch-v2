@echo off
copy /y "%APPDATA%\Kanmuri\settings.json" "%~dp0real-settings.log" >nul
dir /b "%APPDATA%\Kanmuri" > "%~dp0real-dir.log"

