---
type: decision
date: 2026-02-14
---
# Decision: HTTPS remotes with a credential helper

We push to GitHub over HTTPS with the system credential helper. SSH keys are not
provisioned on the build hosts, so SSH remotes are not an option there.
