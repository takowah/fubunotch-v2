@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0set-auto-screen.ps1" %* > "%~dp0set-auto-screen.log" 2>&1
