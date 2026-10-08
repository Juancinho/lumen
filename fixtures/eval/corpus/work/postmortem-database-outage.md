# Postmortem: API outage on 12 August

Impact: the public API returned errors for 37 minutes.

Timeline: at 09:12 the API pods started logging `connect ECONNREFUSED 127.0.0.1:5432`.
The database itself was healthy; the connection pool in the sidecar proxy was exhausted
after a configuration change halved its size.

Root cause: a pool size of 10 shipped with the new proxy image.
Fix: pool size restored to 40, alert added on pool saturation.
Lessons: configuration changes need the same review as code.
