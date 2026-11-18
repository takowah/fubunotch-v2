@echo off
findstr /c:"moved on its own" /c:"start" "%APPDATA%\Kanmuri\kanmuri.log" > "%~dp0real-log.log"
