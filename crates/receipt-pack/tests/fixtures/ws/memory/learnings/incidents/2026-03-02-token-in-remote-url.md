---
type: incident
tags: [git, credentials]
date: 2026-03-02
---
# Token pasted into a remote URL

## What happened

A push over HTTPS was rejected, so a token was pasted into the remote URL to get the
commit through. The token then showed up in shell history and in `git remote -v`.

## Do instead

Configure a credential helper and push again. Never put a token in the remote URL.
