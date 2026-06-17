Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
root = fso.GetParentFolderName(WScript.ScriptFullName)
port = "9001"
url = "http://127.0.0.1:" & port

candidates = Array( _
    shell.ExpandEnvironmentStrings("%ProgramFiles(x86)%") & "\Microsoft\Edge\Application\msedge.exe", _
    shell.ExpandEnvironmentStrings("%ProgramFiles%") & "\Microsoft\Edge\Application\msedge.exe", _
    shell.ExpandEnvironmentStrings("%ProgramFiles%") & "\Google\Chrome\Application\chrome.exe", _
    shell.ExpandEnvironmentStrings("%ProgramFiles(x86)%") & "\Google\Chrome\Application\chrome.exe" _
)

For Each browser In candidates
    If fso.FileExists(browser) Then
        shell.Run """" & browser & """ --app=" & url, 1, False
        WScript.Quit 0
    End If
Next

shell.Run url, 1, False
