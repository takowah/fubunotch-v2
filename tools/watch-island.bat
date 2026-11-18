@echo off
rem Records the island for 4 minutes (tools\watch). Start via explorer.exe.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0watch-island.ps1" > "%~dp0watch-island.log" 2>&1
