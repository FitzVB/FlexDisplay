Set fso = CreateObject("Scripting.FileSystemObject")
Set shell = CreateObject("WScript.Shell")
root = fso.GetParentFolderName(WScript.ScriptFullName)
ps1 = root & "\scripts\start-app.ps1"
cmd = "powershell.exe -WindowStyle Hidden -ExecutionPolicy Bypass -NoProfile -File """ & ps1 & """ -Mode WiFi"
shell.Run cmd, 0, False
