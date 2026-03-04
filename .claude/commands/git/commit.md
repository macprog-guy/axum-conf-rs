---
name: OpenSpec: Commit
description: Commits all changes
category: Git
tags: [git,commit]
---

## Context

- Current git status: !`git status`
- Current git diff (staged and unstaged changes): !`git diff HEAD`
- Current branch: !`git branch --show-current`
- Recent commits: !`git log --oneline -10`

## Your task

1. Analyze the diff content to understand the nature and purpose of the changes
2. Generate commit message based on the changes
   - It should be concise, clear, and capture the essence of the changes
   - Prefer Conventional Commits format (feat:, fix:, docs:, refactor:, etc.)
   - It should contain a nice summary
3. Stage changes if necessary using git add
4. Execute git commit using the selected commit message

## Constraints

- DO NOT add Claude co-authorship footer to commits