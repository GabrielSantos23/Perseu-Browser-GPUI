# Answers run.mjs one line at a time on stdin/stdout:
#   window <pid> <timeoutMs>  ->  epoch ms of the first visible window (-1 on timeout)
#   stats <pid>               ->  pid:cpuMs:contextSwitches:privateBytes for it and each descendant
#   close <pid>               ->  how many windows of it and its descendants were asked to close
#   key <combo>               ->  types one key chord (`ctrl+t`, `ctrl+v`, `enter`, `1`...), like a user would
#   clip <text>               ->  puts `text` on the clipboard, so a chord can paste a URL instead of typing it
Add-Type -Path (Join-Path $PSScriptRoot 'probe.cs')
[Console]::Out.WriteLine('ready')
while ($null -ne ($line = [Console]::In.ReadLine())) {
  $parts = $line.Split(' ')
  switch ($parts[0]) {
    'window' { [Console]::Out.WriteLine([Probe]::WaitForWindow([int]$parts[1], [int]$parts[2])) }
    'stats' { [Console]::Out.WriteLine([Probe]::Stats([int]$parts[1])) }
    'close' { [Console]::Out.WriteLine([Probe]::Close([int]$parts[1])) }
    'key' { [Console]::Out.WriteLine([Probe]::Key($parts[1])) }
    'clip' { Set-Clipboard -Value ($parts[1..($parts.Length - 1)] -join ' '); [Console]::Out.WriteLine('ok') }
    default { [Console]::Out.WriteLine('?') }
  }
}
