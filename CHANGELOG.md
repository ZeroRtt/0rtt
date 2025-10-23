# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org).

<!--
Note: In this file, do not use the hard wrap in the middle of a sentence for
compatibility with GitHub comment style markdown rendering.
-->

## [0.1.17] - 2025-10-23

- quic: bump version to `0.1.17`. fixed `stream shutdown` bug.

## [0.1.16] - 2025-09-13

- o3: Move `it` to a separate [`repository`](https://github.com/ZeroRtt/o3).
- fixedbuf,ringbuf: bump version.
- zrquic: rename crate to `zerortt`.

## [0.1.15] - 2025-09-13

- o3/cli: change default values: `--cc`=bbr, `--mtu`=12000, `--max-ack-delay`=30

## [0.1.14] - 2025-09-13

- o3/cli: new option `cc`, set the congestion control algorithm with it.

## [0.1.13] - 2025-09-12

- o3: add `send` gcongestion.

## [0.1.12] - 2025-09-12

- o3: add `meterics` report.

## [0.1.11] - 2025-09-11

- rollback v0.1.9

## [0.1.10] - 2025-09-11

- o3: Once the pipeline has been established, execute a data transfer immediately.

## [0.1.9] - 2025-09-11

- quico: Optimize the `readiness` event notification mechanism

## [0.1.8] - 2025-09-11

- fixedbuf: remove `no_std` support.

## [0.1.7] - 2025-09-11

- o3: port remove `fin` fn and add `close` fn.

## [0.1.6] - 2025-09-11

- quico: Try fix `Connector` bug.

## [0.1.5] - 2025-09-10

- o3: Improving the tunnel closure procedure

## [0.1.4] - 2025-09-10

- o3: Improving the tunnel closure procedure

## [0.1.3] - 2025-09-10

- o3: add more command line parameters.

## [0.1.2] - 2025-09-10

- o3: Once the pipeline has been established, execute a data transfer immediately.

## [0.1.1] - 2025-09-09

- Fix quic `release_time` bug.

## [0.1.0] - 2025-09-09

- First working version
