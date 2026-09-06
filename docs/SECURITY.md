# Security policy

## Supported versions

Security fixes target the latest tip of the `dev` branch and any tagged releases
published from this repository.

## Reporting a vulnerability

Do **not** open a public GitHub issue for an unfixed vulnerability.

Prefer a private [GitHub Security Advisory](https://github.com/Interchouette-ITC/InterFire/security/advisories/new)
on this repository when available. Otherwise email
[contact@interchouette.net](mailto:contact@interchouette.net) with a clear
description, impact, and reproduction steps when possible.

We will acknowledge receipt and follow up. Do not expect a fixed SLA.

## Privileged components

InterFire is intended to run a privileged daemon, eBPF programs, and an
InterFire-owned nftables table. Treat rule files, Unix-socket credentials, and
kernel hooks as sensitive. Never commit host secrets, socket paths with live
credentials, or capability dumps from production machines.
