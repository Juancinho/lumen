# Fixing a broken network connection on Windows

1. Run the network troubleshooter from Settings.
2. In an administrator terminal: `ipconfig /release`, `ipconfig /renew`, `ipconfig /flushdns`.
3. Reset the stack: `netsh winsock reset` and `netsh int ip reset`, then restart.
4. Still offline? Settings > Network > Advanced > Network reset reinstalls the adapters.
