@echo off
rem Runs switch-from-mini-island.ps1 outside the Claude app's container (start it via explorer.exe
rem or by double-click). Output: switch-from-mini-island.log next to this file.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0switch-from-mini-island.ps1" > "%~dp0switch-from-mini-island.log" 2>&1
