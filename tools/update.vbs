' Runs update.ps1 without a window (the "Kanmuri update" task starts this).
Set sh = CreateObject("WScript.Shell")
here = CreateObject("Scripting.FileSystemObject").GetParentFolderName(WScript.ScriptFullName)
sh.Run "powershell -NoProfile -ExecutionPolicy Bypass -File """ & here & "\update.ps1""", 0, False
