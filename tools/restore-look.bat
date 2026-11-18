@echo off
powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-Process kanmuri -EA SilentlyContinue | Stop-Process -Force; Start-Sleep 1; & '%~dp0restore-look.ps1'" > "%~dp0restore-look.log" 2>&1
