# Qubit Command

[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Documentation](https://img.shields.io/badge/docs-English-blue.svg)](README.md)

面向 Qubit Rust 项目的命令行进程运行工具库。

## 概览

Qubit Command 提供一个小而明确的结构化 API，用于运行外部程序、捕获
stdout/stderr、控制超时，并用清晰的错误类型报告命令执行失败。

本 crate 刻意独立于 `qubit-concurrent`：运行外部命令属于进程管理，不是通用任务执行策略。
