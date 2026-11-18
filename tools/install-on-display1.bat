@echo off
rem Installs the release build as Kanmuri (switch-from-mini-island), then keeps the island on DISPLAY1. Start via explorer.exe.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0switch-from-mini-island.ps1" > "%~dp0install-on-display1.log" 2>&1

