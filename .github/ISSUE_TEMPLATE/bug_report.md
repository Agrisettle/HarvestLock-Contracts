---
name: Bug report
about: A failing test, a real testnet transaction that behaved wrong, or a doc claim the code contradicts
title: ""
labels: bug
assignees: ""
---

## What's wrong

<!-- One or two sentences. -->

## Where

<!-- File/line in contracts/escrow/src/lib.rs (or test.rs), or a real testnet transaction hash / contract address if this is a live-behavior bug, not a unit-test one. -->

## How to reproduce

<!-- A failing `cargo test` case is the strongest report. If it's only reproducible live, give the exact `stellar contract invoke` sequence and network (should always be testnet -- see HANDOFF.md's "All deployed instances are validation artifacts" note). -->

## What you expected vs. what happened

## Anything you already checked

<!-- This project verifies claims before filing them (see any existing issue's style) -- if you confirmed this against lib.rs's own doc comments or HANDOFF.md before filing, say so. -->
