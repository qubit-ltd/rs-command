# Qubit Command

[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

Command-line process running utilities for Qubit Rust projects.

## Overview

Qubit Command provides a small, structured API for running external programs,
capturing their output, enforcing timeouts, and reporting command failures with
clear error values.

This crate is intentionally separate from `qubit-concurrent`: running an
external command is process management, not a generic task execution strategy.

